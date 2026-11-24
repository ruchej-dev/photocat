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

use anyhow::{Context, Result};
use rff_codec::{CodecParams, CodecRegistry, Decoder};
use rff_core::{ColorRange, Frame, MediaType, Packet, PixelFormat, VideoFrame as RawFrame};
use rff_format::{Demuxer, FormatRegistry, Stream};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The audio buffer ahead: ~2 s at 48 kHz stereo.
const RING_SAMPLES: usize = 192_000;
/// A frame later than this is dropped: there is no point in catching up.
const LATE_S: f64 = 0.20;
/// How much earlier than its time a frame counts as "early" and waits.
const EARLY_S: f64 = 0.004;
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

/// The state the interface reads.
struct Shared {
    frame: Mutex<Option<VideoFrame>>,
    error: Mutex<Option<String>>,
    position_ms: AtomicU64,
    duration_ms: AtomicU64,
    playing: AtomicBool,
    finished: AtomicBool,
    audio: AtomicBool,
    /// The sound stutters: the decoder cannot keep the sound card fed.
    audio_lag: AtomicBool,
}

impl Shared {
    fn new() -> Shared {
        Shared {
            frame: Mutex::new(None),
            error: Mutex::new(None),
            position_ms: AtomicU64::new(0),
            duration_ms: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            audio: AtomicBool::new(false),
            audio_lag: AtomicBool::new(false),
        }
    }

    fn fail(&self, e: impl std::fmt::Display) {
        if let Ok(mut slot) = self.error.lock() {
            *slot = Some(e.to_string());
        }
        self.playing.store(false, Ordering::Relaxed);
    }
}

/// The audio clock: the only source of truth about the position while sound plays.
struct AudioClock {
    /// 0 — the device is opening, 1 — playing, 2 — no sound, 3 — stopped.
    state: AtomicU8,
    played: AtomicU64,
    rate: AtomicU32,
    channels: AtomicU32,
    paused: AtomicBool,
    /// The first frame is shown: only now does the sound start playing, otherwise the
    /// audio clock runs ahead while the video is still speeding up.
    started: AtomicBool,
}

impl AudioClock {
    fn new() -> AudioClock {
        AudioClock {
            state: AtomicU8::new(0),
            played: AtomicU64::new(0),
            rate: AtomicU32::new(48_000),
            channels: AtomicU32::new(2),
            paused: AtomicBool::new(false),
            started: AtomicBool::new(false),
        }
    }

    fn playing(&self) -> bool {
        self.state.load(Ordering::Relaxed) == 1
    }

    fn rate(&self) -> u32 {
        self.rate.load(Ordering::Relaxed).max(1)
    }
}

/// The audio ring buffer between the decoder and the sound card.
struct Ring {
    buf: Mutex<VecDeque<f32>>,
}

impl Ring {
    fn new() -> Ring {
        Ring {
            buf: Mutex::new(VecDeque::with_capacity(RING_SAMPLES)),
        }
    }

    fn push_all(&self, samples: &[f32]) {
        if let Ok(mut b) = self.buf.lock() {
            b.extend(samples.iter().copied());
        }
    }

    fn len(&self) -> usize {
        self.buf.lock().map(|b| b.len()).unwrap_or(0)
    }

    /// The ring holds at least 20 ms of sound: the card does not starve.
    fn healthy(&self, rate: u32) -> bool {
        self.len() >= (rate as usize / 50).max(256)
    }

    fn clear(&self) {
        if let Ok(mut b) = self.buf.lock() {
            b.clear();
        }
    }
}

/// Control from the interface: no channels and no blocking.
struct Control {
    stop: AtomicBool,
    seek_epoch: AtomicU64,
    seek_ms: AtomicU64,
    /// A separate seek for the video only: the presentation asks it to "catch up with the
    /// sound" when the decoder has fallen behind so far that dropping frame by frame is pointless.
    video_epoch: AtomicU64,
    video_ms: AtomicU64,
}

impl Control {
    fn new() -> Control {
        Control {
            stop: AtomicBool::new(false),
            seek_epoch: AtomicU64::new(0),
            seek_ms: AtomicU64::new(0),
            video_epoch: AtomicU64::new(0),
            video_ms: AtomicU64::new(0),
        }
    }
}

/// The queue of decoded frames between the decode thread and the presentation thread.
struct FrameQueue {
    q: Mutex<VecDeque<Queued>>,
    cap: usize,
    /// The decoder reached the end of the file.
    closed: AtomicBool,
}

