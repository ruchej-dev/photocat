//! The built-in video player: pure Rust, no external programs and no C.
//!
//! Layer L2: it knows nothing about egui. It hands out a ready RGBA frame (already rotated
//! and downscaled by the container metadata) and the playback state; the interface draws.
//! `app.rs`.
//!
//! The demultiplexer and the decoders are the `rff` family: `rff-format-mp4` and
//! `rff-format-mkv` give packets with PTS and a sample index (seeking by key frame),
//! `rff-codec-h264` (over `rusty_h264`), `rff-codec-hevc` (over `rusty_h265`) and
//! `rff-codec-aac` decode. The sound goes out through `cpal` — ALSA/PipeWire, WASAPI,
//! CoreAudio, that is the API of the operating system, not a package to install.
//! Neither `ffmpeg`, nor `ffplay`, nor `mpv`, nor GStreamer take part in playback
//! (ADR 0030, 0031).
//!
//! Three threads, and this is not a luxury:
//!
//! ```text
//! photocat-video    demux + frame decode → queue (3 frames ahead)
//! photocat-audio    its own demuxer + audio decode → ring → cpal
//! photocat-player   the clock and the presentation: frame → RGBA → `Shared.frame`
//! ```
//!
//! Why exactly so. In one thread the frame and the audio decoding were interleaved: while
//! a frame was decoding, the audio ring ran dry, the sound choked, and the clock counted
//! from the played samples slowed down — and the video "barely moved" together with the
//! sound. Now the audio is fed by its own thread, and the clock takes the audio as the
//! master only while it really plays (the ring is not empty); otherwise the wall clock
//! runs, and the video does not stall.
//!
//! Memory is bounded: the frame queue holds 3 frames, the audio ring ~2 s, the frame being
//! presented one. Lag is dropped, not accumulated.
//!
//! Split by role (M27.5): `demux.rs` opens the container, `decode.rs` turns packets into
//! frames and RGBA, `audio.rs` feeds the sound card, `present.rs` holds the clock and the
//! drop rule; this module is the `Player` facade and the `play` command.

mod audio;
mod decode;
mod demux;
mod present;

#[cfg(test)]
mod tests;


use anyhow::{Context, Result};
use rff_core::Frame;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use audio::{audio_loop, AudioClock, Ring};
use decode::{make_decoder, to_rgba, video_loop, FrameQueue};
use demux::{next_for, Kind, Media, Source};
use present::{present_loop, Control, Shared};

/// The wait step in loop cycles.
const TICK: Duration = Duration::from_millis(5);

/// What the caller needs from the player.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Play sound. `false` is video without sound (headless measurements).
    pub audio: bool,
    /// Down to which width to shrink the frame before presenting. 0 means no shrinking.
    /// A screen does not show 2520 pixels in a row, and every extra thousand pixels is
    /// megabytes per frame and extra work in the conversion.
    pub max_width: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            audio: true,
            max_width: 1280,
        }
    }
}

/// A frame for presentation: RGBA in the screen orientation.
#[derive(Clone)]
pub struct VideoFrame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    pub pts_s: f64,
}

/// Playback of one file. It lives as long as the viewer is open.
pub struct Player {
    shared: Arc<Shared>,
    clock: Arc<AudioClock>,
    ring: Arc<Ring>,
    control: Arc<Control>,
    volume: Arc<AtomicU32>,
    threads: Vec<JoinHandle<()>>,
}

