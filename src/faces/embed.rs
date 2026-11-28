//! The ArcFace session: loading the two ONNX models and turning a face into a vector.

use super::align::{align_face, FACE_SIDE};
use super::detect::{decode_level, detect_blob, nms, RawBox, DET_SIDE, NMS_IOU};
use super::{check_model, model_dir, DetectedFace};
use crate::db;
use crate::embed;
use anyhow::{bail, Context, Result};
use image::{DynamicImage, RgbImage};
use std::collections::HashMap;
use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;

pub const EMBED_DIM: usize = 512;

fn oe<R>(e: ort::Error<R>) -> anyhow::Error {
    anyhow::anyhow!("face model: {e}")
}

/// One model per process: a shared ONNX context and two sessions.
pub struct FaceModel {
    _env: Environment,
    det: Session,
    rec: Session,
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

pub(super) fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 1e-9 {
        for x in v.iter_mut() {
            *x /= n;
        }
    }
}

pub(super) fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