struct Queued {
    pts_s: f64,
    frame: RawFrame,
}

impl FrameQueue {
    fn new(cap: usize) -> FrameQueue {
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
    fn pop(&self) -> Option<Queued> {
        self.q.lock().ok().and_then(|mut q| q.pop_front())
    }

    fn clear(&self) {
        if let Ok(mut q) = self.q.lock() {
            q.clear();
        }
    }
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

// ---------------------------------------------------------------- the media file

/// An open container: the demuxer and the streams found.
struct Media {
    demuxer: Box<dyn Demuxer>,
    video: Option<Stream>,
    audio: Option<Stream>,
}

impl Media {
    fn open(path: &Path) -> Result<Media> {
        let reg = format_registry();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let name = reg
            .by_extension(&ext)
            .map(|f| f.name)
            .or_else(|| {
                // An unfamiliar extension: try by the content.
                let mut head = [0u8; 64];
                let n = std::fs::File::open(path)
                    .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
                    .unwrap_or(0);
                reg.probe(&head[..n]).map(|f| f.name)
            })
            .with_context(|| format!("unknown video format: {}", path.display()))?;

        let file = std::fs::File::open(path)
            .with_context(|| format!("cannot open the video {}", path.display()))?;
        let mut demuxer = reg
            .open_demuxer_seekable(name, Box::new(file))
            .or_else(|_| {
                let f = std::fs::File::open(path)?;
                reg.open_demuxer(name, Box::new(f))
            })
            .with_context(|| format!("cannot parse the container {}", path.display()))?;
        let streams = demuxer
            .read_header()
            .with_context(|| format!("the container has no header: {}", path.display()))?;
        let video = streams.iter().find(|s| s.media_type == MediaType::Video).cloned();
        let audio = streams.iter().find(|s| s.media_type == MediaType::Audio).cloned();
        if video.is_none() && audio.is_none() {
            anyhow::bail!("the file has neither video nor sound: {}", path.display());
        }
        Ok(Media {
            demuxer,
            video,
            audio,
        })
    }

    fn duration_s(&self) -> f64 {
        let of = |s: &Stream| {
            s.duration
                .map(|d| d as f64 * s.time_base.as_f64())
                .unwrap_or(0.0)
        };
        self.video
            .as_ref()
            .map(of)
            .filter(|d| *d > 0.0)
            .or_else(|| self.audio.as_ref().map(of).filter(|d| *d > 0.0))
            .unwrap_or(0.0)
    }
}

fn format_registry() -> FormatRegistry {
    let mut reg = FormatRegistry::new();
    rff_format_mp4::register(&mut reg);
    rff_format_mkv::register(&mut reg);
    reg
}

fn codec_registry() -> CodecRegistry {
    let mut reg = CodecRegistry::new();
    rff_codec_h264::register(&mut reg);
    rff_codec_hevc::register(&mut reg);
    rff_codec_aac::register(&mut reg);
    reg
}

/// A decoder for a stream. A codec out of scope (VP9, Opus, …) is an honest error:
/// the caller will offer an external player.
fn make_decoder(stream: &Stream) -> Result<Box<dyn Decoder>> {
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


/// Open the sound device and start the thread. We try every output: the default device
/// is sometimes busy (PipeWire holds the hw device, while cpal goes through ALSA).
fn open_output_any(
    ring: &Arc<Ring>,
    clock: &Arc<AudioClock>,
    volume: &Arc<AtomicU32>,
) -> Option<(cpal::Stream, u32, usize)> {
    use cpal::traits::HostTrait;

    let host = cpal::default_host();
    let mut candidates: Vec<cpal::Device> = Vec::new();
    if let Some(d) = host.default_output_device() {
        candidates.push(d);
    }
    if let Ok(devices) = host.output_devices() {
        candidates.extend(devices);
    }
    for device in candidates {
        if let Some(ok) = open_output(&device, ring, clock, volume) {
            return Some(ok);
        }
    }
    None
}

fn open_output(
    device: &cpal::Device,
    ring: &Arc<Ring>,
    clock: &Arc<AudioClock>,
    volume: &Arc<AtomicU32>,
) -> Option<(cpal::Stream, u32, usize)> {
    use cpal::traits::{DeviceTrait, StreamTrait};

    let mut supported = Vec::new();
    if let Ok(c) = device.default_output_config() {
        supported.push(c);
    }
    if let Ok(list) = device.supported_output_configs() {
        supported.extend(
            list.filter(|r| r.sample_format() == cpal::SampleFormat::F32)
                .map(|r| r.with_max_sample_rate()),
        );
    }
    let cfg = supported
        .into_iter()
        .find(|c| c.sample_format() == cpal::SampleFormat::F32)?;
    let config = cfg.config();
    let channels = config.channels.max(1) as usize;

    let (r, c, v) = (ring.clone(), clock.clone(), volume.clone());
    let cb = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        if c.paused.load(Ordering::Relaxed) || !c.started.load(Ordering::Relaxed) {
            data.fill(0.0);
            return;
        }
        let vol = f32::from_bits(v.load(Ordering::Relaxed));
        let mut filled = 0usize;
        if let Ok(mut buf) = r.buf.lock() {
            while filled < data.len() {
                match buf.pop_front() {
                    Some(s) => {
                        data[filled] = s * vol;
                        filled += 1;
                    }
                    None => break,
                }
            }
        }
        if filled < data.len() {
            data[filled..].fill(0.0);
        }
        c.played
            .fetch_add((filled / channels) as u64, Ordering::Relaxed);
    };

    let err_clock = clock.clone();
    let stream = device
        .build_output_stream::<f32, _, _>(
            config,
            cb,
            move |_e| {
                // The device went away: the video continues on the wall clock.
                err_clock.state.store(2, Ordering::Relaxed);
            },
            None,
        )
        .ok()?;
    stream.play().ok()?;
    Some((stream, config.sample_rate, channels))
}

/// Resampling into the device format: channels and rate.
struct Resampler {
    src_rate: u32,
    dst_rate: u32,
    src_ch: usize,
    dst_ch: usize,
    prev: Vec<f32>,
    frac: f64,
    started: bool,
}

impl Resampler {
    fn new(src_rate: u32, src_ch: usize, dst_rate: u32, dst_ch: usize) -> Resampler {
        Resampler {
            src_rate: src_rate.max(1),
            dst_rate: dst_rate.max(1),
            src_ch: src_ch.max(1),
            dst_ch: dst_ch.max(1),
            prev: Vec::new(),
            frac: 0.0,
            started: false,
        }
    }

    fn passthrough(&self) -> bool {
        self.src_rate == self.dst_rate && self.src_ch == self.dst_ch
    }

    /// Lay a source frame into the device buffer.
    fn push(&mut self, src: &[f32], out: &mut Vec<f32>) {
        if self.passthrough() {
            out.extend_from_slice(src);
            return;
        }
        let mut cur = vec![0.0f32; self.dst_ch];
        for frame in src.chunks(self.src_ch) {
            map_channels(frame, &mut cur);
            if !self.started {
                self.prev = cur.clone();
                self.started = true;
            }
            let ratio = self.src_rate as f64 / self.dst_rate as f64;
            while self.frac < 1.0 {
                for ch in 0..self.dst_ch {
                    let a = self.prev.get(ch).copied().unwrap_or(0.0);
                    let b = cur.get(ch).copied().unwrap_or(0.0);
                    out.push(a + (b - a) * self.frac as f32);
                }
                self.frac += ratio;
            }
            self.frac -= 1.0;
            self.prev.copy_from_slice(&cur);
        }
    }
}

/// Spread a source frame over the device channels.
fn map_channels(frame: &[f32], out: &mut [f32]) {
    let src = frame.len();
    let dst = out.len();
    match (src, dst) {
        (a, b) if a == b => out.copy_from_slice(frame),
        (1, _) => out.fill(frame[0]),
        (a, 1) if a > 1 => out[0] = frame.iter().sum::<f32>() / a as f32,
        _ => {
            for (i, slot) in out.iter_mut().enumerate() {
                *slot = frame.get(i).copied().unwrap_or(0.0);
            }
        }
    }
}

/// A packet → f32 interleaved.
fn audio_to_f32(frame: &rff_core::AudioFrame) -> Vec<f32> {
    use rff_core::SampleFormat;
    let ch = frame.channels.max(1) as usize;
    let n = frame.samples;
    let mut out = Vec::with_capacity(n * ch);
    match frame.format {
        SampleFormat::F32 => {
            if let Some(p) = frame.planes.first() {
                out.extend(p.as_chunks::<4>().0.iter().take(n * ch).map(|b| {
                    f32::from_le_bytes([b[0], b[1], b[2], b[3]])
                }));
            }
        }
        SampleFormat::F32Planar => {
            for i in 0..n {
                for p in frame.planes.iter().take(ch) {
                    out.push(p.get(i * 4..i * 4 + 4).map_or(0.0, |b| {
                        f32::from_le_bytes([b[0], b[1], b[2], b[3]])
                    }));
                }
            }
        }
        SampleFormat::S16 => {
            if let Some(p) = frame.planes.first() {
                out.extend(p.as_chunks::<2>().0.iter().take(n * ch).map(|b| {
                    i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0
                }));
            }
        }
        SampleFormat::S32 => {
            if let Some(p) = frame.planes.first() {
                out.extend(p.as_chunks::<4>().0.iter().take(n * ch).map(|b| {
                    i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0
                }));
            }
        }
        SampleFormat::U8 => {
            if let Some(p) = frame.planes.first() {
                out.extend(p.iter().take(n * ch).map(|&b| (b as f32 - 128.0) / 128.0));
            }
        }
        _ => {}
    }
    out
}


// ---------------------------------------------------------------- the clock

/// The wall clock: the position when there is no sound or it has not started yet.
struct WallClock {
    base_s: f64,
    anchor: Instant,
    running: bool,
}

impl WallClock {
    fn new() -> WallClock {
        WallClock {
            base_s: 0.0,
            anchor: Instant::now(),
            running: false,
        }
    }

    fn position(&self) -> f64 {
        if self.running {
            self.base_s + self.anchor.elapsed().as_secs_f64()
        } else {
            self.base_s
        }
    }

    fn play(&mut self) {
        if !self.running {
            self.anchor = Instant::now();
            self.running = true;
        }
    }

    fn pause(&mut self) {
        if self.running {
            self.base_s = self.position();
            self.running = false;
        }
    }

    fn set(&mut self, s: f64) {
        self.base_s = s.max(0.0);
        self.anchor = Instant::now();
    }
}


// ---------------------------------------------------------------- the threads

/// Which stream is wanted: video or audio.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Video,
    Audio,
}

/// Where the packets come from: the sample index (MP4/MKV) or sequential reading.
enum Source {
    Stream,
    Index { v: u64, a: u64 },
}

fn new_source(media: &mut Media) -> Source {
    if media.demuxer.sample_index().is_some() {
        Source::Index { v: 0, a: 0 }
    } else {
        Source::Stream
    }
}

/// The next packet of the wanted stream. In index mode the video and the audio have
/// their own cursors, so a full frame queue does not stop the sound and vice versa.
fn next_for(media: &mut Media, source: &mut Source, kind: Kind) -> Option<Packet> {
    match source {
        Source::Stream => loop {
            let pkt = media.demuxer.read_packet().ok()?;
            let want = match kind {
                Kind::Video => media.video.as_ref(),
                Kind::Audio => media.audio.as_ref(),
            };
            if want.is_some_and(|s| s.index == pkt.stream_index) {
                return Some(pkt);
            }
        },
        Source::Index { v, a } => {
            let idx = media.demuxer.sample_index()?;
            let (stream, cursor) = match kind {
                Kind::Video => (media.video.as_ref()?.index, v),
                Kind::Audio => (media.audio.as_ref()?.index, a),
            };
            if *cursor >= idx.sample_count(stream) {
                return None;
            }
            let pkt = idx.read_sample(stream, *cursor).ok()?;
            *cursor += 1;
            Some(pkt)
        }
    }
}

/// Decode video and put the frames into the queue. It sleeps only when the queue is
/// full — that is, when the presentation cannot keep up.
fn video_loop(
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
                while let Ok(Frame::Video(f)) = dec.receive_frame() {
                    let pts = f.pts.map(|p| p as f64 * tb).unwrap_or(0.0);
                    frames.push(pts, f);
                }
            }
            None => done = true,
        }
    }
    frames.closed.store(true, Ordering::Relaxed);
}

