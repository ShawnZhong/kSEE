//! kSEE host runtime.
//!
//! This wasm module imports the kernel's shared memory and lives below the
//! kernel image (CONFIG_WASM_HOST_MEMORY), so it reads kernel objects and
//! the trace ring directly, and its statics are shared by every Worker
//! that instantiates it (only the stack pointer is per instance).
//!
//! Every kernel Worker instantiates it next to the kernel, and the
//! kernel's frequent imports (time, console, command line, halt) are its
//! exports: direct wasm calls. The page's instance serves the UI: console
//! I/O, halt status and text views of the trace. The JS shim (web/ksee.js)
//! does only what needs the platform: Workers, instantiating modules, user
//! memory.
#![no_std]
#![feature(stdarch_wasm_atomic_wait)]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use ksee_core::tables::Tables;
use ksee_core::render::Renderer;
use ksee_core::{cfg, dump, Memory};
use linked_list_allocator::LockedHeap;

mod platform {
    #[link(wasm_import_module = "platform")]
    unsafe extern "C" {
        /// Wall-clock milliseconds (performance.timeOrigin + now()), the
        /// same in every Worker.
        pub fn now_ms() -> f64;
        /// Write bytes to the named output (see [`super::DUMP`]).
        pub fn write(output: u32, ptr: *const u8, len: u32);
        /// Report a panic and stop.
        pub fn panic(ptr: *const u8, len: u32) -> !;
    }
}

/// End of the host's part of memory (CONFIG_WASM_HOST_MEMORY).
const HOST_END: usize = 16 << 20;
const STACK_SIZE: usize = 16 << 10;
const STACKS: usize = 256;

/// Outputs: the trace dump (see ksee_core::dump), and text views.
pub const DUMP: u32 = 0;
pub const TEXT: u32 = 1;

unsafe extern "C" {
    static __heap_base: u8;
}

/// The heap is the rest of the host's part of memory, set up on first use.
struct Heap {
    heap: LockedHeap,
    ready: AtomicU32,
}

#[global_allocator]
static HEAP: Heap = Heap { heap: LockedHeap::empty(), ready: AtomicU32::new(0) };

impl Heap {
    fn get(&self) -> &LockedHeap {
        if self.ready.load(Ordering::Acquire) != 2 {
            if self.ready.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                unsafe {
                    let base = core::ptr::addr_of!(__heap_base) as usize;
                    self.heap.lock().init(base as *mut u8, HOST_END - base);
                }
                self.ready.store(2, Ordering::Release);
            }
            while self.ready.load(Ordering::Acquire) != 2 {
                core::hint::spin_loop();
            }
        }
        &self.heap
    }
}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { self.get().alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { self.get().dealloc(ptr, layout) }
    }
}

/// Formats panic messages without allocating (the heap may be the problem).
struct PanicBuf([u8; 512], usize);

impl core::fmt::Write for PanicBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let n = s.len().min(self.0.len() - self.1);
        self.0[self.1..self.1 + n].copy_from_slice(&s.as_bytes()[..n]);
        self.1 += n;
        Ok(())
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut buf = PanicBuf([0; 512], 0);
    let _ = core::fmt::write(&mut buf, format_args!("{info}"));
    unsafe { platform::panic(buf.0.as_ptr(), buf.1 as u32) }
}

/// The kernel's memory is this module's memory.
struct Direct;

impl Memory for Direct {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        // The kernel is stopped or elsewhere; racing it only garbles data.
        unsafe { core::ptr::copy_nonoverlapping(addr as *const u8, buf.as_mut_ptr(), buf.len()) }
    }
}

// Stacks for instances other than the page's, which uses the linker's:
// one per kernel Worker (the kernel calls in with shallow calls) and the
// timer's. Slots are claimed and released in a bitmap.
#[repr(align(16))]
struct Stacks([[u8; STACK_SIZE]; STACKS]);
static mut STACK_AREA: Stacks = Stacks([[0; STACK_SIZE]; STACKS]);
static STACK_USED: [AtomicU32; STACKS / 32] = [const { AtomicU32::new(0) }; STACKS / 32];

