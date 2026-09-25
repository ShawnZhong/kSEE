//! Tasks, on one thread. Each is a call stack of the one kernel instance,
//! which JSPI (JavaScript Promise Integration) suspends when the task gives
//! the CPU away and resumes when it gets it back; only one runs at a time.
//!
//! The kernel's switch_task(prev, next) is a suspending import: it returns
//! a promise, so prev's stack suspends; then next runs, resumed (its own
//! promise resolved) or, the first time, started (task_entry, promising).
//! The idle task's idle() suspends until an interrupt may be pending. The
//! kernel's clock is virtual: while the page waits for the kernel (a boot,
//! a call), idle skips it to the timer's deadline and goes on at once;
//! else it waits for the page's input (see wake), and time stands still. So
//! a run is the same every time. All
//! tasks share the instance's __stack_pointer: each saves it when it
//! suspends, and has it back before it resumes.

use std::cell::RefCell;
use std::collections::BTreeMap;

use js_sys::{Function, Object, Promise, Reflect, WebAssembly};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::js;
use crate::state::*;
use ksee_core::Memory;

/// A task not running: its stack pointer, and what resumes it (None: it
/// has not started).
struct Task {
    sp: u32,
    resume: Option<Function>,
}

#[derive(Default)]
struct Sched {
    tasks: BTreeMap<u32, Task>,
    /// The kernel's exports: task_entry (promising), boot, and the stack pointer.
    task_entry: Option<Function>,
    sp: Option<WebAssembly::Global>,
    /// What wakes the idle task (it is idle, for the page's input).
    idle: Option<Function>,
    /// The boot (counted): what an earlier one left to do is not done.
    boot: u32,
    /// Whether the machine runs (between boot and stop). A stack of a
    /// stopped machine that suspends is never resumed.
    running: bool,
}

thread_local! {
    static S: RefCell<Sched> = RefCell::default();
}

fn webassembly(name: &str) -> JsValue {
    js::get(&js::get(&js_sys::global(), "WebAssembly"), name)
}

/// `f` as an import the kernel's stack suspends on (it returns a promise).
pub fn suspending(f: JsValue) -> JsValue {
    let ctor: Function = webassembly("Suspending").unchecked_into();
    Reflect::construct(&ctor, &js_sys::Array::of1(&f)).expect("JSPI: WebAssembly.Suspending")
}

/// Export `f` as a function that runs on a stack of its own (JSPI), until it
/// suspends: it returns a promise.
fn promising(f: &JsValue) -> Function {
    let p: Function = webassembly("promising").unchecked_into();
    p.call1(&JsValue::NULL, f).expect("JSPI: WebAssembly.promising").unchecked_into()
}

fn sp_get() -> u32 {
    S.with_borrow(|s| s.sp.as_ref().and_then(|g| g.value().as_f64()).unwrap_or(0.0) as u32)
}

fn sp_set(sp: u32) {
    S.with_borrow(|s| {
        if let Some(g) = &s.sp {
            g.set_value(&sp.into());
        }
    });
}

/// Run `f` soon (a microtask: once the running stack has suspended), if
/// the machine has not started again meanwhile.
fn soon(f: impl FnOnce() + 'static) {
    let boot = S.with_borrow(|s| s.boot);
    let p = Promise::resolve(&JsValue::NULL);
    let then: Function = js::get(&p, "then").unchecked_into();
    let _ = then.call1(&p, &Closure::once_into_js(move |_: JsValue| {
        if S.with_borrow(|s| s.boot) == boot {
            f();
        }
    }));
}

/// Report a task's stack ending with an error (not a halt's).
fn on_error(p: &JsValue) {
    let catch: Function = js::get(p, "catch").unchecked_into();
    let _ = catch.call1(p, &Closure::once_into_js(|e: JsValue| {
        if k(|k| k.halted.get()).is_none() {
            web_sys::console::error_2(&"kernel:".into(), &js::get(&e, "stack"));
        }
    }));
}

/// A promise that never settles: a stack suspended on it is dropped.
pub fn never() -> Promise {
    Promise::new(&mut |_, _| {})
}

/// The kernel instance's exports, and its boot: the boot CPU's stack.
pub fn boot(kernel: &Object) -> Result<(), JsValue> {
    let sp: WebAssembly::Global = js::get(kernel, "__stack_pointer").unchecked_into();
    S.with_borrow_mut(|s| {
        *s = Sched { task_entry: Some(promising(&js::get(kernel, "task_entry"))), sp: Some(sp), boot: s.boot + 1,
                     running: true, ..Default::default() };
    });
    let boot_stack: Function = js::get(kernel, "boot_stack").unchecked_into();
    sp_set(boot_stack.call0(&JsValue::NULL)?.as_f64().unwrap_or(0.0) as u32);
    on_error(&promising(&js::get(kernel, "boot")).call0(&JsValue::NULL)?);
    Ok(())
}

