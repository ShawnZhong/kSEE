//! What a Worker runs, once web/worker.js has initialized this module in
//! it: the timer, or kernel tasks (the boot CPU, or new tasks, one after
//! another).

use std::sync::atomic::{AtomicU64, Ordering};

use js_sys::{Function, WebAssembly};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::imports::{self, EXIT, HALT, RESTART};
use crate::js;
use crate::state::{self, interrupt, DEADLINE, HALTED, IRQ_TIMER};

/// Called with the page's message (see machine.rs, `spawn`).
#[wasm_bindgen]
pub fn worker_entry(msg: JsValue) {
    state::MY_GEN.set(state::GEN.load(Ordering::Acquire));
    match js::get(&msg, "role").as_string().as_deref() {
        Some("timer") => {
            timer();
            js_sys::global().unchecked_into::<web_sys::DedicatedWorkerGlobalScope>().close();
        }
        Some(role) => tasks(&msg, role == "boot"),
        None => panic!("worker without a role"),
    }
}

/// The timer thread: turns the kernel's clockevent deadline (u64 ns,
/// u64::MAX if none) into the timer interrupt; until it is stale.
fn timer() {
    let deadline = DEADLINE.load(Ordering::Relaxed);
    let d = unsafe { &*(deadline as *const AtomicU64) };
    while !state::stale() {
        let when = d.load(Ordering::SeqCst);
        let now = imports::time_ns();
        if when == u64::MAX || when > now {
            let timeout = if when == u64::MAX { -1 } else { (when - now) as i64 };
            // (Not for ever: to see if it is stale.)
            let timeout = if timeout < 0 { 1_000_000_000 } else { timeout.min(1_000_000_000) };
            unsafe { core::arch::wasm32::memory_atomic_wait64(deadline as *mut i64, when as i64, timeout) };
            continue;
        }
        if d.compare_exchange(when, u64::MAX, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            interrupt(IRQ_TIMER);
        }
    }
}

/// Run kernel tasks on this Worker: the boot CPU ("boot"), or, one after
/// another, the new tasks the kernel spawns (see state::SPAWN): when its
/// task exits, the Worker takes the next, with its kernel instance.
fn tasks(msg: &JsValue, boot: bool) {
    let scope: web_sys::DedicatedWorkerGlobalScope = js_sys::global().unchecked_into();
    // (If no Worker waits for a new task, the page starts one more: only it
    // can stop them all.)
    let more = {
        let scope = scope.clone();
        Closure::<dyn FnMut()>::new(move || {
            let _ = scope.post_message(&JsValue::from_str("more"));
        })
        .into_js_value()
        .unchecked_into::<Function>()
    };
    imports::MORE.with_borrow_mut(|m| *m = Some(more));
    let module: WebAssembly::Module = js::get(msg, "kernel").unchecked_into();
    let imports = imports::kernel(&wasm_bindgen::memory(), true);
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
    let mut first = boot;
    loop {
        let result = if first {
            first = false;
            call("boot_stack", &[]).and_then(|sp| {
                set_sp(sp);
                call("boot", &[])
            })
        } else {
            let Some((task, sp)) = state::SPAWN.take() else { break };
            imports::reset_user();
            set_sp(sp.into());
            call("task_entry", &[task.into()])
        };
        // (A task's end unwinds its stack: EXIT; the machine's, HALT.)
        if let Err(e) = result {
            let message = js_sys::Error::from(e.clone()).message();
            if HALTED.load(Ordering::Acquire) != 0 || message == HALT || message == RESTART || state::stale() {
                break;
            }
            if message != EXIT {
                web_sys::console::error_2(&"kernel worker:".into(), &e);
            }
        }
    }
    scope.close();
}
