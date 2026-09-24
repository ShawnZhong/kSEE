//! The wasm runtime: entry points for the JS shim.
//!
//! This module imports the kernel's shared memory and lives below the
//! kernel image, so Rust statics are shared by every Worker that
//! instantiates it; only the stack pointer is per instance.

use core::alloc::GlobalAlloc;
use core::sync::atomic::{AtomicU32, Ordering};

use linked_list_allocator::LockedHeap;

use crate::json::Writer;
use crate::tables::Tables;
use crate::{syscalls, trace, Memory};

mod platform {
    #[link(wasm_import_module = "platform")]
    unsafe extern "C" {
        /// Monotonic nanoseconds since the machine started.
        pub fn now_ns() -> u64;
        /// Output one line (e.g. a JSON record) to the named stream.
        pub fn emit(stream: u32, ptr: *const u8, len: u32);
        /// Report a panic and stop.
        pub fn panic(ptr: *const u8, len: u32) -> !;
    }
}

/// End of the host's part of memory (CONFIG_WASM_HOST_MEMORY).
const HOST_END: usize = 16 << 20;
const STACK_SIZE: usize = 64 << 10;
const STACKS: usize = 8;

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

unsafe impl core::alloc::GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        unsafe { self.get().alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
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
        let src = addr as *const u8;
        for (i, b) in buf.iter_mut().enumerate() {
            *b = unsafe { core::ptr::read_volatile(src.add(i)) };
        }
    }
}

// Stacks for instances other than the first, which uses the linker's.
#[repr(align(16))]
struct Stacks([[u8; STACK_SIZE]; STACKS]);
static mut STACK_AREA: Stacks = Stacks([[0; STACK_SIZE]; STACKS]);
static NEXT_STACK: AtomicU32 = AtomicU32::new(0);

/// A stack for a new instance: the shim sets __stack_pointer to it before
/// calling anything else. (This function itself uses no stack memory.)
#[unsafe(no_mangle)]
pub extern "C" fn stack() -> u32 {
    let i = NEXT_STACK.fetch_add(1, Ordering::Relaxed) as usize;
    assert!(i < STACKS, "out of host stacks");
    unsafe { core::ptr::addr_of!(STACK_AREA.0[i]) as u32 + STACK_SIZE as u32 }
}

static mut TABLES: Option<Tables> = None;

fn tables() -> &'static Tables {
    unsafe { (*core::ptr::addr_of!(TABLES)).as_ref().expect("init() not called") }
}

/// Allocate `len` bytes for the shim to write into; see [`free`].
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: u32) -> *mut u8 {
    unsafe { HEAP.alloc(core::alloc::Layout::from_size_align_unchecked(len.max(1) as usize, 8)) }
}

#[unsafe(no_mangle)]
pub extern "C" fn free(ptr: *mut u8, len: u32) {
    unsafe { HEAP.dealloc(ptr, core::alloc::Layout::from_size_align_unchecked(len.max(1) as usize, 8)) }
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
}

/// The timer thread: turns the kernel's clockevent deadline (u64 ns,
/// u64::MAX if none) into the timer interrupt (bit 0 of the pending word).
#[unsafe(no_mangle)]
pub extern "C" fn timer(deadline: u32, pending: u32) -> ! {
    use core::arch::wasm32::{memory_atomic_notify, memory_atomic_wait64};
    use core::sync::atomic::AtomicU64;

    let d = unsafe { &*(deadline as *const AtomicU64) };
    let p = unsafe { &*(pending as *const AtomicU32) };
    loop {
        let when = d.load(Ordering::SeqCst);
        let now = unsafe { platform::now_ns() };
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

const EVENTS: u32 = 0;
const SYSCALLS: u32 = 1;

fn emit(stream: u32, line: &str) {
    unsafe { platform::emit(stream, line.as_ptr(), line.len() as u32) }
}

/// Decode the trace ring at `ring` (the kernel's ksee_trace()) and output
/// its records (stream 0) and the per-syscall view (stream 1).
#[unsafe(no_mangle)]
pub extern "C" fn dump(ring: u32) {
    let t = tables();
    let w = Writer { tables: t, mem: &Direct };
    for r in trace::ring(&Direct, ring, t.ring_words) {
        emit(EVENTS, &w.record(&r));
    }
    for s in syscalls::split(trace::ring(&Direct, ring, t.ring_words)) {
        emit(SYSCALLS, &w.syscall(&s));
    }
}