/// Stop the machine: forget every task (their stacks are dropped), and
/// what it left to do (see soon).
pub fn stop() {
    S.with_borrow_mut(|s| {
        s.tasks.clear();
        s.idle = None;
        s.boot += 1;
        s.running = false;
    });
}

/// spawn_task: a new task, `task_entry(task)` on the stack at `sp` when it
/// first gets the CPU.
#[unsafe(no_mangle)]
pub extern "C" fn spawn_task(task: u32, sp: u32) {
    S.with_borrow_mut(|s| s.running.then(|| s.tasks.insert(task, Task { sp, resume: None })));
}

fn running() -> bool {
    S.with_borrow(|s| s.running)
}

/// switch_task (suspending): `prev` gives the CPU to `next`; prev resumes
/// when it gets it back (never, if `dead`).
pub fn switch_task(prev: u32, next: u32, dead: u32) -> Promise {
    if !running() {
        return never();
    }
    // (A busy machine gets its timer interrupts too: at its switches.)
    if deadline() <= k(|k| k.clock.get()) {
        raise(IRQ_TIMER);
    }
    let sp = sp_get();
    let back = if dead != 0 {
        S.with_borrow_mut(|s| s.tasks.remove(&prev));
        never()
    } else {
        let mut resume = None;
        let p = Promise::new(&mut |ok, _| resume = Some(ok));
        S.with_borrow_mut(|s| s.tasks.insert(prev, Task { sp, resume }));
        // (Its stack pointer back, before its stack resumes.)
        let then: Function = js::get(&p, "then").unchecked_into();
        then.call1(&p, &Closure::once_into_js(move |v: JsValue| {
            sp_set(sp);
            v
        })).map(|v| v.unchecked_into()).unwrap_or(p)
    };
    soon(move || run(next));
    back
}

/// Give the CPU to `task`: resume it, or start it.
fn run(task: u32) {
    let (resume, start) = S.with_borrow_mut(|s| {
        let t = s.tasks.get_mut(&task);
        match t {
            Some(t) if t.resume.is_some() => (t.resume.take(), None),
            Some(t) => (None, Some((t.sp, s.task_entry.clone()))),
            None => (None, None),
        }
    });
    if let Some(r) = resume {
        let _ = r.call0(&JsValue::NULL);
    } else if let Some((sp, Some(entry))) = start {
        sp_set(sp);
        match entry.call1(&JsValue::NULL, &task.into()) {
            Ok(p) => on_error(&p),
            Err(e) => web_sys::console::error_2(&"kernel:".into(), &e),
        }
    } else {
        web_sys::console::error_1(&format!("kernel: no task {task:#x} to switch to").into());
    }
}

/// Set interrupt line `irq` pending (the kernel takes it when it next
/// enables interrupts).
fn raise(irq: u32) {
    let p = k(|k| k.pending.get());
    Direct.write_u32(p, Direct.read_u32(p) | 1 << irq);
}

/// How far idle skips while the page waits: a virtual minute (a kernel
/// that never gets done waits for the page instead).
const SKIP_NS: u64 = 60_000_000_000;

/// idle (suspending): until an interrupt may be pending.
pub fn idle() -> Promise {
    if !running() {
        return never();
    }
    if Direct.read_u32(k(|k| k.pending.get())) != 0 {
        return Promise::resolve(&JsValue::NULL);
    }
    // The page waits: on to the timer's deadline, now.
    let d = deadline();
    let skip = k(|k| match k.waiting.get() {
        Some(since) if d != u64::MAX && d.saturating_sub(since) < SKIP_NS => {
            k.clock.set(k.clock.get().max(d));
            true
        }
        _ => false,
    });
    if skip {
        raise(IRQ_TIMER);
        return Promise::resolve(&JsValue::NULL);
    }
    // Nothing to do until the page's input: a call's recording ends here
    // (and memory stays as it leaves it, until the next call).
    Direct.write_u32(k(|k| k.trace.get()) + ksee_core::trace::ON, 0);
    let mut wake = None;
    let p = Promise::new(&mut |ok, _| wake = Some(ok));
    S.with_borrow_mut(|s| s.idle = wake);
    p
}

/// Raise interrupt line `irq` (the page's), and wake the CPU if it idles.
pub fn interrupt(irq: u32) {
    raise(irq);
    wake();
}

/// Wake the CPU if it idles (an interrupt is pending).
fn wake() {
    if let Some(w) = S.with_borrow_mut(|s| s.idle.take()) {
        let _ = w.call0(&JsValue::NULL);
    }
}
