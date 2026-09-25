//! Statics shared by every instance (they live in the shared memory). The
//! page may not block, so what it touches is lock-free.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use ksee_core::tables::Tables;
use ksee_core::{task, Memory};

/// The probe tables, set once by the page before boot.
pub static TABLES: OnceLock<Tables> = OnceLock::new();
pub static CMDLINE: OnceLock<Vec<u8>> = OnceLock::new();
/// End of the kernel's RAM (it may use memory below it).
pub static MEMORY_END: AtomicU32 = AtomicU32::new(0);
/// 0 while running, else the halt code + 1.
pub static HALTED: AtomicU32 = AtomicU32::new(0);
/// f64 bits of the wall clock when the machine started.
pub static EPOCH_MS: AtomicU64 = AtomicU64::new(0);
/// Addresses in kernel memory: the trace (ksee_trace()) and the variable
/// holding the current task (KSEE_CURRENT).
pub static TRACE: AtomicU32 = AtomicU32::new(0);
/// The trace ring's size in words.
pub static RING_WORDS: AtomicU32 = AtomicU32::new(0);
pub static CURRENT_TASK: AtomicU32 = AtomicU32::new(0);
/// The kernel's pending-interrupt word (wasm_irq_pending).
pub static PENDING: AtomicU32 = AtomicU32::new(0);

/// The kernel's request to run a program (wasm_run, see its
/// arch/wasm/kernel/run.c): `{ u32 state; s32 status; s32 pid; char path[256]; }`.
pub static RUN: AtomicU32 = AtomicU32::new(0);
/// Its states.
pub const RUN_IDLE: u32 = 0;
pub const RUN_REQUESTED: u32 = 1;
pub const RUN_DONE: u32 = 3;
pub const RUN_PATH: usize = 256;

/// Raise interrupt line `irq` (see WASM_IRQ_* in the kernel's asm/wasm.h).
pub fn interrupt(irq: u32) {
    let p = PENDING.load(Ordering::Relaxed);
    if p != 0 {
        unsafe {
            (*(p as *const AtomicU32)).fetch_or(1 << irq, Ordering::SeqCst);
            core::arch::wasm32::memory_atomic_notify(p as *mut i32, 1);
        }
    }
}

pub static CONSOLE_OUT: ByteRing = ByteRing::new();
pub static CONSOLE_IN: ByteRing = ByteRing::new();

pub fn tables() -> &'static Tables {
    TABLES.get().expect("no kernel loaded")
}

/// The kernel's memory is this module's memory.
pub struct Direct;

impl Memory for Direct {
    /// Reads outside memory (a stale pointer in the trace, say) give zeros.
    fn read(&self, addr: u32, buf: &mut [u8]) {
        let size = core::arch::wasm32::memory_size(0) as u64 * 65536;
        if addr as u64 + buf.len() as u64 > size {
            buf.fill(0);
            return;
        }
        // Racing the kernel only garbles what is read.
        unsafe { core::ptr::copy_nonoverlapping(addr as *const u8, buf.as_mut_ptr(), buf.len()) }
    }
}

pub fn live_task(p: u32) -> Option<task::Task> {
    task::read_task(tables(), &Direct, p)
}

/// A byte ring written by one side and read by the other; the writer drops
/// the oldest bytes if the reader falls behind.
pub struct ByteRing {
    buf: [AtomicU32; ByteRing::WORDS],
    head: AtomicU32, // bytes ever written
    tail: AtomicU32, // bytes ever read
}

impl ByteRing {
    const WORDS: usize = 1 << 12;
    const CAP: u32 = Self::WORDS as u32 * 4;

    pub const fn new() -> Self {
        ByteRing {
            buf: [const { AtomicU32::new(0) }; Self::WORDS],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
        }
    }

    fn byte(&self, i: u32) -> (&AtomicU32, u32) {
        let i = i % Self::CAP;
        (&self.buf[(i / 4) as usize], (i % 4) * 8)
    }

    pub fn put(&self, data: &[u8]) {
        let head = self.head.load(Ordering::Relaxed);
        for (i, &b) in data.iter().enumerate() {
            let (w, shift) = self.byte(head + i as u32);
            let old = w.load(Ordering::Relaxed);
            w.store(old & !(0xff << shift) | (b as u32) << shift, Ordering::Relaxed);
        }
        self.head.store(head + data.len() as u32, Ordering::Release);
    }

    pub fn take(&self, out: &mut [u8]) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed).max(head.saturating_sub(Self::CAP));
        let n = ((head - tail) as usize).min(out.len());
        for (i, b) in out[..n].iter_mut().enumerate() {
            let (w, shift) = self.byte(tail + i as u32);
            *b = (w.load(Ordering::Relaxed) >> shift) as u8;
        }
        self.tail.store(tail + n as u32, Ordering::Relaxed);
        n
    }
}

/// Whether the trace records (its `on` word; probes check it).
pub fn recording() -> bool {
    let trace = TRACE.load(Ordering::Relaxed);
    trace != 0 && Direct.read_u32(trace + ksee_core::trace::ON) != 0
}