/// A stack for a new instance: the shim sets __stack_pointer to it before
/// calling anything else, so this function must use no stack memory.
/// Returns 0 if none is free.
#[unsafe(no_mangle)]
pub extern "C" fn stack() -> u32 {
    for (w, used) in STACK_USED.iter().enumerate() {
        let mut bits = used.load(Ordering::Relaxed);
        while bits != u32::MAX {
            let bit = (!bits).trailing_zeros();
            match used.compare_exchange(bits, bits | 1 << bit, Ordering::Acquire, Ordering::Relaxed) {
                Ok(_) => {
                    let slot = w * 32 + bit as usize;
                    return unsafe { core::ptr::addr_of!(STACK_AREA.0[slot]) as u32 } + STACK_SIZE as u32;
                }
                Err(now) => bits = now,
            }
        }
    }
    0
}

/// Release a stack returned by [`stack`] (its instance will not run again).
#[unsafe(no_mangle)]
pub extern "C" fn unstack(top: u32) {
    let base = core::ptr::addr_of!(STACK_AREA) as u32;
    let slot = ((top - STACK_SIZE as u32 - base) / STACK_SIZE as u32) as usize;
    STACK_USED[slot / 32].fetch_and(!(1 << (slot % 32)), Ordering::Release);
}

static mut TABLES: Option<Tables> = None;

fn tables() -> &'static Tables {
    unsafe { (*core::ptr::addr_of!(TABLES)).as_ref().expect("init() not called") }
}

/// Allocate `len` bytes for the shim to write into; see [`free`].
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: u32) -> *mut u8 {
    unsafe { HEAP.alloc(Layout::from_size_align_unchecked(len.max(1) as usize, 8)) }
}

#[unsafe(no_mangle)]
pub extern "C" fn free(ptr: *mut u8, len: u32) {
    unsafe { HEAP.dealloc(ptr, Layout::from_size_align_unchecked(len.max(1) as usize, 8)) }
}

/// Called once, on the first instance: load the kernel module's
/// ksee.probes and ksee.types custom sections (the shim may then free them).
#[unsafe(no_mangle)]
pub extern "C" fn init(probes: *const u8, probes_len: u32, types: *const u8, types_len: u32) {
    let text = |p, n| unsafe {
        core::str::from_utf8(core::slice::from_raw_parts(p, n as usize)).unwrap_or("")
    };
    let t = Tables::parse(text(probes, probes_len), text(types, types_len));
    unsafe { TABLES = Some(t) };
    EPOCH_MS.store(unsafe { platform::now_ms() }.to_bits(), Ordering::Relaxed);
}

// ---- The kernel's imports ------------------------------------------------

/// A byte ring written by one side and read by the other.
struct ByteRing {
    buf: [AtomicU32; 1 << 12], // 16 KiB, one byte per... see put/take
    head: AtomicU32,           // bytes ever written
    tail: AtomicU32,           // bytes ever read
}

impl ByteRing {
    const fn new() -> Self {
        ByteRing { buf: [const { AtomicU32::new(0) }; 1 << 12], head: AtomicU32::new(0), tail: AtomicU32::new(0) }
    }
    const CAP: u32 = (1 << 12) * 4;

    fn byte(&self, i: u32) -> (&AtomicU32, u32) {
        let i = i % Self::CAP;
        (&self.buf[(i / 4) as usize], (i % 4) * 8)
    }

    /// Append, dropping the oldest unread bytes if the reader is behind.
    fn put(&self, data: &[u8]) {
        let head = self.head.load(Ordering::Relaxed);
        for (i, &b) in data.iter().enumerate() {
            let (w, shift) = self.byte(head + i as u32);
            let old = w.load(Ordering::Relaxed);
            w.store(old & !(0xff << shift) | (b as u32) << shift, Ordering::Relaxed);
        }
        self.head.store(head + data.len() as u32, Ordering::Release);
    }

