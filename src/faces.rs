//! Лица: детекция SCRFD, выравнивание по 5 точкам, ArcFace-эмбеддинги и кластеры.
//!
//! Модели лежат в `<models_dir>/faces/`, скачиваются только по согласию и проверяются по sha256.
//! Наружу не уходит ни одно лицо, ни один эмбеддинг и ни один путь (ADR 0011, 0021).

use crate::db::{self, FaceRow, Progress};
use crate::derive;
use crate::embed::{self, ModelFile};
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
/// Вход детектора: 640×640 с сохранением пропорций и чёрными полями.
pub const DET_SIDE: u32 = 640;
/// Вход ArcFace: выровненный кадр 112×112.
pub const FACE_SIDE: u32 = 112;
pub const EMBED_DIM: usize = 512;
pub const DET_THRESHOLD: f32 = 0.5;
const NMS_IOU: f32 = 0.4;
/// Совсем маленькие лица не дают пригодного эмбеддинга — их не считаем.
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

/// Канонические точки ArcFace: глаза, нос, углы рта в кадре 112×112.
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
                "нет файлов модели лиц: {}\nскачайте в «Настройках» (с согласием) или: \
                 ./target/release/photocat faces download --yes",
                path.display()
            );
        }
        let got = derive::sha256_file(&path)
            .with_context(|| format!("не прочитать {}", path.display()))?;
        if got != f.sha256 {
            bail!(
                "файл модели лиц повреждён ({}): {}\nsha256 {got}, ожидался {}",
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
}

impl std::fmt::Display for DownloadReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "модель лиц скачана: файлов {}, {}",
            self.files,
            db::human(FILES.iter().map(|x| x.bytes as i64).sum())
        )
    }
}

/// Загрузка весов лиц — тем же путём, что CLIP: только по согласию, curl, sha256.
pub fn download_model(
    settings: &db::Settings,
    consent: bool,
    progress: Option<Arc<Progress>>,
) -> Result<DownloadReport> {
    if !consent {
        bail!("скачивание весов лиц требует явного согласия (галочка в «Настройках» или --yes)");
    }
    let bytes: u64 = FILES.iter().map(|f| f.bytes).sum();
    if bytes > settings.max_model_mb.saturating_mul(1024 * 1024) {
        bail!(
            "модель лиц {} больше лимита {} МиБ — поднимите лимит в «Настройках»",
            db::human(bytes as i64),
            settings.max_model_mb
        );
    }
    let files = embed::download_files(
        settings,
        &model_dir(settings),
        "лица buffalo_l",
        &FILES,
        progress,
    )?;
    Ok(DownloadReport { files })
}

fn oe<R>(e: ort::Error<R>) -> anyhow::Error {
    anyhow::anyhow!("модель лиц: {e}")
}

/// Одна модель на процесс: общий ONNX-контекст и два сеанса.
pub struct FaceModel {
    _env: Environment,
    det: Session,
    rec: Session,
}

/// Найденное лицо до записи в БД: рамка в пикселях оригинала и выровненный кадр.
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

    /// Лица на кадре: детекция, NMS, выравнивание, эмбеддинг. Кадр уже повёрнут по EXIF.
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

        // SCRFD отдаёт по три головы на шаг: score [N,1], bbox [N,4], kps [N,10].
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
        counts.sort_unstable_by(|a, b| b.cmp(a)); // крупная карта (шаг 8) первой
        let mut boxes: Vec<RawBox> = Vec::new();
        for (level, count) in counts.into_iter().enumerate() {
            let Some([Some(scores), Some(bbox), Some(kps)]) = levels.get(&count).cloned() else {
                continue;
            };
            // SCRFD использует 2 anchor'а на ячейку: N = 2·(640/stride)².
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
            .context("ArcFace не вернула вектор")?;
        let (shape, data) = value.try_extract_tensor::<f32>().map_err(oe)?;
        let dim = *shape.last().unwrap_or(&0) as usize;
        if dim != EMBED_DIM {
            bail!("ArcFace вернула {dim} признаков, ожидалось {EMBED_DIM}");
        }
        let mut v = data.to_vec();
        normalize(&mut v);
        Ok(v)
    }
}

