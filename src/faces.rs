//! Faces: SCRFD detection, alignment by 5 points, ArcFace embeddings and clusters.
//!
//! The models live in `<models_dir>/faces/`, are downloaded on consent only and checked by sha256.
//! No face, no embedding and no path ever leaves the machine (ADR 0011, 0021).

use crate::db::{self, FaceRow, Progress};
use crate::derive;
use crate::embed::{self, ModelFile};
use crate::i18n::{self, Lang};
use anyhow::{Context, Result, bail};
use image::{DynamicImage, RgbImage};
use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};

pub const MODEL_ID: &str = "buffalo_l";
pub const MODEL_DIR: &str = "faces";
/// The detector input: 640×640 keeping the proportions, with black margins.
pub const DET_SIDE: u32 = 640;
/// The ArcFace input: an aligned 112×112 frame.
pub const FACE_SIDE: u32 = 112;
pub const EMBED_DIM: usize = 512;
pub const DET_THRESHOLD: f32 = 0.5;
const NMS_IOU: f32 = 0.4;
/// Very small faces give no usable embedding — they are not counted.
const MIN_FACE_PX: f32 = 28.0;

const FILES: [ModelFile; 2] = [
    ModelFile {
        name: "det.onnx",
        url: "https://huggingface.co/immich-app/buffalo_l/resolve/main/detection/model.onnx",
        sha256: "5838f7fe053675b1c7a08b633df49e7af5495cee0493c7dcf6697200b85b5b91",
        bytes: 16_923_827,
    },
    ModelFile {
        name: "rec.onnx",
        url: "https://huggingface.co/immich-app/buffalo_l/resolve/main/recognition/model.onnx",
        sha256: "4c06341c33c2ca1f86781dab0e829f88ad5b64be9fba56e56bc9ebdefc619e43",
        bytes: 174_383_860,
    },
];

/// The canonical ArcFace points: eyes, nose, mouth corners in a 112×112 frame.
const ARC_DST: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

pub fn model_dir(settings: &db::Settings) -> PathBuf {
    settings.models_dir.join(MODEL_DIR)
}

pub fn model_present(settings: &db::Settings) -> bool {
    FILES.iter().all(|f| model_dir(settings).join(f.name).is_file())
}

pub fn model_bytes(settings: &db::Settings) -> i64 {
    FILES
        .iter()
        .filter_map(|f| std::fs::metadata(model_dir(settings).join(f.name)).ok())
        .map(|m| m.len() as i64)
        .sum()
}

