//! SCRFD detection: the 640 px blob, the score map and the boxes after NMS.

use image::{DynamicImage, RgbImage};

/// The detector input: 640×640 keeping the proportions, with black margins.
pub const DET_SIDE: u32 = 640;

pub const DET_THRESHOLD: f32 = 0.5;

pub(super) const NMS_IOU: f32 = 0.4;

/// Very small faces give no usable embedding — they are not counted.
pub(super) const MIN_FACE_PX: f32 = 28.0;

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

/// A letterbox into 640×640 keeping the proportions: black margins right and bottom, as in insightface.
pub(super) fn detect_blob(img: &DynamicImage) -> (Vec<f32>, f32) {
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

pub(super) struct RawBox {
    pub(super) rect: [f32; 4],
    pub(super) score: f32,
    pub(super) kps: [[f32; 2]; 5],
}

/// Parse one SCRFD head: anchor centres, distance2bbox, key points.
#[allow(clippy::too_many_arguments)]
pub(super) fn decode_level(
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

pub(super) fn nms(mut boxes: Vec<RawBox>, threshold: f32) -> Vec<RawBox> {
    boxes.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<RawBox> = Vec::new();
    for b in boxes {
        if kept.iter().all(|k| iou(&k.rect, &b.rect) <= threshold) {
            kept.push(b);
        }
    }
    kept
}
