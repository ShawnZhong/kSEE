//! The wasm interface: exports for the host and the kernel's probes.

use std::cell::RefCell;

use crate::event::{self, Recorder};
use crate::tables::Tables;

mod host {
    #[link(wasm_import_module = "host")]
    unsafe extern "C" {
        /// Copy `len` bytes of kernel memory at `addr` to `dst`.
        pub fn kmem_read(addr: u32, dst: *mut u8, len: u32);
        /// Nanoseconds since the machine started.
        pub fn now_ns() -> u64;
        /// Output one finished record (a JSON line, without the newline).
        pub fn emit(ptr: *const u8, len: u32);
    }
}

/// Kernel memory, as seen through the host.
pub struct KernelMemory;

impl event::Memory for KernelMemory {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        unsafe { host::kmem_read(addr, buf.as_mut_ptr(), buf.len() as u32) }
    }
}

thread_local! {
    static RECORDER: RefCell<Option<Recorder>> = const { RefCell::new(None) };
}

fn with_recorder(f: impl FnOnce(&mut Recorder)) {
    RECORDER.with_borrow_mut(|r| {
        if let Some(r) = r {
            f(r)
        }
    })
}

fn emit(line: &str) {
    unsafe { host::emit(line.as_ptr(), line.len() as u32) }
}

/// Allocate `len` bytes for the host to write into (never freed).
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: u32) -> *mut u8 {
    Box::leak(vec![0u8; len as usize].into_boxed_slice()).as_mut_ptr()
}

/// Initialize from the kernel module's custom sections and the address of
/// the kernel's `__current_task` variable.
#[unsafe(no_mangle)]
pub extern "C" fn init(probes: *const u8, probes_len: u32, types: *const u8,
                       types_len: u32, current_task: u32) {
    let text = |p, n| unsafe {
        String::from_utf8_lossy(std::slice::from_raw_parts(p, n as usize)).into_owned()
    };
    let tables = Tables::parse(&text(probes, probes_len), &text(types, types_len));
    RECORDER.set(Some(Recorder::new(tables, current_task)));
}

#[unsafe(no_mangle)]
pub extern "C" fn enter(func: u32, a0: u32, a1: u32, a2: u32, a3: u32) {
    let t = unsafe { host::now_ns() };
    with_recorder(|r| emit(&r.enter(&KernelMemory, t, func, [a0, a1, a2, a3])));
}

#[unsafe(no_mangle)]
pub extern "C" fn exit(func: u32, ret: u32) {
    let t = unsafe { host::now_ns() };
    with_recorder(|r| emit(&r.exit(&KernelMemory, t, func, ret)));
}

#[unsafe(no_mangle)]
pub extern "C" fn block(func: u32, n: u32) {
    let t = unsafe { host::now_ns() };
    with_recorder(|r| emit(&r.block(&KernelMemory, t, func, n)));
}
