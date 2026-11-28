//! The clock and the presentation: the frame that is ready, its time and the drop rule.

use super::audio::{AudioClock, Ring};
use super::decode::{to_rgba, FrameQueue};
use super::{VideoFrame, TICK};
use rff_format::Stream;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A frame later than this is dropped: there is no point in catching up.
const LATE_S: f64 = 0.20;

/// How much earlier than its time a frame counts as "early" and waits.
const EARLY_S: f64 = 0.004;

/// The state the interface reads.
pub(super) struct Shared {
    pub(super) frame: Mutex<Option<VideoFrame>>,
    pub(super) error: Mutex<Option<String>>,
    pub(super) position_ms: AtomicU64,
    pub(super) duration_ms: AtomicU64,
    pub(super) playing: AtomicBool,
    pub(super) finished: AtomicBool,
    pub(super) audio: AtomicBool,
    /// The sound stutters: the decoder cannot keep the sound card fed.
    pub(super) audio_lag: AtomicBool,
}

impl Shared {
    pub(super) fn new() -> Shared {
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

    pub(super) fn fail(&self, e: impl std::fmt::Display) {
        if let Ok(mut slot) = self.error.lock() {
            *slot = Some(e.to_string());
        }
        self.playing.store(false, Ordering::Relaxed);
    }
}

/// Control from the interface: no channels and no blocking.
pub(super) struct Control {
    pub(super) stop: AtomicBool,
    pub(super) seek_epoch: AtomicU64,
    pub(super) seek_ms: AtomicU64,
    /// A separate seek for the video only: the presentation asks it to "catch up with the
    /// sound" when the decoder has fallen behind so far that dropping frame by frame is pointless.
    pub(super) video_epoch: AtomicU64,
    pub(super) video_ms: AtomicU64,
}

impl Control {
    pub(super) fn new() -> Control {
        Control {
            stop: AtomicBool::new(false),
            seek_epoch: AtomicU64::new(0),
            seek_ms: AtomicU64::new(0),
            video_epoch: AtomicU64::new(0),
            video_ms: AtomicU64::new(0),
        }
    }
}

/// The wall clock: the position when there is no sound or it has not started yet.
pub(super) struct WallClock {
    base_s: f64,
    anchor: Instant,
    running: bool,
}

impl WallClock {
    pub(super) fn new() -> WallClock {
        WallClock {
            base_s: 0.0,
            anchor: Instant::now(),
            running: false,
        }
    }

    pub(super) fn position(&self) -> f64 {
        if self.running {
            self.base_s + self.anchor.elapsed().as_secs_f64()
        } else {
            self.base_s
        }
    }

    pub(super) fn play(&mut self) {
        if !self.running {
            self.anchor = Instant::now();
            self.running = true;
        }
    }

    pub(super) fn pause(&mut self) {
        if self.running {
            self.base_s = self.position();
            self.running = false;
        }
    }

    pub(super) fn set(&mut self, s: f64) {
        self.base_s = s.max(0.0);
        self.anchor = Instant::now();
    }
}

/// Presentation: it holds the clock, hands frames to the interface, drops the late ones.
pub(super) fn present_loop(
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
