//! The kernel's imports (arch/wasm/include/asm/wasm.h).
//!
//! Frequent ones (time, the console, the disk; and sched.rs's spawn_task) are exports of this module, so the
//! kernel calls them directly. The rest need JS objects (promises,
//! programs' modules) and are closures reached through wasm-bindgen: the
//! tasks' (sched.rs's switch_task, idle and restart_task, and halt here,
//! suspend the kernel's stack) and the programs' (programs.rs).

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
    state_mut(|s| s.console_out.extend_from_slice(unsafe { core::slice::from_raw_parts(p, n as usize) }));
}

/// console_read: the console's input for the task running, its runner's
/// next call (its number, then its arguments), written into the reader's
/// buffer at `buf` (a program's memory is kernel memory: recorded, as the
/// host's writes are): how many bytes, at most `len`; 0, none yet.
#[unsafe(no_mangle)]
pub extern "C" fn console_read(buf: u32, len: u32) -> u32 {
    let tid = crate::programs::tid();
    let Some(words) = state_mut(|s| s.session.take(tid)) else { return 0 };
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).take(len as usize).collect();
    crate::machine::write_logged(buf, &bytes);
    bytes.len() as u32
}

/// The kernel's clock (ns): virtual (see State::clock), a tick on at
/// each read.
#[unsafe(no_mangle)]
pub extern "C" fn time_ns() -> u64 {
    state_mut(|s| {
        s.clock += TICK_NS;
        s.clock
    })
}

// ---- The disk (arch/wasm/drivers/disk.c) ------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn disk_submit(tag: u32, sector: u64, buf: u32, len: u32, write: u32) {
    state_mut(|s| {
        let (asked_word, task) = (Direct.read_u32(s.trace), Direct.read_u32(s.current));
        s.transfers.push(Transfer { task, tag, sector, buf, len, write: write != 0, asked_word, ..Default::default() });
    });
}

/// The next request done, its tag; or -1.
#[unsafe(no_mangle)]
pub extern "C" fn disk_done() -> i32 {
    state_mut(|s| {
        let Some(tag) = s.done_tags.pop_front() else { return -1 };
        // (Its transfers done now, as the kernel sees them: the trace's word.)
        let at = Direct.read_u32(s.trace);
        for t in s.disk_log.iter_mut().filter(|t| t.tag == tag && t.done_word == 0) {
            t.done_word = at;
        }
        tag as i32
    })
}

/// The disk at work, as the CPU idles: the transfers handed to it made (a
/// read written into kernel memory as the host's writes are, logged), and
/// their requests' tags posted, done; whether there were any (then
/// WASM_IRQ_DISK is due).
pub fn disk_work() -> bool {
    let transfers = state_mut(|s| std::mem::take(&mut s.transfers));
    let any = !transfers.is_empty();
    for mut t in transfers {
        let at = t.sector as usize * 512;
        state_mut(|s| {
            // (A request's segments come together: its tag once.)
            if s.done_tags.back() != Some(&t.tag) {
                s.done_tags.push_back(t.tag);
            }
            // (Past the disk's end: the request ends all the same, its bytes not moved.)
            let Some(blocks) = s.disk.get_mut(at..at + t.len as usize) else { return };
            if t.write {
                t.before = blocks.to_vec();
                Direct.read(t.buf, blocks);
            } else {
                logged(t.buf, t.len);
                unsafe { core::ptr::copy_nonoverlapping(blocks.as_ptr(), t.buf as *mut u8, t.len as usize) };
            }
            t.after = blocks.to_vec();
            // (Logged if recorded: the trace's word 0 until the recording
            // begins, the boot's transfers left out.)
            if t.asked_word != 0 {
                s.disk_log.push(t);
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

/// What the kernel records (see programs.rs's record): 1, everything
/// (a Task record first, as it begins); 2, stores only.
pub fn record(level: u32) {
    WRITERS.with_borrow(|w| if let Some((_, record)) = w {
        let _ = record.call1(&JsValue::NULL, &level.into());
    });
}

/// The import object for the kernel (and the programs' syscall import,
/// once it is instantiated: see programs.rs's instantiated).
pub fn kernel() -> Object {
    let rt = wasm_bindgen::exports();
    let export = |name: &str| js::get(&rt, name);

    let kernel = Object::new();
    for name in ["console_write", "console_read", "time_ns", "spawn_task", "disk_submit", "disk_done"] {
        js::set(&kernel, name, export(name));
    }
    js::set(&kernel, "switch_task", sched::suspending(Closure::<dyn FnMut(u32, u32, u32) -> Promise>::new(sched::switch_task).into_js_value()));
    js::set(&kernel, "idle", sched::suspending(Closure::<dyn FnMut() -> Promise>::new(sched::idle).into_js_value()));
    js::set(&kernel, "restart_task", sched::suspending(Closure::<dyn FnMut(u32, u32) -> Promise>::new(sched::restart_task).into_js_value()));
    js::set(&kernel, "halt", sched::suspending(Closure::<dyn FnMut(i32) -> Promise>::new(|code: i32| {
        state_mut(|s| s.halted = Some(code));
        crate::machine::settle(Some(format!("the kernel halted ({code})")));
        sched::never()
    }).into_js_value()));
    js::set(&kernel, "syscall_enter", Closure::<dyn FnMut()>::new(crate::programs::syscall_enter).into_js_value());
    js::set(&kernel, "syscall_exit", Closure::<dyn FnMut(i32)>::new(crate::programs::syscall_exit).into_js_value());
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

