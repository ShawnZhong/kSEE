//! The machine's state in the runtime: one Kernel (what the kernel's
//! imports and the page share; a thread-local, as the kernel's imports have
//! no context to be given), and the probe tables.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::sync::OnceLock;

use js_sys::Function;
use ksee_core::tables::Tables;
use ksee_core::Memory;

/// The probe tables, set once by the page before boot.
pub static TABLES: OnceLock<Tables> = OnceLock::new();

pub fn tables() -> &'static Tables {
    TABLES.get().expect("no kernel loaded")
}

/// Where the kernel's memory starts (CONFIG_WASM_HOST_MEMORY: below it,
/// the runtime's own static data and stack).
pub const KERNEL_BASE: u32 = 0x100_0000;

/// Interrupt lines (WASM_IRQ_* in the kernel's asm/wasm.h).
pub const IRQ_TIMER: u32 = 0;
pub const IRQ_DOORBELL: u32 = 1;

/// The kernel's clock advances this much at each read (so that a loop on
/// it ends), besides where idle skips to the next timer (see sched).
pub const TICK_NS: u64 = 1000;

#[derive(Default)]
pub struct Kernel {
    /// Addresses in kernel memory (each boot's): the trace (ksee_trace()),
    /// the variable holding the current task, the pending interrupts
    /// (wasm_irq_pending), and the timer's deadline (wasm_timer_deadline).
    pub trace: Cell<u32>,
    pub current: Cell<u32>,
    pub pending: Cell<u32>,
    pub deadline: Cell<u32>,
    /// End of the kernel's RAM, and its command line.
    pub ram: Cell<u32>,
    pub cmdline: RefCell<Vec<u8>>,
    /// The kernel's clock (ns): virtual, so that a run is the same each
    /// time (it moves only as the kernel does: see TICK_NS and sched).
    pub clock: Cell<u64>,
    /// Whether the page waits for the kernel (a boot, a call; then idle
    /// skips to the next timer), since when (the clock); whether it boots.
    pub waiting: Cell<Option<u64>>,
    pub booting: Cell<bool>,
    /// Whether the kernel is quiet: idle, until the page's input.
    pub quiet: Cell<bool>,
    /// What waits for it to be quiet (Machine::quiet): settled with the
    /// calls finished since the last time (or rejected: it halted).
    pub quiet_waiters: RefCell<Vec<(Function, Function)>>,
    pub finished: RefCell<Vec<Finished>>,
    /// The halt code, once the kernel has halted.
    pub halted: Cell<Option<i32>>,
    pub console_out: RefCell<Vec<u8>>,
    /// The tasks to ring (WASM_IRQ_DOORBELL: see the kernel's irq.c).
    pub doorbell: RefCell<VecDeque<u32>>,
    /// The runners ready for the page's calls (user/init.c): thread id ->
    /// task, and thread id -> the word each waits on (`rung`: set to ring
    /// it); the calls asked of them (see Machine::call), and the count of
    /// calls asked.
    pub runners: RefCell<BTreeMap<u32, u32>>,
    pub rung: RefCell<BTreeMap<u32, u32>>,
    pub calls: RefCell<BTreeMap<u32, Call>>,
    pub asked: Cell<u32>,
    /// Every call asked (by id): its runner, its number, and the id its
    /// recording's records start from (see Machine::syscall_of).
    pub made: RefCell<BTreeMap<u32, (u32, u32, u32)>>,
}

/// A call finished: returned (its result, and its buffers as it left them),
/// or ended by an execve (the new program is running) or by its task's
/// exit.
pub struct Finished {
    pub id: u32,
    pub tid: u32,
    pub how: &'static str,
    pub ret: i64,
    pub buffers: Vec<Option<Vec<u8>>>,
}

thread_local! {
    static K: Kernel = Kernel::default();
}

/// Run `f` on the Kernel.
pub fn k<T>(f: impl FnOnce(&Kernel) -> T) -> T {
    K.with(f)
}

/// A syscall for a runner to make (user/init.c): asked, then taken (the
/// runner makes it), then finished.
#[derive(Default)]
pub struct Call {
    pub id: u32,
    pub taken: bool,
    pub nr: u32,
    /// Each an integer, or (bit i of `buffers`) the offset of a buffer in
    /// `data` (its length: `lens`).
    pub args: [u32; 6],
    pub buffers: u32,
    pub lens: [u32; 6],
    pub data: Vec<u8>,
    /// The words of `data` that are pointers into it (an array of strings'
    /// elements): offsets there, made addresses where the runner has it.
    pub relocs: Vec<u32>,
}

/// The size of a call's buffers, all together.
pub const CALL_DATA: usize = 4096;

/// The kernel's memory is this module's memory.
pub struct Direct;

impl Direct {
    pub fn write_u32(&self, addr: u32, v: u32) {
        unsafe { (addr as *mut u32).write_volatile(v) }
    }
}

impl Memory for Direct {
    /// Reads outside memory (a stale pointer in the trace, say) give zeros.
    fn read(&self, addr: u32, buf: &mut [u8]) {
        let size = core::arch::wasm32::memory_size(0) as u64 * 65536;
        if addr as u64 + buf.len() as u64 > size {
            buf.fill(0);
            return;
        }
        unsafe { core::ptr::copy_nonoverlapping(addr as *const u8, buf.as_mut_ptr(), buf.len()) }
    }
}

/// The task running now (the kernel's __current_task).
pub fn current() -> u32 {
    Direct.read_u32(k(|k| k.current.get()))
}

/// Whether the trace records (its `on` word; probes check it).
pub fn recording() -> bool {
    let trace = k(|k| k.trace.get());
    trace != 0 && Direct.read_u32(trace + ksee_core::trace::ON) != 0
}

/// The kernel's timer deadline (ns of its clock), u64::MAX if none.
pub fn deadline() -> u64 {
    let d = k(|k| k.deadline.get());
    if d == 0 {
        return u64::MAX;
    }
    let mut b = [0u8; 8];
    Direct.read(d, &mut b);
    u64::from_le_bytes(b)
}
