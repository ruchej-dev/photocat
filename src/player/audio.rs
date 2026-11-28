//! The sound: its own demuxer, the device, the ring buffer and the audio clock.

use super::decode::make_decoder;
use super::demux::{new_source, next_for, Kind, Media, Source};
use super::present::{Control, Shared};
use super::TICK;
use rff_core::Frame;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

const RING_SAMPLES: usize = 192_000;

/// The audio clock: the only source of truth about the position while sound plays.
pub(super) struct AudioClock {
    /// 0 — the device is opening, 1 — playing, 2 — no sound, 3 — stopped.
    pub(super) state: AtomicU8,
    pub(super) played: AtomicU64,
    pub(super) rate: AtomicU32,
    pub(super) channels: AtomicU32,
    pub(super) paused: AtomicBool,
    /// The first frame is shown: only now does the sound start playing, otherwise the
    /// audio clock runs ahead while the video is still speeding up.
    pub(super) started: AtomicBool,
}

impl AudioClock {
    pub(super) fn new() -> AudioClock {
        AudioClock {
            state: AtomicU8::new(0),
            played: AtomicU64::new(0),
            rate: AtomicU32::new(48_000),
            channels: AtomicU32::new(2),
            paused: AtomicBool::new(false),
            started: AtomicBool::new(false),
        }
    }

    pub(super) fn playing(&self) -> bool {
        self.state.load(Ordering::Relaxed) == 1
    }

    pub(super) fn rate(&self) -> u32 {
        self.rate.load(Ordering::Relaxed).max(1)
    }
}

/// The audio ring buffer between the decoder and the sound card.
pub(super) struct Ring {
    pub(super) buf: Mutex<VecDeque<f32>>,
}

impl Ring {
    pub(super) fn new() -> Ring {
        Ring {
            buf: Mutex::new(VecDeque::with_capacity(RING_SAMPLES)),
        }
    }

    pub(super) fn push_all(&self, samples: &[f32]) {
        if let Ok(mut b) = self.buf.lock() {
            b.extend(samples.iter().copied());
        }
    }

    pub(super) fn len(&self) -> usize {
        self.buf.lock().map(|b| b.len()).unwrap_or(0)
    }

    /// The ring holds at least 20 ms of sound: the card does not starve.
    pub(super) fn healthy(&self, rate: u32) -> bool {
        self.len() >= (rate as usize / 50).max(256)
    }

    pub(super) fn clear(&self) {
        if let Ok(mut b) = self.buf.lock() {
            b.clear();
        }
    }
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
pub(super) struct Resampler {
    src_rate: u32,
    dst_rate: u32,
    src_ch: usize,
    dst_ch: usize,
    prev: Vec<f32>,
    frac: f64,
    started: bool,
}

impl Resampler {
    pub(super) fn new(src_rate: u32, src_ch: usize, dst_rate: u32, dst_ch: usize) -> Resampler {
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
    pub(super) fn push(&mut self, src: &[f32], out: &mut Vec<f32>) {
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
pub(super) fn map_channels(frame: &[f32], out: &mut [f32]) {
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
pub(super) fn audio_to_f32(frame: &rff_core::AudioFrame) -> Vec<f32> {
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

/// Decode audio and feed the sound card. Its own demuxer: so the sound does not depend
/// on how long the frame decoding took.
pub(super) fn audio_loop(
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
