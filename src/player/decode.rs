//! The video decoder and the frame queue: packets in, raw frames out, RGBA on demand.

use super::demux::{new_source, next_for, Kind, Media, Source};
use super::present::{Control, Shared};
use super::{VideoFrame, TICK};
use anyhow::Result;
use rff_codec::{CodecParams, CodecRegistry, Decoder};
use rff_core::{ColorRange, Frame, PixelFormat, VideoFrame as RawFrame};
use rff_format::Stream;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The queue of decoded frames between the decode thread and the presentation thread.
pub(super) struct FrameQueue {
    q: Mutex<VecDeque<Queued>>,
    cap: usize,
    /// The decoder reached the end of the file.
    pub(super) closed: AtomicBool,
}

pub(super) struct Queued {
    pub(super) pts_s: f64,
    pub(super) frame: RawFrame,
}

impl FrameQueue {
    pub(super) fn new(cap: usize) -> FrameQueue {
        FrameQueue {
            q: Mutex::new(VecDeque::with_capacity(cap)),
            cap,
            closed: AtomicBool::new(false),
        }
    }

    fn len(&self) -> usize {
        self.q.lock().map(|q| q.len()).unwrap_or(0)
    }

    fn push(&self, pts_s: f64, frame: RawFrame) {
        if let Ok(mut q) = self.q.lock() {
            q.push_back(Queued { pts_s, frame });
        }
    }

    /// Take a frame off the queue. The lock is held for microseconds: the RGBA
    /// conversion happens without it.
    pub(super) fn pop(&self) -> Option<Queued> {
        self.q.lock().ok().and_then(|mut q| q.pop_front())
    }

    fn clear(&self) {
        if let Ok(mut q) = self.q.lock() {
            q.clear();
        }
    }
}

pub(super) fn codec_registry() -> CodecRegistry {
    let mut reg = CodecRegistry::new();
    rff_codec_h264::register(&mut reg);
    rff_codec_hevc::register(&mut reg);
    rff_codec_aac::register(&mut reg);
    // MJPEG in a `.MOV` is the Panasonic Lumix camera default (the owner's archive): every
    // packet is a JPEG. PCM is the sound of the same files.
    rff_codec_jpeg::register(&mut reg);
    rff_codec_pcm::register(&mut reg);
    reg
}

/// A decoder for a stream. A codec out of scope (VP9, Opus, …) is an honest error:
/// the caller will offer an external player.
pub(super) fn make_decoder(stream: &Stream) -> Result<Box<dyn Decoder>> {
    let reg = codec_registry();
    let mut dec = reg
        .find_decoder(stream.codec_id)
        .map_err(|e| anyhow::anyhow!("no decoder for {:?}: {e}", stream.codec_id))?;
    dec.configure(&CodecParams {
        codec_id: stream.codec_id,
        width: stream.width,
        height: stream.height,
        pixel_format: stream.pixel_format,
        sample_rate: stream.sample_rate,
        channels: stream.channels,
        sample_format: stream.sample_format,
        extradata: stream.extradata.clone(),
    })?;
    Ok(dec)
}

/// Decode video and put the frames into the queue. It sleeps only when the queue is
/// full — that is, when the presentation cannot keep up.
pub(super) fn video_loop(
    mut media: Media,
    frames: Arc<FrameQueue>,
    control: Arc<Control>,
    shared: Arc<Shared>,
    mut decoder: Option<Box<dyn Decoder>>,
) {
    let Some(stream) = media.video.clone() else {
        frames.closed.store(true, Ordering::Relaxed);
        return;
    };
    let mut source = new_source(&mut media);
    // From zero, not from the current value: a seek requested before the thread starts
    // would be lost otherwise.
    let mut epoch = 0u64;
    let mut video_epoch = 0u64;
    let mut done = false;

    while !control.stop.load(Ordering::Relaxed) {
        let e = control.seek_epoch.load(Ordering::Relaxed);
        let ve = control.video_epoch.load(Ordering::Relaxed);
        let catch_up = ve != video_epoch;
        if catch_up {
            video_epoch = ve;
        }
        if e != epoch || catch_up {
            if e != epoch {
                epoch = e;
            }
            frames.clear();
            done = false;
            let target = if catch_up {
                control.video_ms.load(Ordering::Relaxed) as f64 / 1000.0
            } else {
                control.seek_ms.load(Ordering::Relaxed) as f64 / 1000.0
            };
            decoder = match media.video.as_ref().map(make_decoder) {
                Some(Ok(d)) => Some(d),
                other => {
                    if let Some(Err(e)) = other {
                        shared.fail(format!("{e:#}"));
                    }
                    None
                }
            };
            if let Some(idx) = media.demuxer.sample_index() {
                let tb = stream.time_base.as_f64();
                let pts = (target / tb) as i64;
                let start = idx.sync_at_or_before(stream.index, pts).unwrap_or(0);
                source = Source::Index {
                    v: start,
                    a: match &source {
                        Source::Index { a, .. } => *a,
                        Source::Stream => 0,
                    },
                };
            } else {
                source = Source::Stream;
            }
            continue;
        }
        if done || frames.len() >= frames.cap {
            std::thread::sleep(TICK);
            continue;
        }
        match next_for(&mut media, &mut source, Kind::Video) {
            Some(pkt) => {
                let Some(dec) = decoder.as_mut() else {
                    done = true;
                    continue;
                };
                if dec.send_packet(&pkt).is_err() {
                    continue;
                }
                let tb = stream.time_base.as_f64();
                while let Ok(Frame::Video(mut f)) = dec.receive_frame() {
                    // A still-picture decoder (MJPEG) carries no timestamp of its own: the
                    // packet's PTS is the frame's, or every frame would land at zero.
                    if f.pts.is_none() {
                        f.pts = pkt.pts;
                    }
                    let pts = f.pts.map(|p| p as f64 * tb).unwrap_or(0.0);
                    frames.push(pts, f);
                }
            }
            None => done = true,
        }
    }
    frames.closed.store(true, Ordering::Relaxed);
}

