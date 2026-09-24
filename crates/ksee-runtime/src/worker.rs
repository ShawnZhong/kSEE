//! What a Worker runs, once web/worker.js has initialized this module in
//! it: the timer, or one kernel task.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use js_sys::{Function, Object, WebAssembly};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::imports::{self, EXIT, HALT};
use crate::js;
use crate::state::HALTED;

/// Called with the page's message (see machine.rs, `spawn`).
#[wasm_bindgen]
pub fn worker_entry(msg: JsValue) {
    let num = |k: &str| js::get(&msg, k).as_f64().unwrap_or(0.0) as u32;
    match js::get(&msg, "role").as_string().as_deref() {
        Some("timer") => timer(num("deadline"), num("pending")),
        Some(role) => task(&msg, role == "boot", num("task"), num("sp")),
        None => panic!("worker without a role"),
    }
}

/// The timer thread: turns the kernel's clockevent deadline (u64 ns,
/// u64::MAX if none) into the timer interrupt (bit 0 of the pending word).
fn timer(deadline: u32, pending: u32) -> ! {
    use core::arch::wasm32::{memory_atomic_notify, memory_atomic_wait64};

    let d = unsafe { &*(deadline as *const AtomicU64) };
    let p = unsafe { &*(pending as *const AtomicU32) };
    loop {
        let when = d.load(Ordering::SeqCst);
        let now = imports::time_ns();
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

/// Run a kernel task (or, for "boot", the boot CPU) on this Worker.
fn task(msg: &JsValue, boot: bool, task: u32, sp: u32) {
    let scope: web_sys::DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
    // New tasks' Workers are created by the page, which can stop them all.
    let spawn = {
        let scope = scope.clone();
        Closure::<dyn FnMut(u32, u32)>::new(move |task: u32, sp: u32| {
            let m = Object::new();
            js::set(&m, "task", task);
            js::set(&m, "sp", sp);
            let _ = scope.post_message(&m);
        })
        .into_js_value()
        .unchecked_into::<Function>()
    };
    let module: WebAssembly::Module = js::get(msg, "kernel").unchecked_into();
    let imports = imports::kernel(&wasm_bindgen::memory(), spawn, true);
    let kernel = WebAssembly::Instance::new(&module, &imports).expect("kernel instance").exports();
    imports::set_kernel(&kernel);

    let call = |name: &str, args: &[JsValue]| -> Result<JsValue, JsValue> {
        let f: Function = js::get(&kernel, name).unchecked_into();
        f.apply(&JsValue::NULL, &args.iter().collect())
    };
    let set_sp = |v: JsValue| {
        let g: WebAssembly::Global = js::get(&kernel, "__stack_pointer").unchecked_into();
        g.set_value(&v);
    };
    let result = if boot {
        call("boot_stack", &[]).and_then(|sp| {
            set_sp(sp);
            call("boot", &[])
        })
    } else {
        set_sp(sp.into());
        call("task_entry", &[task.into()])
    };
    if let Err(e) = result {
        let quiet = js_sys::Error::from(e.clone()).message();
        let quiet = quiet == EXIT || quiet == HALT || HALTED.load(Ordering::Acquire) != 0;
        if !quiet {
            web_sys::console::error_2(&"kernel worker:".into(), &e);
        }
    }
    scope.close();
}
