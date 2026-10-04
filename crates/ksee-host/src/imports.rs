//! The kernel's imports (arch/wasm/include/asm/wasm.h).
//!
//! Frequent ones (time, console, command line, bulk stores, spawn_task,
//! the doorbell) are this module's exports, so the kernel calls them
//! directly. The rest need JS objects (promises, programs' modules) and are
//! closures reached through wasm-bindgen: the tasks' (sched.rs: switch_task,
//! idle, restart_task and halt suspend the kernel's stack) and the
//! programs' (programs.rs).

use core::num::NonZeroU32;

use js_sys::{Object, Promise};
use ksee_core::trace::wire::{self, EncodedRecord, StoreChunks};
use wasm_bindgen::prelude::*;

use crate::js;
use crate::programs::{clone_user, user_load, user_start};
use crate::sched;
use crate::state::*;
use ksee_core::Memory;

// ---- Exports the kernel imports directly -------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn console_write(p: *const u8, n: u32) {
    k_mut(|k| k.console_out.extend_from_slice(unsafe { core::slice::from_raw_parts(p, n as usize) }));
}

/// doorbell: the next task to ring, or 0.
#[unsafe(no_mangle)]
pub extern "C" fn doorbell() -> u32 {
    k_mut(|k| k.doorbell.pop_front().unwrap_or(0))
}

/// Copy the command line (NUL-terminated) to `buf`.
#[unsafe(no_mangle)]
pub extern "C" fn get_cmdline(buf: *mut u8, len: u32) -> u32 {
    k(|k| {
        let n = k.cmdline.len().min(len as usize - 1);
        unsafe {
            core::ptr::copy_nonoverlapping(k.cmdline.as_ptr(), buf, n);
            *buf.add(n) = 0;
        }
        n as u32
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn memory_end() -> u32 {
    RAM_BYTES
}

/// The kernel's clock (ns): virtual (see State::clock), a tick on at
/// each read.
#[unsafe(no_mangle)]
pub extern "C" fn time_ns() -> u64 {
    k_mut(|k| {
        k.clock += TICK_NS;
        k.clock
    })
}

/// ksee.bulk, for the kernel (memcpy, memset, wide stores): Store records
/// with the old contents of `addr..addr + len`, while recording; `from`:
/// where a copy (a memcpy) takes them from, else 0.
#[unsafe(no_mangle)]
pub extern "C" fn bulk(addr: u32, len: u32, line: u32, from: u32) {
    if recording() {
        log(addr, len, line, from);
    }
}

/// What the runtime itself writes into kernel memory (a call's request,
/// a runner's word): Store records with the old contents of `addr..addr +
/// len`, in 8-byte stores; `line`: the kernel's store's line (see the
/// plugin), 0 for the runtime's own; `from`: where the bytes are copied
/// from (each store then says where its own come from), else 0.
pub fn log(addr: u32, len: u32, line: u32, from: u32) {
    for (off, size) in StoreChunks::new(addr, len, from) {
        let mut old = [0u8; 8];
        Direct.read(addr + off, &mut old[..size.bytes() as usize]);
        let from = if from == 0 { None } else { NonZeroU32::new(from + off) };
        ring_put(wire::store(addr + off, line, size, u64::from_le_bytes(old), from));
    }
}

/// Append a complete record to the kernel's trace ring (called while the
/// kernel runs: no kernel code writes it meanwhile).
pub(crate) fn ring_put(record: EncodedRecord) {
    let (trace, size) = (k(|k| k.trace), tables().ring_words);
    if trace == 0 || size == 0 {
        return;
    }
    debug_assert!(size.is_power_of_two() && size >= 2);
    let words = record.words();
    let p = Direct.read_u32(trace);
    let ring = trace + ksee_core::trace::RING;
    for (i, &w) in words.iter().enumerate() {
        Direct.write_u32(ring + ((p + i as u32) & (size - 1)) * 4, w);
    }
    Direct.write_u32(trace, p + words.len() as u32);
}

/// The import object for the kernel (and the programs' syscall import,
/// once it is instantiated: see instantiated).
pub fn kernel() -> Object {
    let rt = wasm_bindgen::exports();
    let export = |name: &str| js::get(&rt, name);

    let kernel = Object::new();
    for name in ["console_write", "doorbell", "get_cmdline", "memory_end", "time_ns", "spawn_task"] {
        js::set(&kernel, name, export(name));
    }
    js::set(&kernel, "switch_task", sched::suspending(Closure::<dyn FnMut(u32, u32, u32) -> Promise>::new(sched::switch_task).into_js_value()));
    js::set(&kernel, "idle", sched::suspending(Closure::<dyn FnMut() -> Promise>::new(sched::idle).into_js_value()));
    js::set(&kernel, "restart_task", sched::suspending(Closure::<dyn FnMut(u32, u32) -> Promise>::new(sched::restart_task).into_js_value()));
    js::set(&kernel, "halt", sched::suspending(Closure::<dyn FnMut(i32) -> Promise>::new(|code: i32| {
        k_mut(|k| k.halted = Some(code));
        crate::machine::settle(Some(format!("the kernel halted ({code})")));
        sched::never()
    }).into_js_value()));
    js::set(&kernel, "print_stack", Closure::<dyn FnMut()>::new(|| {
        web_sys::console::log_1(&js::get(&js_sys::Error::new("kernel stack"), "stack"));
    }).into_js_value());
    js::set(&kernel, "user_load", Closure::<dyn FnMut(u32, u32) -> i32>::new(user_load).into_js_value());
    js::set(&kernel, "user_start", Closure::<dyn FnMut(u32, u32, u32) -> u32>::new(user_start).into_js_value());
    js::set(&kernel, "clone_user", Closure::<dyn FnMut(u32, u32)>::new(clone_user).into_js_value());

    let ksee = Object::new();
    js::set(&ksee, "bulk", export("bulk"));
    let env = Object::new();
    js::set(&env, "memory", wasm_bindgen::memory());
    let imports = Object::new();
    js::set(&imports, "env", env);
    js::set(&imports, "kernel", kernel);
    js::set(&imports, "ksee", ksee);
    imports
}