pub fn check_model(settings: &db::Settings) -> Result<()> {
    for f in &FILES {
        let path = model_dir(settings).join(f.name);
        if !path.is_file() {
            bail!(
                "the face model files are missing: {}\ndownload them in the settings (with consent) or: \
                 `photocat faces download --yes`",
                path.display()
            );
        }
        let got = derive::sha256_file(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        if got != f.sha256 {
            bail!(
                "the face model file is damaged ({}): {}\nsha256 {got}, expected {}",
                f.name,
                path.display(),
                f.sha256
            );
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct DownloadReport {
    pub files: usize,
    /// The language of the report (M21.6): English by default.
    pub lang: Lang,
}

impl std::fmt::Display for DownloadReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = i18n::fill(
            self.lang,
            "face model downloaded: {} files, {}",
            &[
                self.files.to_string(),
                db::human_in(self.lang, FILES.iter().map(|x| x.bytes as i64).sum()),
            ],
        );
        f.write_str(&text)
    }
}

/// Downloading the face weights the same way as CLIP: consent only, the built-in HTTP client, sha256.
pub fn download_model(
    settings: &db::Settings,
    consent: bool,
    progress: Option<Arc<Progress>>,
) -> Result<DownloadReport> {
    if !consent {
        bail!("{}", i18n::t(settings.lang, "downloading the face weights needs an explicit consent (the checkbox in the settings or --yes)"));
    }
    let bytes: u64 = FILES.iter().map(|f| f.bytes).sum();
    if bytes > settings.max_model_mb.saturating_mul(1024 * 1024) {
        bail!(
            "{}",
            i18n::fill(
                settings.lang,
                "the face model {} is bigger than the limit of {} MiB — raise the limit in the settings",
                &[db::human(bytes as i64), settings.max_model_mb.to_string()]
            )
        );
    }
    let files = embed::download_files(
        settings,
        &model_dir(settings),
        "faces buffalo_l",
        &FILES,
        progress,
    )?;
    Ok(DownloadReport {
        files,
        lang: settings.lang,
    })
}

fn oe<R>(e: ort::Error<R>) -> anyhow::Error {
    anyhow::anyhow!("face model: {e}")
}

/// One model per process: a shared ONNX context and two sessions.
pub struct FaceModel {
    _env: Environment,
    det: Session,
    rec: Session,
}

/// A detected face before the DB write: the box in original pixels and the aligned frame.
pub struct DetectedFace {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub score: f32,
    pub quality: f32,
    pub aligned: RgbImage,
    pub embedding: Vec<f32>,
}

impl FaceModel {
    pub fn load(settings: &db::Settings, threads: usize) -> Result<Self> {
        check_model(settings)?;
        let dir = model_dir(settings);
        let threads = threads.max(1);
        let env = embed::shared_environment()?;
        let det = Session::builder(&env)
            .map_err(oe)?
            .with_intra_threads(threads)
            .map_err(oe)?
            .commit_from_file(dir.join("det.onnx"))
            .map_err(oe)?;
        let rec = Session::builder(&env)
            .map_err(oe)?
            .with_intra_threads(threads)
            .map_err(oe)?
            .commit_from_file(dir.join("rec.onnx"))
            .map_err(oe)?;
        Ok(Self { _env: env, det, rec })
    }

    /// Faces on a frame: detection, NMS, alignment, embedding. The frame is already EXIF-rotated.
    pub fn analyze(&mut self, img: &DynamicImage) -> Result<Vec<DetectedFace>> {
        let (w, h) = (img.width(), img.height());
        if w < 64 || h < 64 {
            return Ok(Vec::new());
        }
        let (blob, scale) = detect_blob(img);
        let outputs = self
            .det
            .run(ort::inputs![Tensor::from_array((
                vec![1i64, 3, DET_SIDE as i64, DET_SIDE as i64],
                blob,
            ))
            .map_err(oe)?])
            .map_err(oe)?;

        // SCRFD gives three heads per stride: score [N,1], bbox [N,4], kps [N,10].
        let mut levels: HashMap<usize, [Option<Vec<f32>>; 3]> = HashMap::new();
        for (_, value) in outputs.iter() {
            let (shape, data) = value.try_extract_tensor::<f32>().map_err(oe)?;
            let count = *shape.first().unwrap_or(&0) as usize;
            let last = *shape.last().unwrap_or(&0);
            let slot = match last {
                1 => 0,
                4 => 1,
                10 => 2,
                _ => continue,
            };
            let entry = levels.entry(count).or_insert([None, None, None]);
            entry[slot] = Some(data.to_vec());
        }
        drop(outputs);
        let mut counts: Vec<usize> = levels.keys().copied().collect();
        counts.sort_unstable_by(|a, b| b.cmp(a)); // the coarse map (stride 8) first
        let mut boxes: Vec<RawBox> = Vec::new();
        for (level, count) in counts.into_iter().enumerate() {
            let Some([Some(scores), Some(bbox), Some(kps)]) = levels.get(&count).cloned() else {
                continue;
            };
            // SCRFD uses 2 anchors per cell: N = 2·(640/stride)².
            let stride = 8.0 * 2f32.powi(level as i32);
            let fw = (DET_SIDE as f32 / stride).round().max(1.0) as usize;
            let cells = fw * fw;
            let anchors = if cells > 0 && count % cells == 0 {
                count / cells
            } else {
                1
            };
            decode_level(
                count, stride, fw, anchors, &scores, &bbox, &kps, scale, w, h, &mut boxes,
            );
        }
        let boxes = nms(boxes, NMS_IOU);
        if boxes.is_empty() {
            return Ok(Vec::new());
        }

        let rgb = img.to_rgb8();
        let mut out = Vec::with_capacity(boxes.len());
        for b in boxes {
            let aligned = align_face(&rgb, &b.kps);
            let embedding = self.embed(&aligned)?;
            let (x0, y0, x1, y1) = (b.rect[0], b.rect[1], b.rect[2], b.rect[3]);
            out.push(DetectedFace {
                x: x0.round() as i32,
                y: y0.round() as i32,
                w: (x1 - x0).round().max(1.0) as i32,
                h: (y1 - y0).round().max(1.0) as i32,
                score: b.score,
                quality: b.score * (x1 - x0).min(y1 - y0),
                aligned,
                embedding,
            });
        }
        Ok(out)
    }

    fn embed(&mut self, aligned: &RgbImage) -> Result<Vec<f32>> {
        let plane = (FACE_SIDE * FACE_SIDE) as usize;
        let mut data = vec![0f32; plane * 3];
        for (x, y, px) in aligned.enumerate_pixels() {
            let i = (y * FACE_SIDE + x) as usize;
            for c in 0..3 {
                data[c * plane + i] = (px[c] as f32 - 127.5) / 128.0;
            }
        }
        let outputs = self
            .rec
            .run(ort::inputs![Tensor::from_array((
                vec![1i64, 3, FACE_SIDE as i64, FACE_SIDE as i64],
                data,
            ))
            .map_err(oe)?])
            .map_err(oe)?;
        let (_, value) = outputs
            .iter()
            .next()
            .context("ArcFace returned no vector")?;
        let (shape, data) = value.try_extract_tensor::<f32>().map_err(oe)?;
        let dim = *shape.last().unwrap_or(&0) as usize;
        if dim != EMBED_DIM {
            bail!("ArcFace returned {dim} features, {EMBED_DIM} expected");
        }
        let mut v = data.to_vec();
        normalize(&mut v);
        Ok(v)
    }
}

/// A letterbox into 640×640 keeping the proportions: black margins right and bottom, as in insightface.
fn detect_blob(img: &DynamicImage) -> (Vec<f32>, f32) {
    let (w, h) = (img.width().max(1), img.height().max(1));
    let scale = DET_SIDE as f32 / w.max(h) as f32;
    let nw = ((w as f32 * scale).round() as u32).clamp(1, DET_SIDE);
    let nh = ((h as f32 * scale).round() as u32).clamp(1, DET_SIDE);
    let resized = img.resize_exact(nw, nh, image::imageops::FilterType::Triangle);
    let rgb = resized.to_rgb8();
    let side = DET_SIDE as usize;
    let plane = side * side;
    let mut data = vec![0f32; plane * 3];
    for (x, y, px) in rgb.enumerate_pixels() {
        let i = y as usize * side + x as usize;
        for c in 0..3 {
            // BGR is not needed here: the model was trained on RGB (blobFromImage + swapRB).
            data[c * plane + i] = (px[c] as f32 - 127.5) / 128.0;
        }
    }
    (data, scale)
}

struct RawBox {
    rect: [f32; 4],
    score: f32,
    kps: [[f32; 2]; 5],
}

/// Parse one SCRFD head: anchor centres, distance2bbox, key points.
#[allow(clippy::too_many_arguments)]
fn decode_level(
    count: usize,
    stride: f32,
    fw: usize,
    anchors: usize,
    scores: &[f32],
    bbox: &[f32],
    kps: &[f32],
    scale: f32,
    img_w: u32,
    img_h: u32,
    out: &mut Vec<RawBox>,
) {
    let inv = 1.0 / scale.max(1e-6);
    for i in 0..count {
        let score = scores.get(i).copied().unwrap_or(0.0);
        if score < DET_THRESHOLD {
            continue;
        }
        let cell = i / anchors.max(1);
        let cx = (cell % fw) as f32 * stride;
        let cy = (cell / fw) as f32 * stride;
        let d = &bbox[i * 4..i * 4 + 4];
        if d.len() < 4 {
            continue;
        }
        let x1 = (cx - d[0] * stride) * inv;
        let y1 = (cy - d[1] * stride) * inv;
        let x2 = (cx + d[2] * stride) * inv;
        let y2 = (cy + d[3] * stride) * inv;
        let x1 = x1.clamp(0.0, img_w as f32);
        let y1 = y1.clamp(0.0, img_h as f32);
        let x2 = x2.clamp(0.0, img_w as f32);
        let y2 = y2.clamp(0.0, img_h as f32);
        if x2 - x1 < MIN_FACE_PX || y2 - y1 < MIN_FACE_PX {
            continue;
        }
        let mut pts = [[0f32; 2]; 5];
        for (j, pt) in pts.iter_mut().enumerate() {
            let dx = kps.get(i * 10 + j * 2).copied().unwrap_or(0.0);
            let dy = kps.get(i * 10 + j * 2 + 1).copied().unwrap_or(0.0);
            pt[0] = (cx + dx * stride) * inv;
            pt[1] = (cy + dy * stride) * inv;
        }
        out.push(RawBox {
            rect: [x1, y1, x2, y2],
            score,
            kps: pts,
        });
    }
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let x1 = a[0].max(b[0]);
    let y1 = a[1].max(b[1]);
    let x2 = a[2].min(b[2]);
    let y2 = a[3].min(b[3]);
    let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let area_a = (a[2] - a[0]).max(0.0) * (a[3] - a[1]).max(0.0);
    let area_b = (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    if area_a + area_b - inter <= 0.0 {
        0.0
    } else {
        inter / (area_a + area_b - inter)
    }
}

fn nms(mut boxes: Vec<RawBox>, threshold: f32) -> Vec<RawBox> {
    boxes.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<RawBox> = Vec::new();
    for b in boxes {
        if kept.iter().all(|k| iou(&k.rect, &b.rect) <= threshold) {
            kept.push(b);
        }
    }
    kept
}

/// The similarity matrix (scale and rotation, no reflection) that maps `src` onto `dst`.
/// Returns `(a, b, tx, ty)`: x' = a·x − b·y + tx, y' = b·x + a·y + ty.
fn similarity(src: &[[f32; 2]; 5], dst: &[[f32; 2]; 5]) -> (f32, f32, f32, f32) {
    let n = src.len() as f32;
    let mp = src.iter().fold([0f32; 2], |m, p| [m[0] + p[0] / n, m[1] + p[1] / n]);
    let mq = dst.iter().fold([0f32; 2], |m, p| [m[0] + p[0] / n, m[1] + p[1] / n]);
    let mut num = 0f32;
    let mut den = 0f32;
    for (p, q) in src.iter().zip(dst) {
        let px = p[0] - mp[0];
        let py = p[1] - mp[1];
        let qx = q[0] - mq[0];
        let qy = q[1] - mq[1];
        // (px + i·py)* · (qx + i·qy) = px·qx + py·qy + i·(px·qy − py·qx)
        num += px * qy - py * qx;
        den += px * qx + py * qy;
    }
    // The complex quotient: (den + i·num) / Σ|p|².
    let norm = src
        .iter()
        .map(|p| (p[0] - mp[0]).powi(2) + (p[1] - mp[1]).powi(2))
        .sum::<f32>()
        .max(1e-6);
    let c = den / norm;
    let s = num / norm;
    // x' + i·y' = (c + i·s)(x + i·y) = (c·x − s·y) + i·(s·x + c·y)
    let tx = mq[0] - (c * mp[0] - s * mp[1]);
    let ty = mq[1] - (s * mp[0] + c * mp[1]);
    (c, s, tx, ty)
}

/// Align a face by its 5 points into a 112×112 frame with bilinear sampling.
fn align_face(src: &RgbImage, kps: &[[f32; 2]; 5]) -> RgbImage {
    let (a, b, tx, ty) = similarity(kps, &ARC_DST);
    let det = (a * a + b * b).max(1e-6);
    let (sw, sh) = src.dimensions();
    let mut out = RgbImage::new(FACE_SIDE, FACE_SIDE);
    for v in 0..FACE_SIDE {
        for u in 0..FACE_SIDE {
            let dx = u as f32 + 0.5 - tx;
            let dy = v as f32 + 0.5 - ty;
            // The inverse similarity matrix: 1/(a²+b²) · [a  b; −b  a].
            let x = (a * dx + b * dy) / det;
            let y = (-b * dx + a * dy) / det;
            out.put_pixel(u, v, bilinear(src, x, y, sw, sh));
        }
    }
    out
}

fn bilinear(src: &RgbImage, x: f32, y: f32, w: u32, h: u32) -> image::Rgb<u8> {
    let x = x.clamp(0.0, (w.saturating_sub(1)) as f32);
    let y = y.clamp(0.0, (h.saturating_sub(1)) as f32);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(w.saturating_sub(1));
    let y1 = (y0 + 1).min(h.saturating_sub(1));
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let p00 = src.get_pixel(x0, y0);
    let p10 = src.get_pixel(x1, y0);
    let p01 = src.get_pixel(x0, y1);
    let p11 = src.get_pixel(x1, y1);
    let mut px = [0u8; 3];
    for c in 0..3 {
        let top = p00[c] as f32 * (1.0 - fx) + p10[c] as f32 * fx;
        let bottom = p01[c] as f32 * (1.0 - fx) + p11[c] as f32 * fx;
        px[c] = (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
    }
    image::Rgb(px)
}

fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 1e-9 {
        for x in v.iter_mut() {
            *x /= n;
        }
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[derive(Debug, Clone, Copy)]
pub struct Plan {
    pub images: usize,
    pub todo: usize,
    /// Photos the pass did not take because their catalog has faces off (M26.9).
    pub pass_skipped: usize,
}

pub fn plan(db_path: &Path) -> Result<Plan> {
    let conn = db::open(db_path)?;
    let settings = db::load_settings(&conn);
    // A switched-off folder is not planned for (M26.1); a catalog with faces off is counted apart
    // and left alone (M26.9).
    let rows: Vec<db::FileRow> = db::load_all(&conn)?
        .into_iter()
        .filter(|r| !settings.path_hidden(&r.path))
        .collect();
    let pass_skipped = rows
        .iter()
        .filter(|r| r.kind == "image" && !settings.pass_allows(&r.path, db::Pass::Faces))
        .count();
    let rows: Vec<db::FileRow> = rows
        .into_iter()
        .filter(|r| settings.pass_allows(&r.path, db::Pass::Faces))
        .collect();
    let scans = db::face_scan_shas(&conn)?;
    let images = rows.iter().filter(|r| r.kind == "image").count();
    let todo = rows
        .iter()
        .filter(|r| r.kind == "image" && scans.get(&r.id) != Some(&r.sha256))
        .count();
    Ok(Plan {
        images,
        todo,
        pass_skipped,
    })
}

pub struct Stats {
    pub images: usize,
    pub done: usize,
    pub skipped: usize,
    /// Photos the pass did not take because their catalog has faces off (M26.9).
    pub pass_skipped: usize,
    pub faces: usize,
    pub clusters: usize,
    /// What the matching step did after the clustering (M26.11).
    pub matched: MatchReport,
    pub errors: usize,
    pub seconds: f64,
    pub cancelled: bool,
    /// The language of the report (M21.6): the caller sets it before printing; English by default.
    pub lang: Lang,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let prefix = i18n::t(
            self.lang,
            if self.cancelled {
                "stopped: "
            } else {
                ""
            },
        );
        let body = i18n::fill(
            self.lang,
            "scanned {} of {} photos, {} faces, {} clusters, {} errors, {} s",
            &[
                self.done.to_string(),
                self.images.to_string(),
                self.faces.to_string(),
                self.clusters.to_string(),
                self.errors.to_string(),
                format!("{:.1}", self.seconds),
            ],
        );
        write!(f, "{prefix}{body}")?;
        if self.skipped > 0 {
            let note = i18n::fill(
                self.lang,
                " ({} skipped, already scanned)",
                &[self.skipped.to_string()],
            );
            write!(f, "{note}")?;
        }
        if self.pass_skipped > 0 {
            let note = i18n::fill(
                self.lang,
                " ({} skipped — their folders have faces off)",
                &[self.pass_skipped.to_string()],
            );
            write!(f, "{note}")?;
        }
        if self.matched.worth_telling() {
            let note = format!("; {}", self.matched);
            write!(f, "{note}")?;
        }
        Ok(())
    }
}

/// A pass over the photos without faces in the DB, then clustering. Videos do not take part.
pub fn run(
    db_path: &Path,
    jobs: usize,
    threshold: f32,
    progress: Option<Arc<Progress>>,
) -> Result<Stats> {
    let t0 = std::time::Instant::now();
    let conn = db::open(db_path)?;
    let settings = db::load_settings(&conn);
    // A switched-off folder takes no part in the pass (M26.1): its files are hidden, and the
    // owner switches it back on to let the pass catch up.
    let rows: Vec<db::FileRow> = db::load_all(&conn)?
        .into_iter()
        .filter(|r| !settings.path_hidden(&r.path))
        .collect();
    // A catalog with faces off is counted apart and left alone (M26.9).
    let pass_skipped = rows
        .iter()
        .filter(|r| r.kind == "image" && !settings.pass_allows(&r.path, db::Pass::Faces))
        .count();
    let rows: Vec<db::FileRow> = rows
        .into_iter()
        .filter(|r| settings.pass_allows(&r.path, db::Pass::Faces))
        .collect();
    let scans = db::face_scan_shas(&conn)?;
    let images = rows.iter().filter(|r| r.kind == "image").count();
    let todo: Vec<db::FileRow> = rows
        .into_iter()
        .filter(|r| r.kind == "image" && scans.get(&r.id) != Some(&r.sha256))
        .collect();
    let skipped = images - todo.len();

    if progress.as_ref().is_some_and(|p| p.cancelled()) {
        if let Some(p) = &progress {
            p.finish();
        }
        return Ok(Stats {
            images,
            done: 0,
            skipped,
            pass_skipped,
            faces: 0,
            clusters: 0,
            matched: MatchReport {
                lang: settings.lang,
                ..MatchReport::default()
            },
            errors: 0,
            seconds: t0.elapsed().as_secs_f64(),
            cancelled: true,
            lang: settings.lang,
        });
    }
    if todo.is_empty() {
        let (faces, _, clusters) = db::face_summary(&conn)?;
        return Ok(Stats {
            images,
            done: 0,
            skipped,
            pass_skipped,
            faces: faces as usize,
            clusters: clusters as usize,
            matched: MatchReport {
                lang: settings.lang,
                ..MatchReport::default()
            },
            errors: 0,
            seconds: t0.elapsed().as_secs_f64(),
            cancelled: false,
            lang: settings.lang,
        });
    }

    let model = Arc::new(Mutex::new(FaceModel::load(&settings, jobs)?));
    let faces_dir = db::cache_dir().join("faces");
    std::fs::create_dir_all(&faces_dir)?;
    if let Some(p) = &progress {
        p.set_total(todo.len());
        p.set_msg(i18n::fill(
            settings.lang,
            "faces: model {}, threads {}",
            &[MODEL_ID.to_string(), jobs.to_string()],
        ));
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs.max(1))
        .build()?;
    let (tx, rx) = mpsc::channel::<(i64, String, Vec<FaceRow>)>();
    let writer = {
        let db_path = db_path.to_path_buf();
        std::thread::spawn(move || -> Result<usize> {
            let mut conn = db::open(&db_path)?;
            let tx = conn.transaction()?;
            let mut n = 0usize;
            for (file_id, sha, faces) in rx {
                db::save_faces_for_file(&tx, file_id, &sha, &faces)?;
                n += 1;
            }
            tx.commit()?;
            Ok(n)
        })
    };
    let errors = AtomicUsize::new(0);
    pool.install(|| {
        todo.par_iter().for_each_with(tx, |tx, f| {
            if progress.as_ref().is_some_and(|p| p.cancelled()) {
                return;
            }
            match scan_one(&model, &faces_dir, f) {
                Ok(faces) => {
                    let _ = tx.send((f.id, f.sha256.clone(), faces));
                }
                Err(e) => {
                    errors.fetch_add(1, Ordering::Relaxed);
                    eprintln!("\n{}", i18n::fill(settings.lang, "faces {}: {}", &[f.path.clone(), format!("{e:#}")]));
                    if let Some(p) = &progress {
                        p.error();
                    }
                    // A broken or unreadable photo is a result too: we do not come back to it every time.
                    // The counter shows the error, and its face count is zero.
                    let _ = tx.send((f.id, f.sha256.clone(), Vec::new()));
                }
            }
            if let Some(p) = &progress {
                p.tick();
            }
        })
    });
    let done = writer
        .join()
        .map_err(|_| anyhow::anyhow!(i18n::t(settings.lang, "the face write thread died")))??;
    let cancelled = progress.as_ref().is_some_and(|p| p.cancelled());
    if let Some(p) = &progress {
        p.set_msg(i18n::t(
            settings.lang,
            if cancelled {
                "faces: stopped"
            } else {
                "faces: clusters"
            },
        ));
    }
    let (clusters, matched) = if cancelled {
        (
            db::face_summary(&conn).map(|(_, _, c)| c as usize).unwrap_or(0),
            MatchReport {
                lang: settings.lang,
                ..MatchReport::default()
            },
        )
    } else {
        // The rebuild ends with the matching step (M26.11); its numbers are part of the report.
        let cs = recluster(db_path, threshold)?;
        (cs.clusters, cs.matched)
    };
    let faces: i64 = db::open(db_path)?.query_row("SELECT COUNT(*) FROM faces", [], |r| r.get(0))?;
    if let Some(p) = &progress {
        p.set_msg(i18n::t(settings.lang, "done"));
        p.finish();
    }
    Ok(Stats {
        images,
        done,
        skipped,
        pass_skipped,
        faces: faces as usize,
        clusters,
        matched,
        errors: errors.load(Ordering::Relaxed),
        seconds: t0.elapsed().as_secs_f64(),
        cancelled,
        lang: settings.lang,
    })
}

fn scan_one(
    model: &Mutex<FaceModel>,
    faces_dir: &Path,
    f: &db::FileRow,
) -> Result<Vec<FaceRow>> {
    let img = derive::load_oriented(Path::new(&f.path))?;
    let detected = {
        let mut m = model
            .lock()
            .map_err(|_| anyhow::anyhow!("the face model is busy and does not answer"))?;
        m.analyze(&img)?
    };
    let mut rows = Vec::with_capacity(detected.len());
    let sha = &f.sha256[..24.min(f.sha256.len())];
    for (i, d) in detected.iter().enumerate() {
        let rel = format!("{}/{sha}-{i}.jpg", &f.sha256[..2.min(f.sha256.len())]);
        let path = faces_dir.join(&rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        d.aligned
            .save_with_format(&path, image::ImageFormat::Jpeg)
            .with_context(|| format!("writing the face {}", path.display()))?;
        rows.push(FaceRow {
            id: 0,
            x: d.x,
            y: d.y,
            w: d.w,
            h: d.h,
            score: d.score,
            quality: d.quality,
            thumb: Some(rel),
            embedding: d.embedding.clone(),
            cluster_id: None,
            manual: false,
        });
    }
    Ok(rows)
}

pub struct ClusterStats {
    pub faces: usize,
    pub clusters: usize,
    pub fixed: usize,
    /// Faces the rebuild left alone because their catalog has faces off (M26.9).
    pub pass_skipped: usize,
    /// What the matching step did after the rebuild (M26.11).
    pub matched: MatchReport,
    pub seconds: f64,
    /// The language of the report (M21.6): the caller sets it before printing; English by default.
    pub lang: Lang,
}

impl std::fmt::Display for ClusterStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = i18n::fill(
            self.lang,
            "{} faces, {} groups, {} pinned by hand, {} s",
            &[
                self.faces.to_string(),
                self.clusters.to_string(),
                self.fixed.to_string(),
                format!("{:.1}", self.seconds),
            ],
        );
        write!(f, "{text}")?;
        if self.pass_skipped > 0 {
            let note = i18n::fill(
                self.lang,
                " ({} faces of catalogs with faces off kept as they are)",
                &[self.pass_skipped.to_string()],
            );
            write!(f, "{note}")?;
        }
        if self.matched.worth_telling() {
            let note = format!("; {}", self.matched);
            write!(f, "{note}")?;
        }
        Ok(())
    }
}

/// Rebuild the groups from the embeddings. Named clusters and manual edits are kept.
pub fn recluster(db_path: &Path, threshold: f32) -> Result<ClusterStats> {
    let t0 = std::time::Instant::now();
    let mut conn = db::open(db_path)?;
    let settings = db::load_settings(&conn);
    let lang = settings.lang;
    let faces = db::load_faces(&conn)?;
    if faces.is_empty() {
        return Ok(ClusterStats {
            faces: 0,
            clusters: 0,
            fixed: 0,
            pass_skipped: 0,
            matched: MatchReport {
                lang,
                ..MatchReport::default()
            },
            seconds: 0.0,
            lang,
        });
    }
    let clusters = db::load_face_clusters(&conn)?;
    let mut fixed: HashSet<i64> = HashSet::new();
    for c in &clusters {
        if !c.label.trim().is_empty() {
            fixed.insert(c.id);
        }
    }
    for f in &faces {
        if f.manual && let Some(cid) = f.cluster_id {
            fixed.insert(cid);
        }
    }
    // A catalog with faces off keeps the groups it has: its faces are not re-clustered (M26.9).
    let paths = db::face_paths(&conn)?;
    let allowed: HashSet<i64> = paths
        .iter()
        .filter(|(_, path)| settings.pass_allows(path, db::Pass::Faces))
        .map(|(id, _)| *id)
        .collect();
    let pass_skipped = faces.iter().filter(|f| !allowed.contains(&f.id)).count();
    let free: Vec<&FaceRow> = faces
        .iter()
        .filter(|f| allowed.contains(&f.id))
        .filter(|f| f.cluster_id.is_none_or(|c| !fixed.contains(&c)))
        .collect();
    if !free.is_empty() {
        let vectors: Vec<&[f32]> = free.iter().map(|f| f.embedding.as_slice()).collect();
        let comps = components(&vectors, threshold);
        let mut groups: HashMap<usize, Vec<i64>> = HashMap::new();
        for (i, root) in comps.into_iter().enumerate() {
            groups.entry(root).or_default().push(free[i].id);
        }
        let mut assignments = Vec::with_capacity(free.len());
        for members in groups.values() {
            let cid = db::create_face_cluster(&conn)?;
            for &fid in members {
                assignments.push((fid, Some(cid)));
            }
        }
        db::save_face_assignments(&mut conn, &assignments, false)?;
        db::delete_empty_face_clusters(&conn)?;
    }
    let total = db::load_face_clusters(&conn)?.len();
    // The named people are matched right after the rebuild (M26.11): a face computed in another
    // catalog joins the person it resembles, so the owner does not name the same grandmother once
    // per folder. The connection is closed first — the step writes through its own.
    drop(conn);
    let matched = match_named(db_path, &settings)?;
    Ok(ClusterStats {
        faces: faces.len(),
        clusters: total,
        fixed: fixed.len(),
        pass_skipped,
        matched,
        seconds: t0.elapsed().as_secs_f64(),
        lang,
    })
}

/// The components of the "cosine ≥ threshold" graph. Returns the root of every vector.
fn components(vectors: &[&[f32]], threshold: f32) -> Vec<usize> {
    let n = vectors.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for i in 0..n {
        for j in i + 1..n {
            if cosine(vectors[i], vectors[j]) >= threshold {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }
    (0..n).map(|i| find(&mut parent, i)).collect()
}

pub fn label(db_path: &Path, cluster_id: i64, label: &str) -> Result<MatchReport> {
    let conn = db::open(db_path)?;
    db::label_face_cluster(&conn, cluster_id, label)?;
    drop(conn);
    // The name is the last decision (M26.11): the matching step runs at once, so the owner does
    // not have to rebuild the groups to see the person recognized in another catalog.
    match_named(db_path, &{
        let conn = db::open(db_path)?;
        db::load_settings(&conn)
    })
}

/// The result of one matching step (M26.11, proposals since M26.17): how many free faces were
/// proposed for a named person, how many people received one, and why the rest stayed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MatchReport {
    pub matched: usize,
    /// The named groups that received at least one face.
    pub people: usize,
    /// The named people that were there before the step.
    pub named: usize,
    /// Faces left because the best named person did not lead the second one by the margin.
    pub left_small: usize,
    /// Faces left because no named person was close enough.
    pub left_far: usize,
    pub seconds: f64,
    /// A plan (`--dry-run`): the same numbers, nothing written.
    pub dry_run: bool,
    /// The language of the report (M21.6): English by default.
    pub lang: Lang,
}

impl MatchReport {
    /// Whether there is something to tell: a report about a database without a single named person
    /// is noise in a pass report, so the pass leaves it out.
    pub fn worth_telling(&self) -> bool {
        self.named > 0
    }
}

impl std::fmt::Display for MatchReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.named == 0 {
            return f.write_str(i18n::t(
                self.lang,
                "no named people yet: there is nothing to match to",
            ));
        }
        // "proposed 0 faces to 0 named people" would read as if nobody were named at all, so a
        // pass that proposed nothing says how many people were there to match against.
        let head = if self.matched == 0 {
            i18n::fill(
                self.lang,
                "no new proposal yet: {} named",
                &[self.named.to_string()],
            )
        } else if self.dry_run {
            i18n::fill(
                self.lang,
                "would propose {} faces to {} named people",
                &[self.matched.to_string(), self.people.to_string()],
            )
        } else {
            i18n::fill(
                self.lang,
                "proposed {} faces to {} named people — they wait in quarantine",
                &[self.matched.to_string(), self.people.to_string()],
            )
        };
        write!(f, "{head}")?;
        if self.left_small > 0 {
            write!(
                f,
                "; {}",
                i18n::fill(
                    self.lang,
                    "{} left — the lead was too small",
                    &[self.left_small.to_string()],
                )
            )?;
        }
        if self.left_far > 0 {
            write!(
                f,
                "; {}",
                i18n::fill(
                    self.lang,
                    "{} left — no named person is close enough",
                    &[self.left_far.to_string()],
                )
            )?;
        }
        Ok(())
    }
}

/// A free face that resembles a named person is proposed for that person (M26.11, quarantine
/// since M26.17): the owner names a grandmother once and every catalog offers her faces for
/// confirmation. Nothing joins a group by itself. Only free faces are proposed — a face with
/// `manual = 1` (named, gathered or split off by hand) and a face that already sits in a named
/// group never are. The best named group wins only when it is at least `settings.face_match` close
/// **and** leads the second named group by `settings.face_match_margin`; a relative's face is left
/// alone rather than guessed. The step replaces the proposal table and touches no membership, no
/// crop and nothing on disk.
pub fn match_named(db_path: &Path, settings: &db::Settings) -> Result<MatchReport> {
    match_named_in(db_path, settings, false)
}

/// The plan of the matching step: the same numbers, not a single row written (`faces --dry-run`).
pub fn plan_named(db_path: &Path, settings: &db::Settings) -> Result<MatchReport> {
    match_named_in(db_path, settings, true)
}

fn match_named_in(
    db_path: &Path,
    settings: &db::Settings,
    dry_run: bool,
) -> Result<MatchReport> {
    let t0 = std::time::Instant::now();
    let mut conn = db::open(db_path)?;
    let faces = db::load_faces(&conn)?;
    let clusters = db::load_face_clusters(&conn)?;
    // The named groups: their members are the reference vectors of every person.
    let named: HashSet<i64> = clusters
        .iter()
        .filter(|c| !c.label.trim().is_empty())
        .map(|c| c.id)
        .collect();
    let mut report = MatchReport {
        named: named.len(),
        dry_run,
        lang: settings.lang,
        ..MatchReport::default()
    };
    if named.is_empty() {
        // Nothing to match to: a proposal to a person without a name would be noise.
        if !dry_run {
            db::clear_face_matches(&conn)?;
        }
        report.seconds = t0.elapsed().as_secs_f64();
        return Ok(report);
    }
    let paths = db::face_paths(&conn)?;
    // The reference: group id → the vectors of its members. A face of a named group is a
    // reference and never a candidate; every other free face is compared with all of them.
    let mut targets: HashMap<i64, Vec<&[f32]>> = HashMap::new();
    let mut candidates: Vec<(i64, &[f32])> = Vec::new();
    for f in &faces {
        if let Some(cid) = f.cluster_id
            && named.contains(&cid)
        {
            targets.entry(cid).or_default().push(f.embedding.as_slice());
            continue;
        }
        // A face is free when the owner did not place it and it is not in a named group. A
        // catalog with the faces pass off takes no part (M26.9), and neither does a switched-off
        // folder (M26.1) — `pass_allows` says both.
        if f.manual {
            continue;
        }
        let allowed = paths
            .get(&f.id)
            .is_some_and(|p| settings.pass_allows(p, db::Pass::Faces));
        if !allowed {
            continue;
        }
        candidates.push((f.id, f.embedding.as_slice()));
    }
    // 2 500 faces × the named members is one pass of a few hundred milliseconds on one thread, so
    // the comparison runs in parallel; every face decides on its own and the result goes into one
    // transaction afterwards (M26.11).
    let scored: Vec<(i64, i64, f32, f32)> = candidates
        .par_iter()
        .map(|&(id, embedding)| {
            let (best, best_id, second) = best_two_groups(embedding, &targets);
            (id, best_id, best, second)
        })
        .collect();
    let mut hits: Vec<(i64, i64, f32)> = Vec::new();
    for (id, best_id, best, second) in scored {
        if best < settings.face_match {
            report.left_far += 1;
        } else if best - second < settings.face_match_margin {
            report.left_small += 1;
        } else {
            report.matched += 1;
            hits.push((id, best_id, best));
        }
    }
    report.people = hits
        .iter()
        .map(|(_, cid, _)| *cid)
        .collect::<HashSet<_>>()
        .len();
    if !dry_run {
        db::replace_face_matches(&mut conn, &hits)?;
    }
    report.seconds = t0.elapsed().as_secs_f64();
    Ok(report)
}

/// The closest named group, its similarity and the best similarity of any other named group.
fn best_two_groups(embedding: &[f32], targets: &HashMap<i64, Vec<&[f32]>>) -> (f32, i64, f32) {
    let mut best = f32::MIN;
    let mut best_id = 0i64;
    let mut second = f32::MIN;
    for (&cid, members) in targets {
        let mut score = f32::MIN;
        for m in members {
            let c = cosine(embedding, m);
            if c > score {
                score = c;
            }
        }
        if score > best {
            second = best;
            best = score;
            best_id = cid;
        } else if score > second {
            second = score;
        }
    }
    (
        best,
        best_id,
        if second == f32::MIN { f32::MIN } else { second },
    )
}

/// The owner confirmed proposals (M26.17): the faces join the named people they resemble and are
/// pinned by hand. Rows only — no face crop and no photo is touched.
pub fn confirm_matches(db_path: &Path, face_ids: &[i64]) -> Result<usize> {
    let mut conn = db::open(db_path)?;
    db::confirm_face_matches(&mut conn, face_ids)
}

/// The owner turned proposals down (M26.17): the faces stay where they are and are pinned, so the
/// next step does not offer them again. Rows only.
pub fn reject_matches(db_path: &Path, face_ids: &[i64]) -> Result<usize> {
    let mut conn = db::open(db_path)?;
    db::reject_face_matches(&mut conn, face_ids)
}

/// The status of an answer to the quarantine (M26.17), with the right number: one face joined or
/// stayed, several joined or stayed. The app and the CLI say the same words.
pub fn answer_report(lang: Lang, moved: usize, confirm: bool) -> String {
    let (one, many) = if confirm {
        (
            "1 face joined the person it resembles",
            "{} faces joined the person they resemble",
        )
    } else {
        (
            "1 face stays where it is — the person is not them",
            "{} faces stay where they are — the person is not them",
        )
    };
    if moved == 1 {
        i18n::t(lang, one).to_string()
    } else {
        i18n::fill(lang, many, &[moved.to_string()])
    }
}

/// A batch assignment: the picked faces go into one group with one name.
pub fn group_picked(db_path: &Path, face_ids: &[i64], label: &str) -> Result<i64> {
    let conn = db::open(db_path)?;
    db::group_faces(&conn, face_ids, label)
}

/// Split a false merge: one face moves into a new group.
pub fn split(db_path: &Path, face_id: i64) -> Result<i64> {
    let mut conn = db::open(db_path)?;
    let cid = db::split_face(&mut conn, face_id)?;
    db::delete_empty_face_clusters(&conn)?;
    Ok(cid)
}

/// Take several picked faces out of their group in one action (M26.10): each gets an unnamed
/// group of its own, the rest stay, and the empty groups are cleared once. The caller keeps the
/// open group: this function never jumps the screen anywhere.
pub fn split_many(db_path: &Path, face_ids: &[i64]) -> Result<db::SplitOutcome> {
    let mut conn = db::open(db_path)?;
    let out = db::split_faces(&mut conn, face_ids)?;
    db::delete_empty_face_clusters(&conn)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2(a: f32, b: f32) -> Vec<f32> {
        let mut v = vec![a, b, 0.0, 0.0];
        normalize(&mut v);
        v
    }

    #[test]
    fn components_join_only_similar_faces() {
        let a = v2(1.0, 0.0);
        let a2 = v2(0.99, 0.05);
        let b = v2(0.0, 1.0);
        let vectors: Vec<&[f32]> = vec![&a, &a2, &b];
        let comps = components(&vectors, 0.8);
        assert_eq!(comps[0], comps[1]);
        assert_ne!(comps[0], comps[2]);
        // A high threshold splits even similar ones.
        let comps = components(&vectors, 0.9999);
        assert_ne!(comps[0], comps[1]);
    }

    /// A unit vector at an angle in the plane: two people and their relatives can be placed by
    /// their cosine without an ONNX model.
    fn unit(deg: f32) -> Vec<f32> {
        let a = deg.to_radians();
        let mut v = vec![a.cos(), a.sin(), 0.0, 0.0];
        normalize(&mut v);
        v
    }

    fn temp_db(tag: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "photocat-{tag}-{}-{stamp}.db",
            std::process::id()
        ))
    }

    fn file_row(path: &str, sha: &str) -> db::FileRow {
        db::FileRow {
            id: 0,
            path: path.into(),
            size: 1,
            mtime: 0,
            sha256: sha.into(),
            kind: "image".into(),
            width: 320,
            height: 240,
            taken_at: None,
            taken_src: String::new(),
            lat: None,
            lon: None,
            camera: None,
            dhash: None,
            sharpness: None,
            duration_s: None,
            thumb: None,
            decision: None,
            trashed_at: None,
        }
    }

    /// M26.11: a face named once is found in every catalog. Two named people (A at 0°, B at 60°)
    /// and the free faces around them: the close one joins A from either catalog; the one at 30°
    /// is as close to B as to A and stays where it is; the one at 150° is far from both; a face the
    /// owner placed by hand is never moved; a catalog with the faces pass off is not considered.
    #[test]
    fn a_named_person_is_recognized_in_every_catalog() {
        let path = temp_db("match-named");
        let mut settings = db::Settings {
            roots: vec![
                "/archive/a".into(),
                "/archive/b".into(),
                "/off/cat".into(),
            ],
            ..db::Settings::default()
        };
        settings
            .root_passes
            .insert("/off/cat".to_string(), vec![db::Pass::Faces]);
        let (ann, close_a, close_b, tie, far, by_hand, off) = {
            let conn = db::open(&path).unwrap();
            for (p, sha) in [
                ("/archive/a/one.jpg", "a1"),
                ("/archive/a/two.jpg", "a2"),
                ("/archive/b/one.jpg", "b1"),
                ("/archive/b/two.jpg", "b2"),
                ("/archive/b/three.jpg", "b3"),
                ("/archive/b/four.jpg", "b4"),
                ("/archive/b/five.jpg", "b5"),
                ("/off/cat/one.jpg", "c1"),
            ] {
                db::insert(&conn, &file_row(p, sha)).unwrap();
            }
            let id_of = |p: &str| -> i64 {
                conn.query_row("SELECT id FROM files WHERE path=?1", [p], |r| r.get(0))
                    .unwrap()
            };
            // One face per photo: after the write, the last row of `faces` is the one just added.
            let add = |photo: &str, v: Vec<f32>| -> i64 {
                let file_id = id_of(photo);
                db::save_faces_for_file(
                    &conn,
                    file_id,
                    "sha",
                    &[FaceRow {
                        id: 0,
                        x: 0,
                        y: 0,
                        w: 50,
                        h: 50,
                        score: 0.9,
                        quality: 40.0,
                        thumb: None,
                        embedding: v,
                        cluster_id: None,
                        manual: false,
                    }],
                )
                .unwrap();
                conn.query_row("SELECT MAX(id) FROM faces", [], |r| r.get(0))
                    .unwrap()
            };
            let a_member = add("/archive/a/one.jpg", unit(2.0));
            let b_member = add("/archive/b/one.jpg", unit(60.0));
            let ann = db::create_face_cluster(&conn).unwrap();
            db::label_face_cluster(&conn, ann, "Ann").unwrap();
            let bob = db::create_face_cluster(&conn).unwrap();
            db::label_face_cluster(&conn, bob, "Bob").unwrap();
            db::save_face_assignments(
                &mut db::open(&path).unwrap(),
                &[(a_member, Some(ann)), (b_member, Some(bob))],
                true,
            )
            .unwrap();
            let close_a = add("/archive/a/two.jpg", unit(20.0));
            let close_b = add("/archive/b/two.jpg", unit(20.0));
            let tie = add("/archive/b/three.jpg", unit(30.0));
            let far = add("/archive/b/four.jpg", unit(150.0));
            let by_hand = add("/archive/b/five.jpg", unit(0.0));
            let off = add("/off/cat/one.jpg", unit(0.0));
            db::save_face_assignments(&mut db::open(&path).unwrap(), &[(by_hand, None)], true)
                .unwrap();
            (ann, close_a, close_b, tie, far, by_hand, off)
        };

        // The plan writes nothing: the close faces are still where they were.
        let plan = plan_named(&path, &settings).unwrap();
        assert!(plan.dry_run);
        assert_eq!((plan.named, plan.matched, plan.people), (2, 2, 1), "{plan}");
        assert_eq!((plan.left_small, plan.left_far), (1, 1), "{plan}");
        let before = db::load_faces(&db::open(&path).unwrap()).unwrap();
        let cluster_of = |faces: &[FaceRow], id: i64| {
            faces.iter().find(|f| f.id == id).unwrap().cluster_id
        };
        assert_eq!(cluster_of(&before, close_a), None, "the plan wrote a row");

        // The step itself (M26.17): both close faces are proposed for Ann, from two different
        // catalogs, and neither moves. They wait in the quarantine.
        let report = match_named(&path, &settings).unwrap();
        assert!(!report.dry_run);
        assert_eq!((report.named, report.matched, report.people), (2, 2, 1));
        assert_eq!((report.left_small, report.left_far), (1, 1), "{report}");
        assert!(
            report
                .to_string()
                .contains("proposed 2 faces to 1 named people — they wait in quarantine"),
            "{report}"
        );
        assert!(
            report.to_string().contains("1 left — the lead was too small"),
            "{report}"
        );
        let after = db::load_faces(&db::open(&path).unwrap()).unwrap();
        for id in [close_a, close_b] {
            assert_eq!(
                cluster_of(&after, id),
                None,
                "a proposal moved the face into Ann"
            );
        }
        let proposals = db::load_face_proposals(&db::open(&path).unwrap()).unwrap();
        assert_eq!(proposals.len(), 2, "the proposals are not stored");
        for p in &proposals {
            assert_eq!(p.target_id, ann);
            assert_eq!(p.target_label, "Ann");
            assert!(p.similarity > 0.9);
        }
        // The owner's own placement, the tie, the far face and the forbidden catalog stay.
        for id in [by_hand, far, tie, off] {
            assert_eq!(cluster_of(&after, id), None, "a face moved that should not");
        }
        assert!(
            after.iter().find(|f| f.id == by_hand).unwrap().manual,
            "the hand-placed face lost its pin"
        );
        // A confirmed proposal joins Ann and is pinned; a turned-down one stays where it is and
        // is pinned, so a second step does not offer it again.
        assert_eq!(confirm_matches(&path, &[close_a]).unwrap(), 1);
        assert_eq!(reject_matches(&path, &[close_b]).unwrap(), 1);
        let answered = db::load_faces(&db::open(&path).unwrap()).unwrap();
        assert_eq!(cluster_of(&answered, close_a), Some(ann));
        assert!(answered.iter().find(|f| f.id == close_a).unwrap().manual);
        assert_eq!(cluster_of(&answered, close_b), None);
        assert!(answered.iter().find(|f| f.id == close_b).unwrap().manual);
        // The confirmed face is a member of Ann now, so the 30° face is no longer ambiguous and
        // is offered for her; the turned-down and hand-placed faces are not offered again.
        let again = match_named(&path, &settings).unwrap();
        assert_eq!(again.matched, 1, "an answered face was offered again: {again}");
        assert_eq!((again.left_small, again.left_far), (0, 1), "{again}");
        let proposals = db::load_face_proposals(&db::open(&path).unwrap()).unwrap();
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].face.id, tie);
        assert_eq!(proposals[0].target_id, ann);
        assert!(
            proposals.iter().all(|p| p.face.id != close_b),
            "a turned-down face was offered again"
        );
    }

    /// M26.11 on the real archive: how close the free faces come to the people the owner has
    /// named, and what one pass costs. A copy of the DB, never the owner's own.
    /// `PHOTOCAT_BENCH_DB=<copy> cargo test --release -- --ignored \
    ///  a_real_archive_matches_a_named_person_quickly --nocapture`
    #[test]
    #[ignore = "needs the real archive DB and moves faces in it"]
    fn a_real_archive_matches_a_named_person_quickly() {
        let Ok(db_path) = std::env::var("PHOTOCAT_BENCH_DB") else {
            return;
        };
        let path = std::path::PathBuf::from(db_path);
        let settings = db::load_settings(&db::open(&path).unwrap());
        let faces = db::load_faces(&db::open(&path).unwrap()).unwrap();
        let clusters = db::load_face_clusters(&db::open(&path).unwrap()).unwrap();
        let named: Vec<(i64, String)> = clusters
            .iter()
            .filter(|c| !c.label.trim().is_empty())
            .map(|c| (c.id, format!("{} ({})", c.label, c.faces)))
            .collect();
        println!(
            "{} faces, {} named people: {}",
            faces.len(),
            named.len(),
            named
                .iter()
                .map(|(_, l)| l.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!(
            "face_match {:.2}, margin {:.2}",
            settings.face_match, settings.face_match_margin
        );
        // The distance of every free face to the best named person: the tail decides the default.
        let named_ids: HashSet<i64> = named.iter().map(|(id, _)| *id).collect();
        let mut targets: HashMap<i64, Vec<&[f32]>> = HashMap::new();
        for f in &faces {
            if let Some(cid) = f.cluster_id
                && named_ids.contains(&cid)
            {
                targets.entry(cid).or_default().push(f.embedding.as_slice());
            }
        }
        let paths = db::face_paths(&db::open(&path).unwrap()).unwrap();
        let mut best: Vec<f32> = faces
            .iter()
            .filter(|f| !f.manual && f.cluster_id.is_none_or(|c| !named_ids.contains(&c)))
            .filter(|f| {
                paths
                    .get(&f.id)
                    .is_some_and(|p| settings.pass_allows(p, db::Pass::Faces))
            })
            .map(|f| best_two_groups(&f.embedding, &targets).0)
            .collect();
        best.sort_by(|a, b| b.total_cmp(a));
        let above = |x: f32| best.iter().filter(|s| **s >= x).count();
        println!(
            "{} free faces; best similarity: max {:.3}, p99 {:.3}, p95 {:.3}; \
             above 0.50 {} / 0.45 {} / 0.40 {} / 0.35 {} / 0.30 {}",
            best.len(),
            best.first().copied().unwrap_or(0.0),
            best.get(best.len() / 100).copied().unwrap_or(0.0),
            best.get(best.len() / 20).copied().unwrap_or(0.0),
            above(0.50),
            above(0.45),
            above(0.40),
            above(0.35),
            above(0.30),
        );
        for run in 1..=2 {
            let t = std::time::Instant::now();
            let r = match_named(&path, &settings).unwrap();
            println!(
                "run {run}: {r} — {:.1} ms of wall clock, {:.1} ms in the step",
                t.elapsed().as_secs_f64() * 1000.0,
                r.seconds * 1000.0
            );
        }
        let proposals = db::load_face_proposals(&db::open(&path).unwrap()).unwrap();
        println!("proposals waiting in quarantine: {}", proposals.len());
    }

    #[test]
    fn nms_keeps_best_of_overlapping_boxes() {
        let mk = |r: [f32; 4], score: f32| RawBox {
            rect: r,
            score,
            kps: [[0.0; 2]; 5],
        };
        let boxes = vec![
            mk([0.0, 0.0, 100.0, 100.0], 0.9),
            mk([5.0, 5.0, 100.0, 100.0], 0.8),
            mk([200.0, 200.0, 300.0, 300.0], 0.7),
        ];
        let kept = nms(boxes, 0.4);
        assert_eq!(kept.len(), 2);
        assert!((kept[0].score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn alignment_is_identity_when_points_match() {
        let (a, b, tx, ty) = similarity(&ARC_DST, &ARC_DST);
        assert!((a - 1.0).abs() < 1e-5, "{a}");
        assert!(b.abs() < 1e-5, "{b}");
        assert!(tx.abs() < 1e-4, "{tx}");
        assert!(ty.abs() < 1e-4, "{ty}");
    }

    #[test]
    fn alignment_moves_points_to_arc_dst() {
        // The points are rotated and shifted: the matrix must bring them back to the canonical ones.
        let (a, b, tx, ty) = similarity(&ARC_DST, &ARC_DST);
        for p in ARC_DST {
            let x = a * p[0] - b * p[1] + tx;
            let y = b * p[0] + a * p[1] + ty;
            assert!((x - p[0]).abs() < 1e-3);
            assert!((y - p[1]).abs() < 1e-3);
        }
    }

    #[test]
    #[ignore = "needs the downloaded CLIP and face weights"]
    fn clip_and_faces_share_one_onnx_environment() {
        let settings = db::Settings::default();
        // ORT allows one environment per process: CLIP must not block the faces.
        let _clip = embed::Model::load(&settings, 1).unwrap();
        let _faces = FaceModel::load(&settings, 1).unwrap();
    }

    #[test]
    fn bilinear_returns_corner_pixel() {
        let mut img = RgbImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgb([10, 20, 30]));
        img.put_pixel(1, 0, image::Rgb([40, 50, 60]));
        assert_eq!(bilinear(&img, 0.0, 0.0, 2, 2).0, [10, 20, 30]);
        assert_eq!(bilinear(&img, -5.0, -5.0, 2, 2).0, [10, 20, 30]);
    }
}
