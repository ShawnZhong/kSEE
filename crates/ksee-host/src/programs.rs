//! User programs: position-independent modules, in kernel memory (see the
//! kernel's binfmt_wasm.c): compiled (exec), instantiated where exec mapped
//! them, and each task's user stack pointer; and the runners (user/prog.c),
//! the programs that make the page's calls.

use std::cell::RefCell;
use std::collections::BTreeMap;

use js_sys::{Function, Object, Uint8Array, WebAssembly};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::js;
use crate::state::*;
use ksee_core::Memory;

/// The tasks' programs (position-independent modules, in kernel memory):
/// the one each compiled (exec), until it is instantiated; and each task's
/// user stack pointer. A program's __stack_pointer is its instance's
/// global, which its threads (and a vfork child, until it execs) share: a
/// task's own value is put back in it when the task gets the CPU.
#[derive(Default)]
struct Programs {
    loaded: BTreeMap<u32, (WebAssembly::Module, u32)>,
    users: BTreeMap<u32, User>,
    /// The kernel's syscall export (a program's import), and its function
    /// table (the programs' functions go in it too, and their entries).
    syscall: Option<Function>,
    table: Option<WebAssembly::Table>,
}

struct User {
    sp: WebAssembly::Global,
    /// Its value while the task is not running.
    saved: u32,
}

thread_local! {
    static PROGRAMS: RefCell<Programs> = RefCell::default();
}

/// The current task's thread id.
fn tid() -> u32 {
    tables().types.read_task(&Direct, current()).map_or(0, |t| t.pid as u32)
}

fn value(g: &WebAssembly::Global) -> u32 {
    g.value().as_f64().unwrap_or(0.0) as u32
}

/// `prev` gives the CPU to `next` (see sched::switch_task): their user
/// stack pointers; a dead `prev` is gone, and so is its runner.
pub(crate) fn switch_user(prev: u32, next: u32, dead: bool) {
    PROGRAMS.with_borrow_mut(|u| {
        if dead {
            u.users.remove(&prev);
        } else if let Some(p) = u.users.get_mut(&prev) {
            p.saved = value(&p.sp);
        }
        if let Some(n) = u.users.get(&next) {
            n.sp.set_value(&n.saved.into());
        }
    });
    if dead {
        let gone = state_mut(|s| {
            let tid = s.session.runner_of(prev);
            tid.inspect(|&t| s.session.forget(t))
        });
        if let Some(tid) = gone {
            finish(tid, None);
        }
    }
}

/// clone_user: task `p` is a user clone of the current task, on its stack
/// at `sp` (0: below the current task's, vfork).
pub(crate) fn clone_user(p: u32, sp: u32) {
    PROGRAMS.with_borrow_mut(|u| {
        let Some(me) = u.users.get(&current()) else { return };
        let saved = if sp != 0 { sp } else { value(&me.sp) };
        let sp_global = me.sp.clone();
        u.users.insert(p, User { sp: sp_global, saved });
    });
}

/// (asm-generic/errno-base.h's.)
const ENOEXEC: i32 = 8;

/// Compile program `image` for the current task: the bytes its data
/// needs, or -ENOEXEC.
pub(crate) fn user_load(image: u32, len: u32) -> i32 {
    let Some((data, entries)) = ksee_core::wasm::dylink(&Direct, image, len) else { return -ENOEXEC };
    let Ok(module) = WebAssembly::Module::new(&Uint8Array::from(unsafe { core::slice::from_raw_parts(image as *const u8, len as usize) })) else { return -ENOEXEC };
    PROGRAMS.with_borrow_mut(|u| u.loaded.insert(current(), (module, entries)));
    data as i32
}

fn global(value: u32, mutable: bool) -> Result<WebAssembly::Global, JsValue> {
    let desc = Object::new();
    js::set(&desc, "value", "i32");
    js::set(&desc, "mutable", mutable);
    WebAssembly::Global::new(&desc, &value.into())
}