/// Decode audio and feed the sound card. Its own demuxer: so the sound does not depend
/// on how long the frame decoding took.
fn audio_loop(
    path: PathBuf,
    shared: Arc<Shared>,
    ring: Arc<Ring>,
    clock: Arc<AudioClock>,
    control: Arc<Control>,
    volume: Arc<AtomicU32>,
) {
    let Some((device, rate, channels)) = open_output_any(&ring, &clock, &volume) else {
        clock.state.store(2, Ordering::Relaxed);
        return;
    };
    clock.rate.store(rate, Ordering::Relaxed);
    clock.channels.store(channels as u32, Ordering::Relaxed);
    clock.state.store(1, Ordering::Relaxed);

    let mut media = match Media::open(&path) {
        Ok(m) => m,
        Err(e) => {
            shared.fail(format!("sound: {e:#}"));
            clock.state.store(2, Ordering::Relaxed);
            return;
        }
    };
    let Some(stream) = media.audio.clone() else {
        clock.state.store(2, Ordering::Relaxed);
        return;
    };
    let mut decoder = make_decoder(&stream).ok();
    let mut source = new_source(&mut media);
    let mut epoch = 0u64;
    let mut resampler = Resampler::new(
        stream.sample_rate.max(1),
        stream.channels.max(1) as usize,
        rate,
        channels,
    );
    let mut buf: Vec<f32> = Vec::new();

    while !control.stop.load(Ordering::Relaxed) && clock.state.load(Ordering::Relaxed) == 1 {
        if clock.paused.load(Ordering::Relaxed) || !clock.started.load(Ordering::Relaxed) {
            std::thread::sleep(TICK);
            continue;
        }
        let e = control.seek_epoch.load(Ordering::Relaxed);
        if e != epoch {
            epoch = e;
            ring.clear();
            let target = control.seek_ms.load(Ordering::Relaxed) as f64 / 1000.0;
            decoder = media.audio.as_ref().and_then(|s| make_decoder(s).ok());
            if let Some(idx) = media.demuxer.sample_index() {
                let tb = stream.time_base.as_f64();
                let pts = (target / tb) as i64;
                let start = idx.sync_at_or_before(stream.index, pts).unwrap_or(0);
                source = Source::Index {
                    v: match &source {
                        Source::Index { v, .. } => *v,
                        Source::Stream => 0,
                    },
                    a: start,
                };
            } else {
                source = Source::Stream;
            }
            continue;
        }
        if ring.len() + 4096 >= RING_SAMPLES {
            std::thread::sleep(TICK);
            continue;
        }
        match next_for(&mut media, &mut source, Kind::Audio) {
            Some(pkt) => {
                let Some(dec) = decoder.as_mut() else { break };
                if dec.send_packet(&pkt).is_err() {
                    continue;
                }
                while let Ok(Frame::Audio(af)) = dec.receive_frame() {
                    buf.clear();
                    resampler.push(&audio_to_f32(&af), &mut buf);
                    ring.push_all(&buf);
                }
            }
            None => {
                // The sound is over: wait for stop, seek or pause.
                std::thread::sleep(TICK);
            }
        }
    }

    clock.state.store(3, Ordering::Relaxed);
    drop(device);
}