    fn take(&self, out: &mut [u8]) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let mut tail = self.tail.load(Ordering::Relaxed);
        tail = tail.max(head.saturating_sub(Self::CAP));
        let n = ((head - tail) as usize).min(out.len());
        for (i, b) in out[..n].iter_mut().enumerate() {
            let (w, shift) = self.byte(tail + i as u32);
            *b = (w.load(Ordering::Relaxed) >> shift) as u8;
        }
        self.tail.store(tail + n as u32, Ordering::Relaxed);
        n
    }
}

static CONSOLE_OUT: ByteRing = ByteRing::new();
static CONSOLE_IN: ByteRing = ByteRing::new();
static mut CMDLINE: Option<alloc::vec::Vec<u8>> = None;
/// 0 while running, else the halt code + 1.
static HALTED: AtomicU32 = AtomicU32::new(0);
static EPOCH_MS: AtomicU64 = AtomicU64::new(0);

unsafe fn bytes<'a>(p: *const u8, n: u32) -> &'a [u8] {
    unsafe { core::slice::from_raw_parts(p, n as usize) }
}

/// kernel.console_write
#[unsafe(no_mangle)]
pub extern "C" fn console_write(p: *const u8, n: u32) {
    CONSOLE_OUT.put(unsafe { bytes(p, n) });
}

/// kernel.console_read: pending input, without blocking.
#[unsafe(no_mangle)]
pub extern "C" fn console_read(p: *mut u8, n: u32) -> i32 {
    CONSOLE_IN.take(unsafe { core::slice::from_raw_parts_mut(p, n as usize) }) as i32
}