impl Player {
    /// Open a file and start playing. The error is an honest reason (no demuxer, a codec
    /// out of scope, a broken file); the caller decides whether to offer an external
    /// player.
    pub fn open(path: &Path, opts: Options) -> Result<Player> {
        let media = Media::open(path)?;
        let video_stream = media
            .video
            .clone()
            .with_context(|| format!("no video in the file: {}", path.display()))?;
        // The video decoder is checked at once: a codec out of scope is an honest error,
        // and `app` will have time to offer ffplay.
        let decoder = make_decoder(&video_stream)?;

        let shared = Arc::new(Shared::new());
        shared
            .duration_ms
            .store((media.duration_s() * 1000.0).max(0.0) as u64, Ordering::Relaxed);
        let clock = Arc::new(AudioClock::new());
        let ring = Arc::new(Ring::new());
        let control = Arc::new(Control::new());
        let volume = Arc::new(AtomicU32::new(1.0f32.to_bits()));
        let frames = Arc::new(FrameQueue::new(3));
        let mut threads = Vec::new();

        if opts.audio && media.audio.is_some() {
            let (p, s, r, c, ctl, v) = (
                path.to_path_buf(),
                shared.clone(),
                ring.clone(),
                clock.clone(),
                control.clone(),
                volume.clone(),
            );
            threads.push(
                std::thread::Builder::new()
                    .name("photocat-audio".into())
                    .spawn(move || audio_loop(p, s, r, c, ctl, v))
                    .context("cannot start the audio thread")?,
            );
            shared.audio.store(true, Ordering::Relaxed);
        } else {
            clock.state.store(2, Ordering::Relaxed);
        }

        let (f, ctl, sh) = (frames.clone(), control.clone(), shared.clone());
        threads.push(
            std::thread::Builder::new()
                .name("photocat-video".into())
                .spawn(move || video_loop(media, f, ctl, sh, Some(decoder)))
                .context("cannot start the decode thread")?,
        );

        let (f, sh, c, r, ctl) = (
            frames,
            shared.clone(),
            clock.clone(),
            ring.clone(),
            control.clone(),
        );
        threads.push(
            std::thread::Builder::new()
                .name("photocat-player".into())
                .spawn(move || present_loop(f, sh, c, r, ctl, video_stream, opts.max_width))
                .context("cannot start the presentation thread")?,
        );

        Ok(Player {
            shared,
            clock,
            ring,
            control,
            volume,
            threads,
        })
    }

    pub fn play(&self) {
        self.clock.paused.store(false, Ordering::Relaxed);
    }

    pub fn pause(&self) {
        self.clock.paused.store(true, Ordering::Relaxed);
    }