/// Presentation: it holds the clock, hands frames to the interface, drops the late ones.
fn present_loop(
    frames: Arc<FrameQueue>,
    shared: Arc<Shared>,
    clock: Arc<AudioClock>,
    ring: Arc<Ring>,
    control: Arc<Control>,
    video: Stream,
    max_width: usize,
) {
    let mut wall = WallClock::new();
    wall.play();
    let mut played_base = clock.played.load(Ordering::Relaxed);
    let mut started = false;
    let mut paused_last = false;
    let mut epoch = 0u64;
    // A frame already converted to RGBA and waiting for its time.
    let mut ready: Option<(f64, VideoFrame)> = None;
    let mut starving = 0u32;
    // Catching up is expensive (a decode from a key frame), so not more than once in 1.5 s.
    let mut last_catch_up: Option<Instant> = None;
    // If catching up was needed twice in 4 s, the decoder cannot keep up with the sound:
    // we turn the sound off and play slowly but smoothly (better than 2 frames per second).
    let mut catch_ups: VecDeque<Instant> = VecDeque::new();
    let mut audio_off = false;

    let position = |wall: &WallClock, played_base: u64| -> f64 {
        // The sound is the master only while it really plays: the ring holds data.
        if clock.playing()
            && clock.started.load(Ordering::Relaxed)
            && ring.healthy(clock.rate())
        {
            let played = clock
                .played
                .load(Ordering::Relaxed)
                .saturating_sub(played_base);
            let audio_pos = wall.base_s + played as f64 / clock.rate() as f64;
            if (audio_pos - wall.position()).abs() < 0.35 {
                return audio_pos;
            }
        }
        wall.position()
    };

    while !control.stop.load(Ordering::Relaxed) {
        let paused = clock.paused.load(Ordering::Relaxed);
        if paused != paused_last {
            if paused {
                let pos = position(&wall, played_base);
                wall.pause();
                wall.set(pos);
            } else {
                wall.play();
                played_base = clock.played.load(Ordering::Relaxed);
            }
            paused_last = paused;
        }
        let e = control.seek_epoch.load(Ordering::Relaxed);
        if e != epoch {
            epoch = e;
            wall.set(control.seek_ms.load(Ordering::Relaxed) as f64 / 1000.0);
            played_base = clock.played.load(Ordering::Relaxed);
            started = false;
            ready = None;
            if !paused {
                wall.play();
            }
        }
        if paused {
            shared.playing.store(false, Ordering::Relaxed);
            std::thread::sleep(TICK);
            continue;
        }
        shared.playing.store(true, Ordering::Relaxed);

        // The conversion is done as soon as a frame appears: not at the moment of presentation.
        if ready.is_none()
            && let Some(q) = frames.pop()
            && let Some(vf) = to_rgba(&q.frame, &video, max_width)
        {
            ready = Some((q.pts_s, vf));
        }
        let Some((pts, _)) = ready.as_ref() else {
            if frames.closed.load(Ordering::Relaxed) {
                shared.finished.store(true, Ordering::Relaxed);
                break;
            }
            std::thread::sleep(TICK);
            continue;
        };
        let pts = *pts;
        if !started {
            wall.set(pts.max(0.0));
            played_base = clock.played.load(Ordering::Relaxed);
            started = true;
            // The video is ready — the sound may start: the clock will run from the first frame.
            clock.started.store(true, Ordering::Relaxed);
        }
        let audio_master = !audio_off
            && clock.playing()
            && clock.started.load(Ordering::Relaxed)
            && ring.healthy(clock.rate());
        let now = position(&wall, played_base);
        // The late tolerance: without sound frames are not dropped at all (the clock follows
        // the decoder), with sound up to 0.5 s, so that a shortage of 2–3 frames per second
        // does not turn the video into a slideshow.
        let late = if audio_master { 0.5 } else { LATE_S };
        if pts + late < now {
            if !audio_master {
                // No sound (or it is off already): the clock follows the decoder, and the
                // video plays slower, but smoothly.
                wall.set(pts);
                played_base = clock.played.load(Ordering::Relaxed);
            } else {
                // The sound is the master. If the decoder has fallen far behind, we ask it to
                // jump to the current position: otherwise every frame in a row would be
                // dropped and the video would stand still.
                let may_catch_up =
                    last_catch_up.is_none_or(|t| t.elapsed() > Duration::from_millis(1500));
                if now - pts > 1.2 && may_catch_up {
                    control
                        .video_ms
                        .store((now * 1000.0) as u64, Ordering::Relaxed);
                    control.video_epoch.fetch_add(1, Ordering::Relaxed);
                    last_catch_up = Some(Instant::now());
                    catch_ups.push_back(Instant::now());
                    while catch_ups
                        .front()
                        .is_some_and(|t| t.elapsed() > Duration::from_secs(4))
                    {
                        catch_ups.pop_front();
                    }
                    if catch_ups.len() >= 2 {
                        // The decoder cannot cope: the sound goes off, the video runs as it can.
                        audio_off = true;
                        clock.state.store(2, Ordering::Relaxed);
                        ring.clear();
                        shared.audio_lag.store(true, Ordering::Relaxed);
                        shared.audio.store(false, Ordering::Relaxed);
                        wall.set(pts);
                        played_base = clock.played.load(Ordering::Relaxed);
                        ready = Some((pts, ready.take().map(|(_, vf)| vf).unwrap()));
                        continue;
                    }
                }
                // We are behind, but within the tolerance: present as it is.
            }
        }
        if pts > now + EARLY_S {
            // Is the sound starving? Counted, but not more than once in 60 ticks.
            if clock.playing() && clock.started.load(Ordering::Relaxed) {
                if ring.healthy(clock.rate()) {
                    starving = 0;
                } else {
                    starving += 1;
                    if starving == 60 {
                        shared.audio_lag.store(true, Ordering::Relaxed);
                    }
                }
            }
            std::thread::sleep(TICK);
            continue;
        }
        if let Some((_, vf)) = ready.take()
            && let Ok(mut slot) = shared.frame.lock()
        {
            *slot = Some(vf);
        }
        shared
            .position_ms
            .store((pts.max(0.0) * 1000.0) as u64, Ordering::Relaxed);
    }
    shared.playing.store(false, Ordering::Relaxed);
}

