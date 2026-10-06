//! The machine's state in the runtime (what the kernel's imports and the
//! page share; thread-local, as the kernel's imports have no context to be
//! given), and the probe tables.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::OnceLock;

use js_sys::Function;
use ksee_core::tables::Tables;
use ksee_core::program::Session;
use ksee_core::Memory;

/// The probe tables, set once by the page before boot.
pub static TABLES: OnceLock<Tables> = OnceLock::new();

pub fn tables() -> &'static Tables {
    TABLES.get().expect("no kernel loaded")
}

/// Where the kernel's memory starts (CONFIG_WASM_HOST_MEMORY: below it,
/// the runtime's own static data and stack; checked at boot).
pub const KERNEL_BASE: u32 = 0x100_0000;
/// Where it ends.
pub const RAM_BYTES: u32 = 64 << 20;

/// Interrupt lines (WASM_IRQ_* in the kernel's asm/wasm.h: the same).
pub const IRQ_TIMER: u32 = 0;
pub const IRQ_DOORBELL: u32 = 1;
pub const IRQ_DISK: u32 = 2;

/// The kernel's clock advances this much at each read (so that a loop on
/// it ends), besides where idle skips to the next timer (see sched).
pub const TICK_NS: u64 = 1000;

#[derive(Default)]
pub struct State {
    /// Addresses in kernel memory (each boot's): the trace (ksee_trace),
    /// the variable holding the current task, the pending interrupts
    /// (wasm_irq_pending), and the timer's deadline (wasm_timer_deadline).
    pub trace: u32,
    pub current: u32,
    pub pending: u32,
    pub deadline: u32,
    /// The kernel's command line.
    pub cmdline: Vec<u8>,
    /// The kernel's clock (ns): virtual, so that a run is the same each
    /// time (it moves only as the kernel does: see TICK_NS and sched).
    pub clock: u64,
    /// Whether the page waits for the kernel (a boot, a call; then idle
    /// skips to the next timer), since when (the clock); whether it boots.
    pub waiting: Option<u64>,
    pub booting: bool,
    /// What waits for it to be quiet (Machine::quiet): settled (or
    /// rejected: it halted).
    pub quiet_waiters: Vec<(Function, Function)>,
    /// The halt code, once the kernel has halted.
    pub halted: Option<i32>,
    pub console_out: Vec<u8>,
    /// The tasks to ring (WASM_IRQ_DOORBELL: see the kernel's irq.c).
    pub doorbell: VecDeque<u32>,
    /// The disk (arch/wasm/drivers/disk.c): its blocks; the transfers handed to
    /// it and not yet made, the tags of the
    /// requests done; and every transfer made (the Disk tab's).
    pub disk: Vec<u8>,
    pub transfers: Vec<Transfer>,
    pub done: VecDeque<u32>,
    pub disk_log: Vec<Transfer>,
    /// The page's program: its runners, calls and lines.
    pub session: Session,
}

/// A transfer the disk made: its request's tag, `len` bytes at `sector`
/// to (a read) or from (`write`) kernel memory at `buf`; the trace's words
/// written when it was handed over (`asked`) and when the kernel took it
/// as done (`done`: in the disk's interrupt, its tag taken); and its bytes, `after` (a write's: what the disk holds then;
/// a read's: what it read), and a write's `before`.
#[derive(Clone, Debug, Default)]
pub struct Transfer {
    /// The task it was handed over by (its task_struct: the kernel's
    /// current then), whether recorded or not.
    pub task: u32,
    pub tag: u32,
    pub sector: u64,
    pub buf: u32,
    pub len: u32,
    pub write: bool,
    pub asked: u32,
    pub done: u32,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

thread_local! {
    static K: RefCell<State> = RefCell::default();
}

/// Read the machine's state.
pub fn state<T>(f: impl FnOnce(&State) -> T) -> T {
    K.with_borrow(f)
}

/// Change the machine's state.
pub fn state_mut<T>(f: impl FnOnce(&mut State) -> T) -> T {
    K.with_borrow_mut(f)
}

/// The kernel's memory is this module's memory.
pub struct Direct;

impl Direct {
    pub fn write_u32(&self, addr: u32, v: u32) {
        unsafe { (addr as *mut u32).write_volatile(v) }
    }
}

/// A wasm memory page, in bytes.
pub(crate) const PAGE_BYTES: u32 = 65536;

impl Memory for Direct {
    /// Reads outside memory (a stale pointer in the trace, say) give zeros.
    fn read(&self, addr: u32, buf: &mut [u8]) {
        let size = core::arch::wasm32::memory_size(0) as u64 * PAGE_BYTES as u64;
        if addr as u64 + buf.len() as u64 > size {
            buf.fill(0);
            return;
        }
        unsafe { core::ptr::copy_nonoverlapping(addr as *const u8, buf.as_mut_ptr(), buf.len()) }
    }
}

/// The task running now (the kernel's __current_task).
pub fn current() -> u32 {
    Direct.read_u32(state(|s| s.current))
}

/// The kernel's timer deadline (ns of its clock), u64::MAX if none.
pub fn deadline() -> u64 {
    let d = state(|s| s.deadline);
    if d == 0 {
        return u64::MAX;
    }
    let mut b = [0u8; 8];
    Direct.read(d, &mut b);
    u64::from_le_bytes(b)
}
