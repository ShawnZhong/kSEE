//! Statics shared by every instance (they live in the shared memory). The
//! page may not block, so what it touches is lock-free.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use ksee_core::tables::Tables;
use ksee_core::Memory;

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
/// The kernel's pending-interrupt word (wasm_irq_pending), and its timer's
/// deadline (wasm_timer_deadline).
pub static PENDING: AtomicU32 = AtomicU32::new(0);
pub static DEADLINE: AtomicU32 = AtomicU32::new(0);

/// Interrupt lines (WASM_IRQ_* in the kernel's asm/wasm.h).
pub const IRQ_TIMER: u32 = 0;
pub const IRQ_CONSOLE: u32 = 1;

/// Raise interrupt line `irq`.
pub fn interrupt(irq: u32) {
    let p = PENDING.load(Ordering::Relaxed);
    if p != 0 {
        unsafe {
            (*(p as *const AtomicU32)).fetch_or(1 << irq, Ordering::SeqCst);
            core::arch::wasm32::memory_atomic_notify(p as *mut i32, 1);
        }
    }
}

/// New tasks waiting for a Worker (see worker.rs): the kernel's
/// spawn_task puts them here (from the Worker holding the CPU: one at a
/// time), and idle Workers take them.
pub static SPAWN: Spawns = Spawns::new();

pub struct Spawns {
    /// (task << 32 | sp) per slot.
    slots: [AtomicU64; Spawns::SLOTS],
    /// Tasks ever put, and taken.
    put: AtomicU32,
    taken: AtomicU32,
    /// Workers waiting for a task.
    pub idle: AtomicU32,
}

impl Spawns {
    const SLOTS: usize = 256;

    pub const fn new() -> Self {
        Spawns { slots: [const { AtomicU64::new(0) }; Self::SLOTS], put: AtomicU32::new(0),
                 taken: AtomicU32::new(0), idle: AtomicU32::new(0) }
    }

    /// Queue task `task` (its stack pointer `sp`) for a Worker. Whether
    /// none waits (so another is needed).
    pub fn put(&self, task: u32, sp: u32) -> bool {
        let n = self.put.load(Ordering::Relaxed);
        self.slots[n as usize % Self::SLOTS].store((task as u64) << 32 | sp as u64, Ordering::Relaxed);
        self.put.store(n + 1, Ordering::Release);
        unsafe { core::arch::wasm32::memory_atomic_notify(self.put.as_ptr() as *mut i32, 1) };
        self.idle.load(Ordering::Acquire) == 0
    }

    /// The next task to run: (task, sp); waits for one.
    pub fn take(&self) -> (u32, u32) {
        loop {
            let (t, put) = (self.taken.load(Ordering::Acquire), self.put.load(Ordering::Acquire));
            if t < put {
                if self.taken.compare_exchange(t, t + 1, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                    let v = self.slots[t as usize % Self::SLOTS].load(Ordering::Relaxed);
                    return ((v >> 32) as u32, v as u32);
                }
                continue;
            }
            self.idle.fetch_add(1, Ordering::AcqRel);
            unsafe { core::arch::wasm32::memory_atomic_wait32(self.put.as_ptr() as *mut i32, put as i32, -1) };
            self.idle.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

pub static CONSOLE_OUT: ByteRing = ByteRing::new();
pub static CONSOLE_IN: ByteRing = ByteRing::new();

/// A syscall the page asks /init to make (user/init.c; see Machine::call):
/// the page fills it and sets CALL_ASKED; /init takes it (ksee.call), makes
/// it, and gives the result back (ksee.done): CALL_DONE.
pub static CALL: Call = Call::new();
pub const CALL_IDLE: u32 = 0;
pub const CALL_ASKED: u32 = 1;
pub const CALL_DONE: u32 = 2;

pub struct Call {
    pub state: AtomicU32,
    pub nr: AtomicU32,
    /// Each an integer, or (bit i of `buffers`) the offset of a buffer in
    /// `data` (its length: `lens`).
    pub args: [AtomicU32; 6],
    pub buffers: AtomicU32,
    pub lens: [AtomicU32; 6],
    /// The result (i64 bits).
    pub ret: AtomicU64,
    pub data: core::cell::UnsafeCell<[u8; CALL_DATA]>,
}
pub const CALL_DATA: usize = 4096;

// (data: written by one side at a time, the state word says which.)
unsafe impl Sync for Call {}

impl Call {
    const fn new() -> Self {
        Call { state: AtomicU32::new(0), nr: AtomicU32::new(0), args: [const { AtomicU32::new(0) }; 6],
               buffers: AtomicU32::new(0), lens: [const { AtomicU32::new(0) }; 6], ret: AtomicU64::new(0),
               data: core::cell::UnsafeCell::new([0; CALL_DATA]) }
    }

    pub fn data(&self) -> &mut [u8; CALL_DATA] {
        unsafe { &mut *self.data.get() }
    }
}

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

/// A byte ring written by the kernel's Workers and read by the page; the
/// writer drops the oldest bytes if the reader falls behind.
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