/// Письмо в 640×640 с сохранением пропорций: чёрные поля снизу и справа, как в insightface.
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
            // BGR здесь не нужен: модель обучена на RGB (blobFromImage + swapRB).
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

/// Разобрать одну голову SCRFD: anchor-центры, distance2bbox, ключевые точки.
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

/// Матрица подобия (масштаб + поворот без отражения), переводящая `src` в `dst`.
/// Возвращает `(a, b, tx, ty)`: x' = a·x − b·y + tx, y' = b·x + a·y + ty.
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
    // Комплексное частное: (den + i·num) / Σ|p|².
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

/// Выровнять лицо по 5 точкам в кадр 112×112 билинейной выборкой.
fn align_face(src: &RgbImage, kps: &[[f32; 2]; 5]) -> RgbImage {
    let (a, b, tx, ty) = similarity(kps, &ARC_DST);
    let det = (a * a + b * b).max(1e-6);
    let (sw, sh) = src.dimensions();
    let mut out = RgbImage::new(FACE_SIDE, FACE_SIDE);
    for v in 0..FACE_SIDE {
        for u in 0..FACE_SIDE {
            let dx = u as f32 + 0.5 - tx;
            let dy = v as f32 + 0.5 - ty;
            // Обратная матрица подобия: 1/(a²+b²) · [a  b; −b  a].
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
}

pub fn plan(db_path: &Path) -> Result<Plan> {
    let conn = db::open(db_path)?;
    let (images, todo) = db::face_scan_plan(&conn)?;
    Ok(Plan { images, todo })
}

pub struct Stats {
    pub images: usize,
    pub done: usize,
    pub skipped: usize,
    pub faces: usize,
    pub clusters: usize,
    pub errors: usize,
    pub seconds: f64,
    pub cancelled: bool,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}просканировано {} из {} снимков, лиц {}, кластеров {}, ошибок {}, за {:.1} с",
            if self.cancelled { "остановлено: " } else { "" },
            self.done,
            self.images,
            self.faces,
            self.clusters,
            self.errors,
            self.seconds
        )?;
        if self.skipped > 0 {
            write!(f, " (пропущено {}, уже просканировано)", self.skipped)?;
        }
        Ok(())
    }
}

/// Проход по снимкам без лиц в БД, затем кластеризация. Видео не участвуют.
pub fn run(
    db_path: &Path,
    jobs: usize,
    threshold: f32,
    progress: Option<Arc<Progress>>,
) -> Result<Stats> {
    let t0 = std::time::Instant::now();
    let conn = db::open(db_path)?;
    let settings = db::load_settings(&conn);
    let rows = db::load_all(&conn)?;
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
            faces: 0,
            clusters: 0,
            errors: 0,
            seconds: t0.elapsed().as_secs_f64(),
            cancelled: true,
        });
    }
    if todo.is_empty() {
        let (faces, _, clusters) = db::face_summary(&conn)?;
        return Ok(Stats {
            images,
            done: 0,
            skipped,
            faces: faces as usize,
            clusters: clusters as usize,
            errors: 0,
            seconds: t0.elapsed().as_secs_f64(),
            cancelled: false,
        });
    }

    let model = Arc::new(Mutex::new(FaceModel::load(&settings, jobs)?));
    let faces_dir = db::cache_dir().join("faces");
    std::fs::create_dir_all(&faces_dir)?;
    if let Some(p) = &progress {
        p.set_total(todo.len());
        p.set_msg(format!("лица: моделей {}, потоков {jobs}", MODEL_ID));
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
                    eprintln!("\nлица {}: {e:#}", f.path);
                    if let Some(p) = &progress {
                        p.error();
                    }
                    // Битый/нечитаемый снимок — тоже результат: не возвращаемся к нему каждый раз.
                    // Ошибка видна в счётчике, а лиц у него ноль.
                    let _ = tx.send((f.id, f.sha256.clone(), Vec::new()));
                }
            }
            if let Some(p) = &progress {
                p.tick();
            }
        })
    });
    let done = writer.join().map_err(|_| anyhow::anyhow!("поток записи лиц упал"))??;
    let cancelled = progress.as_ref().is_some_and(|p| p.cancelled());
    if let Some(p) = &progress {
        p.set_msg(if cancelled { "лица: остановлено" } else { "лица: кластеры" });
    }
    let clusters = if cancelled {
        db::face_summary(&conn).map(|(_, _, c)| c as usize).unwrap_or(0)
    } else {
        recluster(db_path, threshold)?.clusters
    };
    let faces: i64 = db::open(db_path)?.query_row("SELECT COUNT(*) FROM faces", [], |r| r.get(0))?;
    if let Some(p) = &progress {
        p.set_msg("готово");
        p.finish();
    }
    Ok(Stats {
        images,
        done,
        skipped,
        faces: faces as usize,
        clusters,
        errors: errors.load(Ordering::Relaxed),
        seconds: t0.elapsed().as_secs_f64(),
        cancelled,
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
            .map_err(|_| anyhow::anyhow!("модель лиц занята и не отвечает"))?;
        m.analyze(&img)?
    };
    let mut rows = Vec::with_capacity(detected.len());
    let sha = &f.sha256[..24.min(f.sha256.len())];
    for (i, d) in detected.iter().enumerate() {
        let rel = format!("{}/{sha}-{i}.jpg", &f.sha256[..2.min(f.sha256.len())]);
        let path = faces_dir.join(&rel);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("не создать {}", dir.display()))?;
        }
        d.aligned
            .save_with_format(&path, image::ImageFormat::Jpeg)
            .with_context(|| format!("запись лица {}", path.display()))?;
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
    pub seconds: f64,
}

