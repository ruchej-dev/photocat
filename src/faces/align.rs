//! The five-point alignment: the similarity transform and the bilinear crop.

use image::RgbImage;

/// The ArcFace input: an aligned 112×112 frame.
pub const FACE_SIDE: u32 = 112;

/// The canonical ArcFace points: eyes, nose, mouth corners in a 112×112 frame.
pub(super) const ARC_DST: [[f32; 2]; 5] = [
    [38.2946, 51.6963],
    [73.5318, 51.5014],
    [56.0252, 71.7366],
    [41.5493, 92.3655],
    [70.7299, 92.2041],
];

/// The similarity matrix (scale and rotation, no reflection) that maps `src` onto `dst`.
/// Returns `(a, b, tx, ty)`: x' = a·x − b·y + tx, y' = b·x + a·y + ty.
pub(super) fn similarity(src: &[[f32; 2]; 5], dst: &[[f32; 2]; 5]) -> (f32, f32, f32, f32) {
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
pub(super) fn align_face(src: &RgbImage, kps: &[[f32; 2]; 5]) -> RgbImage {
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

pub(super) fn bilinear(src: &RgbImage, x: f32, y: f32, w: u32, h: u32) -> image::Rgb<u8> {
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