/// kernel.get_cmdline: copy the command line (NUL-terminated) to `buf`.
#[unsafe(no_mangle)]
pub extern "C" fn get_cmdline(buf: *mut u8, len: u32) -> u32 {
    let cmd = unsafe { (*core::ptr::addr_of!(CMDLINE)).as_deref().unwrap_or(&[]) };
    let n = cmd.len().min(len as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(cmd.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n as u32
}

/// kernel.time_ns and ksee.now: nanoseconds since the machine started.
#[unsafe(no_mangle)]
pub extern "C" fn time_ns() -> u64 {
    let ms = unsafe { platform::now_ms() } - f64::from_bits(EPOCH_MS.load(Ordering::Relaxed));
    (ms * 1e6) as u64
}

/// kernel.halt: record the code for the page, and stop this Worker.
#[unsafe(no_mangle)]
pub extern "C" fn halt(code: i32) -> ! {
    HALTED.store(code as u32 + 1, Ordering::Release);
    core::arch::wasm32::unreachable()
}

// ---- The page -----------------------------------------------------------

/// Set the kernel command line (before boot).
#[unsafe(no_mangle)]
pub extern "C" fn set_cmdline(p: *const u8, n: u32) {
    unsafe { CMDLINE = Some(bytes(p, n).to_vec()) };
}

/// Console output since the last call.
#[unsafe(no_mangle)]
pub extern "C" fn console_take(p: *mut u8, n: u32) -> u32 {
    CONSOLE_OUT.take(unsafe { core::slice::from_raw_parts_mut(p, n as usize) }) as u32
}

/// Console input for the kernel.
#[unsafe(no_mangle)]
pub extern "C" fn console_give(p: *const u8, n: u32) {
    CONSOLE_IN.put(unsafe { bytes(p, n) });
}

/// 0 while the machine runs, else the halt code + 1.
#[unsafe(no_mangle)]
pub extern "C" fn halted() -> u32 {
    HALTED.load(Ordering::Acquire)
}

/// The timer thread: turns the kernel's clockevent deadline (u64 ns,
/// u64::MAX if none) into the timer interrupt (bit 0 of the pending word).
#[unsafe(no_mangle)]
pub extern "C" fn timer(deadline: u32, pending: u32) -> ! {
    use core::arch::wasm32::{memory_atomic_notify, memory_atomic_wait64};

    let d = unsafe { &*(deadline as *const AtomicU64) };
    let p = unsafe { &*(pending as *const AtomicU32) };
    loop {
        let when = d.load(Ordering::SeqCst);
        let now = time_ns();
        if when == u64::MAX || when > now {
            let timeout = if when == u64::MAX { -1 } else { (when - now) as i64 };
            unsafe { memory_atomic_wait64(deadline as *mut i64, when as i64, timeout) };
            continue;
        }
        if d.compare_exchange(when, u64::MAX, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            p.fetch_or(1 << 0, Ordering::SeqCst);
            unsafe { memory_atomic_notify(pending as *mut i32, 1) };
        }
    }
}

/// Save the trace ring at `ring` (the kernel's ksee_trace()) and the tasks
/// it mentions to output [`DUMP`].
#[unsafe(no_mangle)]
pub extern "C" fn dump(ring: u32) {
    dump::write(tables(), &Direct, ring, &mut |b| unsafe {
        platform::write(DUMP, b.as_ptr(), b.len() as u32)
    });
}

// ---- Views of the trace, as text ----------------------------------------

fn text(s: &str) {
    unsafe { platform::write(TEXT, s.as_ptr(), s.len() as u32) }
}

fn live_task(p: u32) -> Option<dump::Task> {
    dump::read_task(tables(), &Direct, p)
}

fn syscalls_in(ring: u32) -> alloc::vec::Vec<ksee_core::syscalls::Syscall> {
    let t = tables();
    let entry = t.find(ksee_core::syscalls::ENTRY).unwrap_or(0);
    ksee_core::syscalls::split(entry, ksee_core::trace::ring(&Direct, ring, t.ring_words))
}

/// Write the syscalls in the ring at `ring`, one per line, to output TEXT.
#[unsafe(no_mangle)]
pub extern "C" fn view_syscalls(ring: u32) {
    let r = Renderer { tables: tables(), tasks: &live_task };
    for (i, s) in syscalls_in(ring).iter().enumerate() {
        text(&alloc::format!("#{i} {}\n", r.syscall_text(s, false)));
    }
}

/// Write syscall `n`'s blocks to output TEXT.
#[unsafe(no_mangle)]
pub extern "C" fn view_syscall(ring: u32, n: u32) {
    let r = Renderer { tables: tables(), tasks: &live_task };
    if let Some(s) = syscalls_in(ring).get(n as usize) {
        text(&r.syscall_text(s, true));
        text("\n");
    }
}

/// Write the control flow graph of the function named at `name` to output
/// TEXT, with counts from syscall `n` (or the whole ring if n = u32::MAX).
#[unsafe(no_mangle)]
pub extern "C" fn view_cfg(ring: u32, name: *const u8, len: u32, n: u32) {
    let t = tables();
    let name = core::str::from_utf8(unsafe { bytes(name, len) }).unwrap_or("");
    let ids: alloc::vec::Vec<u32> = t.functions.iter().filter(|(_, f)| f.name == name).map(|(&id, _)| id).collect();
    let keep = |f: u32| ids.contains(&f);
    let cov = if n == u32::MAX {
        cfg::coverage(t, ksee_core::trace::ring(&Direct, ring, t.ring_words), keep)
    } else {
        let blocks = syscalls_in(ring).into_iter().nth(n as usize).map(|s| s.blocks).unwrap_or_default();
        cfg::coverage(t, blocks.into_iter().map(|(func, n)| ksee_core::trace::Record::Block { func, n }), keep)
    };
    let r = Renderer { tables: t, tasks: &live_task };
    for id in ids {
        text(&r.cfg_text(id, &cov));
    }
}
