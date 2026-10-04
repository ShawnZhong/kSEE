//! The kernel's imports (arch/wasm/include/asm/wasm.h).
//!
//! Frequent ones (time, console, command line, spawn_task, the doorbell,
//! the disk) are this module's exports, so the kernel calls them
//! directly. The rest need JS objects (promises, programs' modules) and are
//! closures reached through wasm-bindgen: the tasks' (sched.rs: switch_task,
//! idle, restart_task and halt suspend the kernel's stack) and the
//! programs' (programs.rs).

use std::cell::RefCell;

use js_sys::{Function, Object, Promise};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use ksee_core::Memory;

use crate::js;
use crate::programs::{clone_user, user_load, user_start};
use crate::sched;
use crate::state::*;

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

// ---- The disk (the kernel's disk.c) ------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn disk_sectors() -> u64 {
    k(|k| k.disk.len() as u64 / 512)
}

#[unsafe(no_mangle)]
pub extern "C" fn disk_submit(tag: u32, sector: u64, buf: u32, len: u32, write: u32) {
    k_mut(|k| k.transfers.push((tag, sector, buf, len, write != 0)));
}

/// The next request done, its tag; or -1.
#[unsafe(no_mangle)]
pub extern "C" fn disk_done() -> i32 {
    k_mut(|k| k.done.pop_front().map_or(-1, |t| t as i32))
}

/// The disk at work, as the CPU idles: the transfers handed to it made (a
/// read written into kernel memory as the host's writes are, logged), and
/// their requests' tags posted, done; whether there were any (then
/// WASM_IRQ_DISK is due).
pub fn disk_work() -> bool {
    let transfers = k_mut(|k| std::mem::take(&mut k.transfers));
    let any = !transfers.is_empty();
    for (tag, sector, buf, len, write) in transfers {
        let at = sector as usize * 512;
        k_mut(|k| {
            let Some(blocks) = k.disk.get_mut(at..at + len as usize) else { return };
            if write {
                Direct.read(buf, blocks);
            } else {
                logged(buf, len);
                unsafe { core::ptr::copy_nonoverlapping(blocks.as_ptr(), buf as *mut u8, len as usize) };
            }
            // (A request's segments come together: its tag once.)
            if k.done.back() != Some(&tag) {
                k.done.push_back(tag);
            }
        });
    }
    any
}

thread_local! {
    /// The kernel's trace exports (arch/wasm/kernel/ksee.c): ksee_bulk and
    /// ksee_record.
    static WRITERS: RefCell<Option<(Function, Function)>> = const { RefCell::new(None) };
}

/// The kernel instance's exports: its trace writers.
pub fn instantiated(kernel: &Object) {
    WRITERS.set(Some((js::get(kernel, "ksee_bulk").unchecked_into(), js::get(kernel, "ksee_record").unchecked_into())));
}

/// What the host writes into kernel memory (a variable, a call's request, a
/// runner's word), logged before it is written: Store records with the old
/// contents of `addr..addr + len`, while recording, as the kernel logs its
/// own bulk writes.
pub fn logged(addr: u32, len: u32) {
    WRITERS.with_borrow(|w| if let Some((bulk, _)) = w {
        let _ = bulk.call3(&JsValue::NULL, &addr.into(), &len.into(), &0.into());
    });
}

/// What the kernel records: 1, everything (a Task record first, as it
/// begins); 2, stores only.
pub fn record(level: u32) {
    WRITERS.with_borrow(|w| if let Some((_, record)) = w {
        let _ = record.call1(&JsValue::NULL, &level.into());
    });
}

/// The import object for the kernel (and the programs' syscall import,
/// once it is instantiated: see instantiated).
pub fn kernel() -> Object {
    let rt = wasm_bindgen::exports();
    let export = |name: &str| js::get(&rt, name);

    let kernel = Object::new();
    for name in ["console_write", "doorbell", "get_cmdline", "memory_end", "time_ns", "spawn_task",
                 "disk_sectors", "disk_submit", "disk_done"] {
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

    let env = Object::new();
    js::set(&env, "memory", wasm_bindgen::memory());
    let imports = Object::new();
    js::set(&imports, "env", env);
    js::set(&imports, "kernel", kernel);
    imports
}

