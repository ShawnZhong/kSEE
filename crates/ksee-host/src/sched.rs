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
/// has not started; it starts in task_entry, or, `user`, user_entry).
struct Saved {
    sp: u32,
    resume: Option<Function>,
    user: bool,
}

#[derive(Default)]
struct Sched {
    tasks: BTreeMap<u32, Saved>,
    /// The kernel's exports: task_entry and user_entry (promising), and the
    /// stack pointer.
    task_entry: Option<Function>,
    user_entry: Option<Function>,
    sp: Option<WebAssembly::Global>,
    /// What wakes the idle task (it is idle, for the page's input).
    idle: Option<Function>,
    /// The boot (counted): what an earlier one left to do is not done.
    boot: u32,
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
        if k(|k| k.halted).is_none() {
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
        *s = Sched { task_entry: Some(promising(&js::get(kernel, "task_entry"))),
                     user_entry: Some(promising(&js::get(kernel, "user_entry"))), sp: Some(sp), boot: s.boot + 1,
                     ..Default::default() };
    });
    let boot_stack: Function = js::get(kernel, "boot_stack").unchecked_into();
    sp_set(boot_stack.call0(&JsValue::NULL)?.as_f64().unwrap_or(0.0) as u32);
    on_error(&promising(&js::get(kernel, "boot")).call0(&JsValue::NULL)?);
    Ok(())
}

/// Stop the machine: forget every task (their stacks are dropped), and
/// what it left to do (see soon).
pub fn stop() {
    S.with_borrow_mut(|s| *s = Sched { boot: s.boot + 1, ..Default::default() });
}

/// spawn_task: a new task, `task_entry(task)` on the stack at `sp` when it
/// first gets the CPU.
#[unsafe(no_mangle)]
pub extern "C" fn spawn_task(task: u32, sp: u32) {
    if running() {
        S.with_borrow_mut(|s| s.tasks.insert(task, Saved { sp, resume: None, user: false }));
    }
}

/// restart_task (suspending): the current task's stack is dropped, and it
/// starts again, in user_entry, on the stack at `sp` (after an execve).
pub fn restart_task(task: u32, sp: u32) -> Promise {
    if running() {
        S.with_borrow_mut(|s| s.tasks.insert(task, Saved { sp, resume: None, user: true }));
        soon(move || run(task));
    }
    never()
}

/// Whether the machine runs (between boot and stop). A stack of a
/// stopped machine that suspends is never resumed.
fn running() -> bool {
    S.with_borrow(|s| s.task_entry.is_some())
}

/// switch_task (suspending): `prev` gives the CPU to `next`; prev resumes
/// when it gets it back (never, if `dead`).
pub fn switch_task(prev: u32, next: u32, dead: u32) -> Promise {
    if !running() {
        return never();
    }
    // (A busy machine gets its timer interrupts too: at its switches.)
    if deadline() <= k(|k| k.clock) {
        raise(IRQ_TIMER);
    }
    let sp = S.with_borrow(|s| s.sp.as_ref().and_then(|g| g.value().as_f64()).unwrap_or(0.0) as u32);
    crate::programs::switch_user(prev, next, dead != 0);
    let back = if dead != 0 {
        S.with_borrow_mut(|s| s.tasks.remove(&prev));
        never()
    } else {
        let mut resume = None;
        let p = Promise::new(&mut |ok, _| resume = Some(ok));
        S.with_borrow_mut(|s| s.tasks.insert(prev, Saved { sp, resume, user: false }));
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
            Some(t) => (None, Some((t.sp, if t.user { s.user_entry.clone() } else { s.task_entry.clone() }))),
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
    let p = k(|k| k.pending);
    Direct.write_u32(p, Direct.read_u32(p) | 1 << irq);
}

/// How far idle skips while the page waits for the boot: what it leaves
/// for later runs then, unrecorded.
const BOOT_NS: u64 = 1_000_000_000;

/// Whether a runner the page waits for sleeps on a timer of its own (see
/// ksee_core::arch::sleeping).
fn sleeping() -> bool {
    let tasks = k(|k| k.p.waiting_tasks());
    ksee_core::arch::sleeping(&tables().types, &Direct, &tasks)
}

/// idle (suspending): until an interrupt may be pending.
pub fn idle() -> Promise {
    if !running() {
        return never();
    }
    if Direct.read_u32(k(|k| k.pending)) != 0 {
        return Promise::resolve(&JsValue::NULL);
    }
    // The page waits: on to the timer's deadline, now. For a call, if its
    // runner sleeps on a timer (else it blocks: it waits for another call).
    let d = deadline();
    let (waiting, booting) = k(|k| (k.waiting, k.booting));
    if let Some(since) = waiting && d != u64::MAX && (if booting { d.saturating_sub(since) < BOOT_NS } else { sleeping() }) {
        k_mut(|k| k.clock = k.clock.max(d));
        raise(IRQ_TIMER);
        return Promise::resolve(&JsValue::NULL);
    }
    // Nothing to do until the page's input: quiet.
    let mut wake = None;
    let p = Promise::new(&mut |ok, _| wake = Some(ok));
    S.with_borrow_mut(|s| s.idle = wake);
    k_mut(|k| k.booting = false);
    crate::machine::settle(None);
    p
}

/// Raise interrupt line `irq` (the page's), and wake the CPU if it idles.
pub fn interrupt(irq: u32) {
    raise(irq);
    if let Some(w) = S.with_borrow_mut(|s| s.idle.take()) {
        let _ = w.call0(&JsValue::NULL);
    }
}

/// Whether the kernel is quiet: idle, until the page's input.
pub fn quiet() -> bool {
    S.with_borrow(|s| s.idle.is_some())
}