/// YUV → RGBA honouring the matrix, the range and the rotation from the container.
///
/// Rows are computed in parallel (rayon): 2520×1080 is 2.7 million pixels, one thread
/// cannot do them within a frame. Integer arithmetic in 8.8, no floats.
/// Formats beyond 4:2:0 8/10 bit are not promised: the caller will yield to an external
/// player (ADR 0030).
pub(super) fn to_rgba(f: &RawFrame, s: &Stream, max_width: usize) -> Option<VideoFrame> {
    use rayon::prelude::*;

    let (w, h) = (f.width as usize, f.height as usize);
    if w == 0 || h == 0 || f.planes.len() < 3 || f.strides.len() < 3 {
        return None;
    }
    let depth = match f.format {
        PixelFormat::Yuv420p => 8u32,
        PixelFormat::Yuv420p10 => 10,
        _ => return None,
    };
    // 10-bit planes are brought down to 8 bits once: a rare case, and the loop after
    // that is the same.
    let packed;
    let planes = if depth == 8 {
        Planes {
            y: &f.planes[0],
            u: &f.planes[1],
            v: &f.planes[2],
            ys: f.strides[0],
            us: f.strides[1],
            vs: f.strides[2],
        }
    } else {
        packed = pack8(f);
        Planes {
            y: &packed.0,
            u: &packed.1,
            v: &packed.2,
            ys: w,
            us: w / 2,
            vs: w / 2,
        }
    };
    let c = coefficients(s);
    let rot = rotation_deg(s);
    // Shrunk to the screen width: nobody sees 2520 pixels in a row, and every extra
    // thousand is megabytes per frame and extra work.
    let step = if max_width > 0 && w > max_width {
        w.div_ceil(max_width)
    } else {
        1
    };
    let (sw, sh) = (w.div_ceil(step), h.div_ceil(step));
    let (dw, dh) = if rot == 90 || rot == 270 {
        (sh, sw)
    } else {
        (sw, sh)
    };
    let cw = w / 2;
    let mut rgba = vec![0u8; dw * dh * 4];

    if rot == 0 {
        rgba.par_chunks_mut(sw * 4)
            .enumerate()
            .for_each(|(y, orow)| {
                let sy = y * step;
                let yrow = &planes.y[sy * planes.ys..sy * planes.ys + w];
                let urow = &planes.u[(sy / 2) * planes.us..(sy / 2) * planes.us + cw];
                let vrow = &planes.v[(sy / 2) * planes.vs..(sy / 2) * planes.vs + cw];
                for (x, px) in orow.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    let sx = x * step;
                    let (r, g, b) = pixel(c, yrow[sx], urow[sx / 2], vrow[sx / 2]);
                    px[0] = r;
                    px[1] = g;
                    px[2] = b;
                    px[3] = 255;
                }
            });
    } else {
        rgba.par_chunks_mut(dw * 4)
            .enumerate()
            .for_each(|(dy, orow)| {
                for (dx, px) in orow.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    let (ux, uy) = map_rot(rot, sw, sh, dx, dy);
                    let (sx, sy) = (ux * step, uy * step);
                    let (r, g, b) = pixel(
                        c,
                        planes.y[sy * planes.ys + sx],
                        planes.u[(sy / 2) * planes.us + sx / 2],
                        planes.v[(sy / 2) * planes.vs + sx / 2],
                    );
                    px[0] = r;
                    px[1] = g;
                    px[2] = b;
                    px[3] = 255;
                }
            });
    }

    Some(VideoFrame {
        width: dw,
        height: dh,
        rgba,
        pts_s: f.pts.map(|p| p as f64 * s.time_base.as_f64()).unwrap_or(0.0),
    })
}

