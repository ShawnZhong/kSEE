//! Stopping the kernel: breakpoints and stepping.
//!
//! While the trace's `armed` word is set, every block probe calls
//! `ksee.stop(fn, n)`, this module's [`stop`] export, on the kernel's
//! Worker. If the block is a breakpoint, or the page asked to step, it
//! records where the kernel is and waits (a wasm atomic wait, in the
//! Worker) until the page resumes it. The kernel is uniprocessor, so every
//! other task is parked meanwhile: kernel memory is quiet to inspect.
//!
//! It does not stop in /init's traffic on the /dev/ksee channel (a read
//! or write on fd 100, see user/init.c): that is how the page's syscalls
//! get there, not one of them. The plugin's frames (see Machine::call_stack)
//! say which syscall the task is in.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use crate::imports::wall_ns;
use crate::state::{Direct, CURRENT_TASK, FRAMES, PAUSED_NS, STOPPED_AT, TRACE};
use ksee_core::Memory;

/// Run until a breakpoint; stop at the next block; stop at the next block
/// of one function on one task; stop at the next block on one task.
pub const RUN: u32 = 0;
pub const STEP: u32 = 1;
pub const NEXT: u32 = 2;
pub const TASK: u32 = 3;

const BREAKS: usize = 256;
/// A breakpoint's block number meaning "any block of the function".
pub const ANY: u32 = u32::MAX;

pub struct Debugger {
    mode: AtomicU32,
    next_fn: AtomicU32,
    next_task: AtomicU32,
    /// (fn << 32 | block) + 1 per breakpoint, 0 if free.
    breaks: [AtomicU64; BREAKS],
    /// Nonzero while stopped; then where, and on which task.
    stopped: AtomicU32,
    at_fn: AtomicU32,
    at_block: AtomicU32,
    at_task: AtomicU32,
    /// Bumped to let a stopped kernel go.
    resume: AtomicU32,
}

pub static DEBUG: Debugger = Debugger {
    mode: AtomicU32::new(RUN),
    next_fn: AtomicU32::new(0),
    next_task: AtomicU32::new(0),
    breaks: [const { AtomicU64::new(0) }; BREAKS],
    stopped: AtomicU32::new(0),
    at_fn: AtomicU32::new(0),
    at_block: AtomicU32::new(0),
    at_task: AtomicU32::new(0),
    resume: AtomicU32::new(0),
};

/// init's channel fd, and the syscalls it uses on it (read, write).
const CHANNEL_FD: u32 = 100;
const CHANNEL_NRS: [u32; 2] = [63, 64];

/// A function's id in the trace (plugin/ksee-trace.cpp: FNV-1a of the name).
const fn fn_id(name: &str) -> u32 {
    let b = name.as_bytes();
    let mut h: u32 = 2166136261;
    let mut i = 0;
    while i < b.len() {
        h = (h ^ b[i] as u32).wrapping_mul(16777619);
        i += 1;
    }
    h
}

/// Whether the current task is in a read or write on the channel: its
/// innermost wasm_syscall frame (args: nr, a0, ...) says.
fn in_channel_traffic() -> bool {
    const SYSCALL: u32 = fn_id("wasm_syscall");
    let head = FRAMES.load(Ordering::Relaxed);
    if head == 0 {
        return false;
    }
    let mut frame = Direct.read_u32(head);
    let top = frame.saturating_add(16 << 10);
    for _ in 0..64 {
        if frame == 0 || frame >= top {
            return false;
        }
        if Direct.read_u32(frame + 4) == SYSCALL {
            let (nr, fd) = (Direct.read_u32(frame + 8), Direct.read_u32(frame + 12));
            return fd == CHANNEL_FD && CHANNEL_NRS.contains(&nr);
        }
        let prev = Direct.read_u32(frame);
        if prev <= frame {
            return false;
        }
        frame = prev;
    }
    false
}

fn current_task() -> u32 {
    Direct.read_u32(CURRENT_TASK.load(Ordering::Relaxed))
}