/// Instantiate the current task's program, its data at `base` and its
/// stack below `top`: its _start's index in the kernel's function table,
/// or 0; its __wasm_clone_start's (or 0) at `clone_entry`.
pub(crate) fn user_start(base: u32, top: u32, clone_entry: u32) -> u32 {
    let (module, syscall, table) = PROGRAMS.with_borrow_mut(|u| (u.loaded.remove(&current()), u.syscall.clone(), u.table.clone()));
    let (Some((module, entries)), Some(table)) = (module, table) else { return 0 };
    let start = || -> Result<u32, JsValue> {
        // (Room for its table entries and the two below, grown once.)
        let table_base = table.grow(entries + 2)?;
        let sp = global(top, true)?;
        let env = Object::new();
        js::set(&env, "memory", wasm_bindgen::memory());
        js::set(&env, "__indirect_function_table", &table);
        js::set(&env, "__memory_base", global(base, false)?);
        js::set(&env, "__table_base", global(table_base, false)?);
        js::set(&env, "__stack_pointer", &sp);
        let linux = Object::new();
        js::set(&linux, "syscall", syscall.unwrap());
        let imports = Object::new();
        js::set(&imports, "env", env);
        js::set(&imports, "linux", linux);
        js::set(&imports, "ksee", runner_imports());
        // (GOT.mem: the addresses of data it does not define: only weak
        // symbols, left undefined, so 0.)
        let got = Object::new();
        for i in WebAssembly::Module::imports(&module).iter() {
            if js::get(&i, "module").as_string().as_deref() == Some("GOT.mem") {
                js::set(&got, &js::get(&i, "name").as_string().unwrap_or_default(), global(0, true)?);
            }
        }
        js::set(&imports, "GOT.mem", got);
        let inst = WebAssembly::Instance::new(&module, &imports)?;
        let exports = inst.exports();
        if let Ok(relocs) = js::get(&exports, "__wasm_apply_data_relocs").dyn_into::<Function>() {
            relocs.call0(&JsValue::NULL)?;
        }
        // (Its entries, in the kernel's table, to call directly.)
        let entry = |at: u32, name: &str| -> Result<u32, JsValue> {
            let Ok(f) = js::get(&exports, name).dyn_into::<Function>() else { return Ok(0) };
            table.set(at, &f)?;
            Ok(at)
        };
        let (start, clone) = (entry(table_base + entries, "_start")?, entry(table_base + entries + 1, "__wasm_clone_start")?);
        crate::imports::logged(clone_entry, 4);
        Direct.write_u32(clone_entry, clone);
        PROGRAMS.with_borrow_mut(|u| u.users.insert(current(), User { sp, saved: top }));
        // (A runner's call that was this execve is over: the new program
        // runs. A runner again only if it says so: ksee.ready.)
        let tid = tid();
        finish(tid, None);
        state_mut(|s| s.session.forget(tid));
        Ok(start)
    };
    start().unwrap_or_else(|e| {
        web_sys::console::error_2(&"exec:".into(), &e);
        0
    })
}

// ---- Runners -----------------------------------------------------------------

/// What the kernel records: everything while a call the page asked for is
/// under way (begun, not done: the syscall, and what the kernel does
/// meanwhile, other tasks it wakes or waits for); between calls, stores
/// only (memory stays exact; the runners' waits for their next calls, the
/// kernel's own work then, are not shown). The program's first line
/// starts it (the boot, before, is not recorded).
pub(crate) fn record() {
    crate::imports::record(if state(|s| s.session.recording()) { 1 } else { 2 });
}

/// Finish runner `tid`'s call, if it has one (for Machine::quiet); the
/// page waits no more once none is left.
pub(crate) fn finish(tid: u32, ret: Option<i64>) {
    let done = state_mut(|s| {
        let done = s.session.finish(tid, ret);
        if done && s.session.all_done() {
            s.waiting = None;
        }
        done
    });
    if done {
        record();
    }
}

/// The imports of a runner (user/prog.c), module ksee. ready(req, heap,
/// size, main): the current thread is a runner (the program's own, its
/// main thread, if `main`: see Machine::main), waiting for calls in a futex on
/// the number in its request at `req` (user/prog.c's struct request, { nr, args[6] }: the host puts a
/// call there, and rings); its program's own memory at `heap` (its
/// variables and its calls' literals, see Machine::make). begin(), end(ret): around the call it makes: recorded from the
/// one to the other; the result.
fn runner_imports() -> Object {
    let ksee = Object::new();
    js::set(&ksee, "ready", Closure::<dyn FnMut(u32, u32, u32, u32)>::new(|req: u32, heap: u32, size: u32, main: u32| {
        let (tid, task) = (tid(), current());
        state_mut(|s| {
            if s.session.ready(tid, task, req, heap, size, main != 0) {
                // (The boot's over: let what it left for later run now, while
                // nothing is recorded.)
                s.waiting = Some(s.clock);
            }
        });
    }).into_js_value());
    js::set(&ksee, "begin", Closure::<dyn FnMut()>::new(|| {
        let tid = tid();
        // (Its recording starts here: the records' id, which counts the
        // words ever written: see trace::ring.)
        let begun = state_mut(|s| s.session.begin(tid, Direct.read_u32(s.trace)));
        if begun {
            record();
        }
    }).into_js_value());
    js::set(&ksee, "end", Closure::<dyn FnMut(i64)>::new(|ret: i64| {
        let tid = tid();
        if state(|s| s.session.begun(tid)) {
            finish(tid, Some(ret));
        }
    }).into_js_value());
    ksee
}

/// The kernel is instantiated (its `exports`): programs import its syscall
/// export, and their entries go in its function table. None of the tasks'
/// programs are left (a new machine).
pub fn instantiated(exports: &Object) {
    PROGRAMS.with_borrow_mut(|u| {
        *u = Programs {
            syscall: Some(js::get(exports, "syscall").unchecked_into()),
            table: Some(js::get(exports, "__indirect_function_table").unchecked_into()),
            ..Default::default()
        };
    });
}