/// The planes of a frame: either as they are (with the stride) or packed.
struct Planes<'a> {
    y: &'a [u8],
    u: &'a [u8],
    v: &'a [u8],
    ys: usize,
    us: usize,
    vs: usize,
}

/// 10-bit planes → 8 bits, with packed rows.
fn pack8(f: &RawFrame) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let (w, h) = (f.width as usize, f.height as usize);
    let (cw, ch) = (w / 2, h / 2);
    let mut out = (
        Vec::with_capacity(w * h),
        Vec::with_capacity(cw * ch),
        Vec::with_capacity(cw * ch),
    );
    let pack = |plane: &[u8], stride: usize, width: usize, height: usize, dst: &mut Vec<u8>| {
        for row in 0..height {
            let src = &plane[row * stride..row * stride + width * 2];
            dst.extend(src.as_chunks::<2>().0.iter().map(|b| (u16::from_le_bytes([b[0], b[1]]) >> 2) as u8));
        }
    };
    pack(&f.planes[0], f.strides[0], w, h, &mut out.0);
    pack(&f.planes[1], f.strides[1], cw, ch, &mut out.1);
    pack(&f.planes[2], f.strides[2], cw, ch, &mut out.2);
    out
}

/// One YUV→RGB pixel in integers (8.8).
#[inline]
fn pixel(c: Coef, y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let yy = ((y as i32 - c.yoff) * c.ys) >> 8;
    let u = u as i32 - 128;
    let v = v as i32 - 128;
    let r = yy + ((c.rv * v) >> 8);
    let g = yy - ((c.gu * u) >> 8) - ((c.gv * v) >> 8);
    let b = yy + ((c.bu * u) >> 8);
    (clamp_u8(r), clamp_u8(g), clamp_u8(b))
}

#[inline]
fn clamp_u8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// The coefficients in 8.8 and the luma offset: the limited range is 16…235, the full
/// one 0…255. The chroma coefficients for the full range are smaller by exactly as much
/// as 224 is smaller than 255.
#[derive(Clone, Copy)]
pub(super) struct Coef {
    pub(super) ys: i32,
    pub(super) yoff: i32,
    pub(super) rv: i32,
    pub(super) gu: i32,
    pub(super) gv: i32,
    pub(super) bu: i32,
}

pub(super) fn coefficients(s: &Stream) -> Coef {
    let full = matches!(s.color_range, ColorRange::Full);
    // (Rv, Gu, Gv, Bu) for the limited range, in 8.8.
    let (rv, gu, gv, bu) = match s.color.matrix {
        1 => (459, 55, 136, 541),         // BT.709
        5 | 6 => (409, 100, 208, 516),    // BT.601
        9 | 10 => (377, 42, 146, 481),    // BT.2020 NCL
        _ => {
            if s.width >= 1280 || s.height >= 720 {
                (459, 55, 136, 541)
            } else {
                (409, 100, 208, 516)
            }
        }
    };
    if full {
        Coef {
            ys: 256,
            yoff: 0,
            rv: rv * 224 / 255,
            gu: gu * 224 / 255,
            gv: gv * 224 / 255,
            bu: bu * 224 / 255,
        }
    } else {
        Coef {
            ys: 298,
            yoff: 16,
            rv,
            gu,
            gv,
            bu,
        }
    }
}

/// The rotation from the container metadata, reduced to 0/90/180/270.
fn rotation_deg(s: &Stream) -> u16 {
    let deg = s.rotation().unwrap_or(0.0);
    let deg = ((deg.round() as i64) % 360 + 360) % 360;
    match deg {
        45..=134 => 90,
        135..=224 => 180,
        225..=314 => 270,
        _ => 0,
    }
}

/// Where in the source frame the pixel (dx, dy) of a frame rotated by `rot` looks.
pub(super) fn map_rot(rot: u16, w: usize, h: usize, dx: usize, dy: usize) -> (usize, usize) {
    match rot {
        90 => (dy.min(w.saturating_sub(1)), h.saturating_sub(1).saturating_sub(dx)),
        180 => (
            w.saturating_sub(1).saturating_sub(dx),
            h.saturating_sub(1).saturating_sub(dy),
        ),
        270 => (w.saturating_sub(1).saturating_sub(dy), dx.min(h.saturating_sub(1))),
        _ => (dx, dy),
    }
}
