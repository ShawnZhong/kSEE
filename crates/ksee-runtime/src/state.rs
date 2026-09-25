//! The machine's state in the runtime: one thread, so plain statics (for
//! addresses, set once) and thread-locals (for what changes).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use ksee_core::tables::Tables;
use ksee_core::Memory;

/// The probe tables, set once by the page before boot.
pub static TABLES: OnceLock<Tables> = OnceLock::new();
pub static CMDLINE: OnceLock<Vec<u8>> = OnceLock::new();
/// Where the kernel's memory starts (CONFIG_WASM_HOST_MEMORY: below it,
/// the runtime's own static data and stack).
pub const KERNEL_BASE: u32 = 0x100_0000;
/// End of the kernel's RAM (it may use memory below it).
pub static MEMORY_END: AtomicU32 = AtomicU32::new(0);
/// 0 while running, else the halt code + 1.
pub static HALTED: AtomicU32 = AtomicU32::new(0);
/// f64 bits of the wall clock when the machine started.
pub static EPOCH_MS: AtomicU64 = AtomicU64::new(0);
/// Addresses in kernel memory: the trace (ksee_trace()), the variable
/// holding the current task, the pending interrupts (wasm_irq_pending), and
/// the timer's deadline (wasm_timer_deadline).
pub static TRACE: AtomicU32 = AtomicU32::new(0);
pub static CURRENT_TASK: AtomicU32 = AtomicU32::new(0);
pub static PENDING: AtomicU32 = AtomicU32::new(0);
pub static DEADLINE: AtomicU32 = AtomicU32::new(0);
/// The trace ring's size in words.
pub static RING_WORDS: AtomicU32 = AtomicU32::new(0);

/// Interrupt lines (WASM_IRQ_* in the kernel's asm/wasm.h).
pub const IRQ_TIMER: u32 = 0;
pub const IRQ_CONSOLE: u32 = 1;

thread_local! {
    pub static CONSOLE_OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    pub static CONSOLE_IN: RefCell<VecDeque<u8>> = const { RefCell::new(VecDeque::new()) };
    /// The syscall the page asks /init to make (see Machine::call).
    pub static CALL: RefCell<Call> = RefCell::new(Call::default());
}

/// A syscall for /init to make (user/init.c): asked, then made (its
/// promise settled).
#[derive(Default)]
pub struct Call {
    pub state: CallState,
    pub nr: u32,
    /// Each an integer, or (bit i of `buffers`) the offset of a buffer in
    /// `data` (its length: `lens`).
    pub args: [u32; 6],
    pub buffers: u32,
    pub lens: [u32; 6],
    pub data: Vec<u8>,
    /// Settles Machine::call's promise.
    pub resolve: Option<js_sys::Function>,
    pub reject: Option<js_sys::Function>,
}

#[derive(Default, PartialEq, Clone, Copy)]
pub enum CallState {
    #[default]
    Idle,
    Asked,
}

/// The size of a call's buffers, all together.
pub const CALL_DATA: usize = 4096;

/// Raise interrupt line `irq`, and wake the CPU if it idles.
pub fn interrupt(irq: u32) {
    let p = PENDING.load(Ordering::Relaxed);
    if p != 0 {
        Direct.write_u32(p, Direct.read_u32(p) | 1 << irq);
    }
    crate::sched::wake();
}

pub fn tables() -> &'static Tables {
    TABLES.get().expect("no kernel loaded")
}

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

/// Whether the trace records (its `on` word; probes check it).
pub fn recording() -> bool {
    let trace = TRACE.load(Ordering::Relaxed);
    trace != 0 && Direct.read_u32(trace + ksee_core::trace::ON) != 0
}

/// The kernel's timer deadline (ns of time_ns), u64::MAX if none.
pub fn deadline() -> u64 {
    let d = DEADLINE.load(Ordering::Relaxed);
    if d == 0 {
        return u64::MAX;
    }
    let mut b = [0u8; 8];
    Direct.read(d, &mut b);
    u64::from_le_bytes(b)
}