impl std::fmt::Display for ClusterStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "лиц {}, групп {}, из них закреплено вручную {}, за {:.1} с",
            self.faces, self.clusters, self.fixed, self.seconds
        )
    }
}

/// Пересобрать группы по эмбеддингам. Кластеры с подписью и ручные правки не пересобираются.
pub fn recluster(db_path: &Path, threshold: f32) -> Result<ClusterStats> {
    let t0 = std::time::Instant::now();
    let mut conn = db::open(db_path)?;
    let faces = db::load_faces(&conn)?;
    if faces.is_empty() {
        return Ok(ClusterStats { faces: 0, clusters: 0, fixed: 0, seconds: 0.0 });
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
    let free: Vec<&FaceRow> = faces
        .iter()
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
    Ok(ClusterStats {
        faces: faces.len(),
        clusters: total,
        fixed: fixed.len(),
        seconds: t0.elapsed().as_secs_f64(),
    })
}

/// Компоненты графа «косинус ≥ порога». Возвращает корень для каждого вектора.
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

pub fn label(db_path: &Path, cluster_id: i64, label: &str) -> Result<()> {
    let conn = db::open(db_path)?;
    db::label_face_cluster(&conn, cluster_id, label)
}

/// Разъединить ложную склейку: одно лицо уезжает в новую группу.
pub fn split(db_path: &Path, face_id: i64) -> Result<i64> {
    let mut conn = db::open(db_path)?;
    let cid = db::split_face(&mut conn, face_id)?;
    db::delete_empty_face_clusters(&conn)?;
    Ok(cid)
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
        // Высокий порог разъединяет даже похожие.
        let comps = components(&vectors, 0.9999);
        assert_ne!(comps[0], comps[1]);
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
        // Точки повёрнуты и сдвинуты: матрица должна вернуть их к каноническим.
        let (a, b, tx, ty) = similarity(&ARC_DST, &ARC_DST);
        for p in ARC_DST {
            let x = a * p[0] - b * p[1] + tx;
            let y = b * p[0] + a * p[1] + ty;
            assert!((x - p[0]).abs() < 1e-3);
            assert!((y - p[1]).abs() < 1e-3);
        }
    }

    #[test]
    #[ignore = "нужны скачанные веса CLIP и лиц"]
    fn clip_and_faces_share_one_onnx_environment() {
        let settings = db::Settings::default();
        // ORT разрешает одно окружение на процесс: CLIP не должен блокировать лица.
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
