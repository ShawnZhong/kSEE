//! The kernel's imports (arch/wasm/include/asm/wasm.h).
//!
//! Frequent ones (time, console, command line, bulk stores, spawn_task,
//! the doorbell) are this module's exports, so the kernel calls them
//! directly. The rest need JS objects (promises, programs' modules) and are
//! closures reached through wasm-bindgen: the tasks' (sched.rs: switch_task,
//! idle, restart_task and halt suspend the kernel's stack) and the
//! programs' (programs.rs).

use js_sys::{Object, Promise};
use wasm_bindgen::prelude::*;

use crate::js::{self, func};
use crate::programs::{clone_user, user_load, user_start};
use crate::sched;
use crate::state::*;
use ksee_core::Memory;

// ---- Exports the kernel imports directly -------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn console_write(p: *const u8, n: u32) {
    k(|k| k.console_out.borrow_mut().extend_from_slice(unsafe { core::slice::from_raw_parts(p, n as usize) }));
}

/// doorbell: the next task to ring, or 0.
#[unsafe(no_mangle)]
pub extern "C" fn doorbell() -> u32 {
    k(|k| k.doorbell.borrow_mut().pop_front().unwrap_or(0))
}

/// Copy the command line (NUL-terminated) to `buf`.
#[unsafe(no_mangle)]
pub extern "C" fn get_cmdline(buf: *mut u8, len: u32) -> u32 {
    k(|k| {
        let cmd = k.cmdline.borrow();
        let n = cmd.len().min(len as usize - 1);
        unsafe {
            core::ptr::copy_nonoverlapping(cmd.as_ptr(), buf, n);
            *buf.add(n) = 0;
        }
        n as u32
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn memory_end() -> u32 {
    k(|k| k.ram.get())
}

/// The kernel's clock (ns): virtual (see Kernel::clock), a tick on at
/// each read.
#[unsafe(no_mangle)]
pub extern "C" fn time_ns() -> u64 {
    k(|k| {
        let c = k.clock.get() + TICK_NS;
        k.clock.set(c);
        c
    })
}

/// ksee.bulk, for the kernel (memcpy, memset, wide stores): Store records
/// with the old contents of `addr..addr + len`, while recording.
#[unsafe(no_mangle)]
pub extern "C" fn bulk(addr: u32, len: u32) {
    if recording() {
        log(addr, len);
    }
}

/// What the runtime itself writes into kernel memory (a call's request,
/// a runner's word): Store records with the old contents of `addr..addr +
/// len`, in 8-byte stores, up to 256 bytes (the rest as a range of
/// unknown old contents).
pub fn log(addr: u32, len: u32) {
    use ksee_core::trace::{STORE, UNKNOWN, VALUE};
    if len == 0 {
        return;
    }
    let logged = len.min(256);
    let mut off = 0;
    while off < logged {
        let class = match logged - off { 8.. => 3, 4..=7 => 2, 2..=3 => 1, _ => 0 };
        let size = 1u32 << class;
        let mut old = [0u8; 8];
        Direct.read(addr + off, &mut old[..size as usize]);
        let v = u64::from_le_bytes(old);
        if class == 3 {
            ring_put(&[addr + off, STORE << 28 | class, v as u32, VALUE << 28, (v >> 32) as u32, VALUE << 28 | 1]);
        } else {
            ring_put(&[addr + off, STORE << 28 | class, v as u32, VALUE << 28]);
        }
        off += size;
    }
    if len > logged {
        ring_put(&[addr + logged, STORE << 28 | UNKNOWN, len - logged, VALUE << 28]);
    }
}

/// A Task record: `task` (a task_struct) runs; where a recording begins.
pub fn task(task: u32) {
    ring_put(&[task, ksee_core::trace::TASK << 28]);
}

/// Append a record (pairs of words) to the kernel's trace ring (called
/// while the kernel runs: no kernel code writes it meanwhile).
fn ring_put(words: &[u32]) {
    let (trace, size) = (k(|k| k.trace.get()), tables().ring_words);
    if trace == 0 || size == 0 {
        return;
    }
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
    js::set(&kernel, "switch_task", sched::suspending(func(Closure::<dyn FnMut(u32, u32, u32) -> Promise>::new(sched::switch_task))));
    js::set(&kernel, "idle", sched::suspending(func(Closure::<dyn FnMut() -> Promise>::new(sched::idle))));
    js::set(&kernel, "restart_task", sched::suspending(func(Closure::<dyn FnMut(u32, u32) -> Promise>::new(sched::restart_task))));
    js::set(&kernel, "halt", sched::suspending(func(Closure::<dyn FnMut(i32) -> Promise>::new(|code: i32| {
        k(|k| k.halted.set(Some(code)));
        for (_, fail) in k(|k| k.quiet_waiters.take()) {
            let _ = fail.call1(&JsValue::NULL, &format!("the kernel halted ({code})").into());
        }
        sched::never()
    }))));
    js::set(&kernel, "print_stack", func(Closure::<dyn FnMut()>::new(|| {
        web_sys::console::log_1(&js::get(&js_sys::Error::new("kernel stack"), "stack"));
    })));
    js::set(&kernel, "user_load", func(Closure::<dyn FnMut(u32, u32) -> i32>::new(user_load)));
    js::set(&kernel, "user_start", func(Closure::<dyn FnMut(u32, u32, u32) -> u32>::new(user_start)));
    js::set(&kernel, "clone_user", func(Closure::<dyn FnMut(u32, u32)>::new(clone_user)));

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