    /// Pause ↔ play.
    pub fn toggle(&self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&self, seconds: f64) {
        let ms = (seconds.max(0.0) * 1000.0) as u64;
        self.control.seek_ms.store(ms, Ordering::Relaxed);
        self.control.seek_epoch.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_volume(&self, volume: f32) {
        self.volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    /// A fresh frame if the decoder has produced a new one. Every frame is handed out once.
    pub fn take_frame(&self) -> Option<VideoFrame> {
        self.shared.frame.lock().ok().and_then(|mut f| f.take())
    }

    pub fn position_s(&self) -> f64 {
        self.shared.position_ms.load(Ordering::Relaxed) as f64 / 1000.0
    }

    pub fn duration_s(&self) -> f64 {
        self.shared.duration_ms.load(Ordering::Relaxed) as f64 / 1000.0
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    pub fn is_finished(&self) -> bool {
        self.shared.finished.load(Ordering::Relaxed)
    }

    pub fn has_audio(&self) -> bool {
        self.shared.audio.load(Ordering::Relaxed)
    }

    /// The sound stutters: the decoder cannot keep up.
    pub fn audio_lag(&self) -> bool {
        self.shared.audio_lag.load(Ordering::Relaxed)
    }

    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().ok().and_then(|e| e.clone())
    }

    /// Stop playback and release the sound device.
    ///
    /// **We do not wait for the threads.** `join` here used to hang the window: the owner
    /// pressed "Stop" and could not close the program. The threads see the flag and leave
    /// by themselves, and the handles are dropped — the interface has nothing to hang on.
    pub fn stop(&mut self) {
        self.control.stop.store(true, Ordering::Relaxed);
        self.clock.paused.store(true, Ordering::Relaxed);
        if self.clock.state.load(Ordering::Relaxed) == 1 {
            self.clock.state.store(3, Ordering::Relaxed);
        }
        self.ring.clear();
        self.threads.clear();
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

/// `photocat play FILE [--frames N] [--seek S] [--no-audio] [--decode-only]`
///
/// A check without a screen: how many frames the player manages to show and how fast it
/// decodes. `--decode-only` turns the clock off and measures the pure decode.
pub fn cli(args: &[String]) -> Result<()> {
    let path = match args.first() {
        Some(p) if !p.starts_with('-') => std::path::PathBuf::from(p),
        _ => {
            eprintln!("a file is needed: photocat play video.mp4 [--frames N] [--seek S] [--no-audio]");
            std::process::exit(2);
        }
    };
    let flag = |name: &str| args.iter().any(|a| a == name);
    let num = |name: &str, def: f64| -> f64 {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(def)
    };

    if flag("--decode-only") {
        let dump = args
            .iter()
            .position(|a| a == "--dump")
            .and_then(|i| args.get(i + 1))
            .cloned();
        return decode_only(
            &path,
            num("--frames", 0.0) as usize,
            !flag("--no-convert"),
            !flag("--stream"),
            dump,
        );
    }

    let opts = Options {
        audio: !flag("--no-audio"),
        max_width: num("--max-width", 1280.0) as usize,
    };
    let player = Player::open(&path, opts)?;
    let seek = num("--seek", 0.0);
    if seek > 0.0 {
        player.seek(seek);
    }
    let want = num("--frames", 0.0) as usize;
    let t0 = Instant::now();
    let mut shown = 0usize;
    let mut last_pts = 0.0f64;
    let mut first_frame_at = None;
    while shown < want.max(1) && !player.is_finished() {
        if let Some(f) = player.take_frame() {
            shown += 1;
            last_pts = f.pts_s;
            first_frame_at.get_or_insert_with(|| t0.elapsed().as_secs_f64());
        }
        if let Some(e) = player.error() {
            anyhow::bail!("{e}");
        }
        if t0.elapsed().as_secs_f64() > player.duration_s().max(1.0) + 10.0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let el = t0.elapsed().as_secs_f64();
    println!(
        "{}: {:.1} s, sound {}, {shown} frames in {el:.2} s ({:.1} fps), first frame {}",
        path.display(),
        player.duration_s(),
        if player.has_audio() { "yes" } else { "no" },
        shown as f64 / el.max(1e-6),
        first_frame_at.map(|t| format!("{t:.2} s")).unwrap_or("—".into()),
    );
    println!(
        "the last PTS: {last_pts:.2} s{}",
        if player.audio_lag() { ", the sound is off: the decoder cannot keep up" } else { "" }
    );
    Ok(())
}

/// Write a frame to PPM (P6) — checking pixels without pictures or dependencies.
fn write_ppm(path: &str, f: &VideoFrame) -> Result<()> {
    use std::io::Write;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    write!(out, "P6\n{} {}\n255\n", f.width, f.height)?;
    for px in f.rgba.as_chunks::<4>().0 {
        out.write_all(&px[..3])?;
    }
    out.flush()?;
    Ok(())
}

/// A pure decode without the clock and the sound: how many frames per second the codec gives.
fn decode_only(path: &Path, limit: usize, convert: bool, indexed: bool, dump: Option<String>) -> Result<()> {
    let mut media = Media::open(path)?;
    let video = media
        .video
        .clone()
        .with_context(|| format!("no video in the file: {}", path.display()))?;
    let mut dec = make_decoder(&video)?;
    let mut source = if indexed && media.demuxer.sample_index().is_some() {
        Source::Index { v: 0, a: 0 }
    } else {
        Source::Stream
    };
    let t0 = Instant::now();
    let mut n = 0usize;
    let mut converted = 0usize;
    let mut first = None;
    let mut convert_ns: u64 = 0;
    while let Some(pkt) = next_for(&mut media, &mut source, Kind::Video) {
        let _ = video.index;
        if dec.send_packet(&pkt).is_err() {
            continue;
        }
        while let Ok(Frame::Video(f)) = dec.receive_frame() {
            n += 1;
            first.get_or_insert_with(|| t0.elapsed().as_secs_f64());
            if convert {
                let tc = Instant::now();
                if let Some(vf) = to_rgba(&f, &video, 0) {
                    converted += 1;
                    if let Some(out) = dump.as_ref().filter(|_| converted == 1) {
                        write_ppm(out, &vf)?;
                    }
                }
                convert_ns += tc.elapsed().as_nanos() as u64;
            }
            if limit > 0 && n >= limit {
                break;
            }
        }
        if limit > 0 && n >= limit {
            break;
        }
    }
    let el = t0.elapsed().as_secs_f64();
    println!(
        "{}: {n} frames (RGBA {converted}) in {el:.2} s → {:.1} fps, first frame {}",
        path.display(),
        n as f64 / el.max(1e-6),
        first.map(|t| format!("{t:.2} s")).unwrap_or("—".into()),
    );
    println!(
        "  RGBA conversion: {:.1} ms/frame ({:.0}%)",
        convert_ns as f64 / 1e6 / n.max(1) as f64,
        100.0 * convert_ns as f64 / (el * 1e9).max(1.0)
    );
    Ok(())
}