/// ksee.stop: called by block probes while armed.
#[unsafe(no_mangle)]
pub extern "C" fn stop(func: u32, n: u32) {
    let d = &DEBUG;
    let task = current_task();
    let hit = match d.mode.load(Ordering::Acquire) {
        STEP => true,
        NEXT => func == d.next_fn.load(Ordering::Relaxed) && task == d.next_task.load(Ordering::Relaxed),
        TASK => task == d.next_task.load(Ordering::Relaxed),
        _ => false,
    } || d.is_break(func, n);
    if !hit || in_channel_traffic() {
        return;
    }
    d.mode.store(RUN, Ordering::Relaxed);
    d.at_fn.store(func, Ordering::Relaxed);
    d.at_block.store(n, Ordering::Relaxed);
    d.at_task.store(task, Ordering::Relaxed);
    let generation = d.resume.load(Ordering::Acquire);
    STOPPED_AT.store(wall_ns().max(1), Ordering::Release);
    d.stopped.store(1, Ordering::Release);
    d.rearm();
    while d.resume.load(Ordering::Acquire) == generation {
        unsafe {
            core::arch::wasm32::memory_atomic_wait32(d.resume.as_ptr() as *mut i32, generation as i32, -1);
        }
    }
    d.stopped.store(0, Ordering::Release);
    // (Paused time first: a reader in between sees the clock a bit behind,
    // not ahead.)
    let at = STOPPED_AT.load(Ordering::Acquire);
    PAUSED_NS.fetch_add(wall_ns().saturating_sub(at), Ordering::AcqRel);
    STOPPED_AT.store(0, Ordering::Release);
}

impl Debugger {
    fn is_break(&self, func: u32, n: u32) -> bool {
        let exact = ((func as u64) << 32 | n as u64) + 1;
        let any = ((func as u64) << 32 | ANY as u64) + 1;
        self.breaks.iter().any(|b| {
            let b = b.load(Ordering::Relaxed);
            b == exact || b == any
        })
    }

    /// Set the trace's armed word: probes call stop() only when needed.
    fn rearm(&self) {
        let armed = self.mode.load(Ordering::Relaxed) != RUN
            || self.breaks.iter().any(|b| b.load(Ordering::Relaxed) != 0);
        let word = TRACE.load(Ordering::Relaxed) + ksee_core::trace::ARMED;
        if word != ksee_core::trace::ARMED {
            unsafe { (*(word as *const AtomicU32)).store(armed as u32, Ordering::Release) };
        }
    }

    /// Where the kernel is stopped: (function, block, task, which stop
    /// this is), if it is.
    pub fn stopped(&self) -> Option<(u32, u32, u32, u32)> {
        (self.stopped.load(Ordering::Acquire) != 0).then(|| {
            (self.at_fn.load(Ordering::Relaxed), self.at_block.load(Ordering::Relaxed),
             self.at_task.load(Ordering::Relaxed), self.resume.load(Ordering::Relaxed))
        })
    }

    /// Stop at the next block the kernel runs.
    pub fn pause(&self) {
        self.mode.store(STEP, Ordering::Release);
        self.rearm();
    }

    /// Let a stopped kernel go, in `mode`.
    pub fn go(&self, mode: u32) {
        if mode == NEXT || mode == TASK {
            self.next_fn.store(self.at_fn.load(Ordering::Relaxed), Ordering::Relaxed);
            self.next_task.store(self.at_task.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        self.mode.store(mode, Ordering::Release);
        self.rearm();
        // Not stopped from here on (the Worker may take a while to wake).
        self.stopped.store(0, Ordering::Release);
        self.resume.fetch_add(1, Ordering::AcqRel);
        unsafe { core::arch::wasm32::memory_atomic_notify(self.resume.as_ptr() as *mut i32, u32::MAX) };
    }

    pub fn add(&self, func: u32, n: u32) -> bool {
        let key = ((func as u64) << 32 | n as u64) + 1;
        if self.breaks.iter().any(|b| b.load(Ordering::Relaxed) == key) {
            return true;
        }
        let ok = self.breaks.iter().any(|b| b.compare_exchange(0, key, Ordering::AcqRel, Ordering::Relaxed).is_ok());
        self.rearm();
        ok
    }

    pub fn remove(&self, func: u32, n: u32) {
        let key = ((func as u64) << 32 | n as u64) + 1;
        for b in &self.breaks {
            let _ = b.compare_exchange(key, 0, Ordering::AcqRel, Ordering::Relaxed);
        }
        self.rearm();
    }

    pub fn list(&self) -> Vec<(u32, u32)> {
        self.breaks.iter().map(|b| b.load(Ordering::Relaxed)).filter(|&b| b != 0)
            .map(|b| ((b - 1) >> 32) as u32).zip(
                self.breaks.iter().map(|b| b.load(Ordering::Relaxed)).filter(|&b| b != 0)
                    .map(|b| (b - 1) as u32)).collect()
    }
}
