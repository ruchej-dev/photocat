//! Indexing progress: atomics, so the GUI thread can read it while a pass runs.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Indexing progress (atomics, so the GUI thread can read it).
#[derive(Default)]
pub struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
    errors: AtomicUsize,
    finished: AtomicBool,
    cancel: AtomicBool,
    msg: Mutex<String>,
}

impl Progress {
    pub fn set_total(&self, n: usize) {
        self.total.store(n, Ordering::Relaxed);
    }
    /// Start over (for instance, when moving to the next candidate model).
    pub fn reset(&self) {
        self.done.store(0, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        self.errors.store(0, Ordering::Relaxed);
        self.finished.store(false, Ordering::Relaxed);
    }
    pub fn tick(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }
    pub fn error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }
    pub fn finish(&self) {
        self.finished.store(true, Ordering::Relaxed);
    }
    /// Ask the pass to stop: the workers check the flag between files.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn set_msg(&self, s: impl Into<String>) {
        *self.msg.lock().unwrap() = s.into();
    }
    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }
    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }
    pub fn errors(&self) -> usize {
        self.errors.load(Ordering::Relaxed)
    }
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
    pub fn msg(&self) -> String {
        self.msg.lock().unwrap().clone()
    }
}
