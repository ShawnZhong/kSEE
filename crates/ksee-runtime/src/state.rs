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
    /// What waits for it to be quiet (Machine::quiet): settled with the
    /// calls finished since the last time (or rejected: it halted).
    pub quiet_waiters: RefCell<Vec<(Function, Function)>>,
    pub finished: RefCell<Vec<Finished>>,
    /// The halt code, once the kernel has halted.
    pub halted: Cell<Option<i32>>,
    pub console_out: RefCell<Vec<u8>>,
    /// The tasks to ring (WASM_IRQ_DOORBELL: see the kernel's irq.c).
    pub doorbell: RefCell<VecDeque<u32>>,
    /// The runners ready for the page's calls (user/prog.c), by thread id.
    pub runners: RefCell<BTreeMap<u32, Runner>>,
    /// The program's own runner (user/prog.c's main thread: the page's
    /// calls' by default), once it is ready.
    pub main: Cell<Option<u32>>,
    /// The programs' own memory (user/prog.c's heap: a process's, its
    /// threads' and vfork children's too, the same memory): what of each
    /// is handed out (its start -> (next free, end)).
    pub heaps: RefCell<BTreeMap<u32, (u32, u32)>>,
    /// The calls asked (by id: see Machine::run), this boot's; and how many
    /// ever (the last id: ids are not used again, a boot after another's).
    pub calls: RefCell<BTreeMap<u32, Call>>,
    pub asked: Cell<u32>,
    /// The page's program: its names (a variable's, a result's), and how
    /// many of its lines have been made (see Machine::run).
    pub names: RefCell<ksee_core::program::Names>,
    pub lines: Cell<usize>,
}

impl Kernel {
    /// The call runner `tid` is making (asked, not done), if any: its id.
    pub fn in_flight(&self, tid: u32) -> Option<u32> {
        self.calls.borrow().values().find(|c| c.tid == tid && !c.done).map(|c| c.id)
    }

    /// Runner `tid` is one no more (its task exited, or it ran another
    /// program): its request, room and heap forgotten (a heap no runner has
    /// left, its process's gone: another's may come to the same address).
    pub fn forget(&self, tid: u32) {
        let mut runners = self.runners.borrow_mut();
        if let Some(r) = runners.remove(&tid) && !runners.values().any(|o| o.heap == r.heap) {
            self.heaps.borrow_mut().remove(&r.heap);
        }
        drop(runners);
        if self.main.get() == Some(tid) {
            self.main.set(None); // (the program exited: /init starts it again)
        }
    }
}

/// A runner: its task (its task_struct), its request (where a call is put:
/// its first word, the number, is what the runner waits on), and its
/// program's heap (its start).
#[derive(Clone, Copy)]
pub struct Runner {
    pub task: u32,
    pub req: u32,
    pub heap: u32,
}

/// A call finished: returned (its result), or ended by an execve (the new
/// program is running) or by its task's exit.
pub struct Finished {
    pub id: u32,
    pub tid: u32,
    pub how: &'static str,
    pub ret: i64,
}

thread_local! {
    static K: Kernel = Kernel::default();
}

/// Run `f` on the Kernel.
pub fn k<T>(f: impl FnOnce(&Kernel) -> T) -> T {
    K.with(f)
}

/// A syscall a runner makes (user/prog.c): asked (written into its
/// request), begun (recorded from `from`, the id of the record there),
/// then done (see Finished).
#[derive(Default)]
pub struct Call {
    pub id: u32,
    pub tid: u32,
    /// Its runner's task (its task_struct): as the recording names it.
    pub task: u32,
    pub nr: u32,
    pub from: u32,
    pub begun: bool,
    pub done: bool,
    /// The program's line that made it, and its result's name and type, if
    /// declared (`int fd = openat(…)`).
    pub line: usize,
    pub result: Option<(String, String)>,
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

/// The task running now (the kernel's __current_task).
pub fn current() -> u32 {
    Direct.read_u32(k(|k| k.current.get()))
}

/// Whether the probes record now (the trace's `on` word, at either level:
/// see programs::on_off).
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