// ---------------------------------------------------------------- the frame

/// YUV → RGBA honouring the matrix, the range and the rotation from the container.
///
/// Rows are computed in parallel (rayon): 2520×1080 is 2.7 million pixels, one thread
/// cannot do them within a frame. Integer arithmetic in 8.8, no floats.
/// Formats beyond 4:2:0 8/10 bit are not promised: the caller will yield to an external
/// player (ADR 0030).
fn to_rgba(f: &RawFrame, s: &Stream, max_width: usize) -> Option<VideoFrame> {
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
struct Coef {
    ys: i32,
    yoff: i32,
    rv: i32,
    gu: i32,
    gv: i32,
    bu: i32,
}

fn coefficients(s: &Stream) -> Coef {
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
fn map_rot(rot: u16, w: usize, h: usize, dx: usize, dy: usize) -> (usize, usize) {
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


// ---------------------------------------------------------------- CLI

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


#[cfg(test)]
mod tests {
    use super::*;
    use rff_core::{CodecId, ColorInfo, Rational};

    fn stream(w: u32, h: u32, matrix: u16, range: ColorRange) -> Stream {
        let mut s = Stream::new(0, CodecId::H264);
        s.media_type = MediaType::Video;
        s.time_base = Rational::new(1, 30);
        s.width = w;
        s.height = h;
        s.color = ColorInfo {
            primaries: 1,
            transfer: 1,
            matrix,
        };
        s.color_range = range;
        s
    }

    fn yuv420(w: usize, h: usize, y: u8, u: u8, v: u8) -> RawFrame {
        RawFrame {
            width: w as u32,
            height: h as u32,
            format: PixelFormat::Yuv420p,
            planes: vec![
                vec![y; w * h],
                vec![u; (w / 2) * (h / 2)],
                vec![v; (w / 2) * (h / 2)],
            ],
            strides: vec![w, w / 2, w / 2],
            pts: Some(0),
        }
    }

    #[test]
    fn a_black_frame_stays_black() {
        // The limited range: Y=16, Cb=Cr=128 is black.
        let f = yuv420(4, 2, 16, 128, 128);
        let out = to_rgba(&f, &stream(4, 2, 1, ColorRange::Limited), 0).unwrap();
        assert_eq!((out.width, out.height), (4, 2));
        for px in out.rgba.chunks(4) {
            assert!(px[0] <= 1 && px[1] <= 1 && px[2] <= 1, "не чёрный: {px:?}");
        }
    }

    #[test]
    fn a_white_frame_stays_white() {
        let f = yuv420(4, 2, 235, 128, 128);
        let out = to_rgba(&f, &stream(4, 2, 1, ColorRange::Limited), 0).unwrap();
        for px in out.rgba.chunks(4) {
            assert!(px[0] >= 254 && px[1] >= 254 && px[2] >= 254, "не белый: {px:?}");
        }
    }

    #[test]
    fn the_full_range_reads_differently() {
        let f = yuv420(2, 2, 16, 128, 128);
        let limited = to_rgba(&f, &stream(2, 2, 1, ColorRange::Limited), 0).unwrap();
        let full = to_rgba(&f, &stream(2, 2, 1, ColorRange::Full), 0).unwrap();
        assert!(limited.rgba[0] <= 1, "ограниченный Y=16 должен быть чёрным");
        assert!(full.rgba[0] >= 14, "в полном диапазоне Y=16 — тёмно-серый");
    }

    #[test]
    fn a_90_degree_rotation_swaps_the_sides() {
        let f = yuv420(4, 2, 128, 128, 128);
        let mut s = stream(4, 2, 1, ColorRange::Limited);
        s.display_matrix = Some([0, 1 << 16, 0, -(1 << 16), 0, 0, 0, 0, 1 << 30]);
        let out = to_rgba(&f, &s, 0).unwrap();
        assert_eq!((out.width, out.height), (2, 4));
    }

    #[test]
    fn the_rotation_map_agrees_with_the_sizes() {
        // 90°: the bottom-left corner of the source becomes the top-left one.
        assert_eq!(map_rot(90, 4, 2, 0, 0), (0, 1));
        assert_eq!(map_rot(90, 4, 2, 1, 3), (3, 0));
        assert_eq!(map_rot(180, 4, 2, 0, 0), (3, 1));
        assert_eq!(map_rot(270, 4, 2, 0, 0), (3, 0));
        assert_eq!(map_rot(0, 4, 2, 2, 1), (2, 1));
    }

    #[test]
    fn an_unknown_frame_format_is_not_promised() {
        let mut f = yuv420(2, 2, 128, 128, 128);
        f.format = PixelFormat::Yuv444p;
        assert!(to_rgba(&f, &stream(2, 2, 1, ColorRange::Limited), 0).is_none());
    }

    #[test]
    fn the_matrix_from_the_container_beats_the_size() {
        let hd = stream(1920, 1080, 2, ColorRange::Limited);
        let sd = stream(640, 480, 2, ColorRange::Limited);
        assert_eq!(coefficients(&hd).rv, 459, "HD без метки — BT.709");
        assert_eq!(coefficients(&sd).rv, 409, "SD без метки — BT.601");
        let tagged = stream(640, 480, 1, ColorRange::Limited);
        assert_eq!(coefficients(&tagged).rv, 459, "метка 709 сильнее размера");
    }

    #[test]
    fn a_resampler_without_changes_copies() {
        let mut r = Resampler::new(48_000, 2, 48_000, 2);
        let mut out = Vec::new();
        r.push(&[0.1, 0.2, 0.3, 0.4], &mut out);
        assert_eq!(out, vec![0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn the_resampler_doubles_the_rate() {
        let mut r = Resampler::new(24_000, 1, 48_000, 1);
        let mut out = Vec::new();
        r.push(&[0.0, 1.0, 0.0, 1.0], &mut out);
        // Roughly two output samples per input sample.
        assert!((out.len() as i64 - 8).abs() <= 1, "получено {}", out.len());
    }

    #[test]
    fn channels_are_laid_out_by_the_device() {
        let mut mono = [0.0];
        map_channels(&[0.5], &mut mono);
        assert_eq!(mono[0], 0.5);
        let mut stereo = [0.0, 0.0];
        map_channels(&[0.5], &mut stereo);
        assert_eq!(stereo, [0.5, 0.5]);
        let mut one = [0.0];
        map_channels(&[0.2, 0.4], &mut one);
        assert!((one[0] - 0.3).abs() < 1e-6);
    }

    #[test]
    fn an_f32_audio_packet_reads_as_is() {
        let f = rff_core::AudioFrame {
            sample_rate: 48_000,
            channels: 1,
            format: rff_core::SampleFormat::F32,
            planes: vec![0.25f32.to_le_bytes().to_vec()],
            samples: 1,
            pts: Some(0),
        };
        let out = audio_to_f32(&f);
        assert_eq!(out.len(), 1);
        assert!((out[0] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn s16_audio_is_converted_to_f32() {
        let f = rff_core::AudioFrame {
            sample_rate: 44_100,
            channels: 1,
            format: rff_core::SampleFormat::S16,
            planes: vec![i16::MAX.to_le_bytes().to_vec()],
            samples: 1,
            pts: None,
        };
        let out = audio_to_f32(&f);
        assert!((out[0] - 1.0).abs() < 0.001);
    }

    #[test]
    fn the_clock_counts_the_position_and_the_pause() {
        let mut w = WallClock::new();
        w.set(10.0);
        assert!((w.position() - 10.0).abs() < 1e-6);
        w.play();
        std::thread::sleep(Duration::from_millis(20));
        assert!(w.position() > 10.0);
        w.pause();
        let p = w.position();
        std::thread::sleep(Duration::from_millis(20));
        assert!((w.position() - p).abs() < 1e-6, "на паузе позиция не растёт");
    }

    #[test]
    fn the_registry_knows_the_corpus_codecs() {
        let reg = codec_registry();
        for id in [CodecId::H264, CodecId::Hevc, CodecId::Aac] {
            assert!(reg.find_decoder(id).is_ok(), "нет декодера {id:?}");
        }
    }

    #[test]
    fn the_duration_comes_from_the_container() {
        let mut s = stream(1920, 1080, 1, ColorRange::Limited);
        s.duration = Some(90); // 90 ticks of 1/30 s = 3 s
        let media = Media {
            demuxer: format_registry().open_demuxer("mp4", Box::new(std::io::empty())).unwrap(),
            video: Some(s),
            audio: None,
        };
        assert!((media.duration_s() - 3.0).abs() < 1e-9);
    }
}

