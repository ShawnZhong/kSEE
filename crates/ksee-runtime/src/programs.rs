//! User programs: position-independent modules, in kernel memory (see the
//! kernel's binfmt_wasm.c): compiled (exec), instantiated where exec mapped
//! them, and each task's user stack pointer; and the runners (user/init.c),
//! the programs that make the page's calls.

use std::cell::RefCell;
use std::collections::BTreeMap;

use js_sys::{Function, Object, Uint8Array, WebAssembly};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::imports::bulk;
use crate::js::{self, func};
use crate::state::*;
use ksee_core::Memory;

/// The tasks' programs (position-independent modules, in kernel memory):
/// the one each compiled (exec), until it is instantiated; and each task's
/// user stack pointer. A program's __stack_pointer is its instance's
/// global, which its threads (and a vfork child, until it execs) share: a
/// task's own value is put back in it when the task gets the CPU.
#[derive(Default)]
struct Programs {
    loaded: BTreeMap<u32, WebAssembly::Module>,
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
    ksee_core::task::read_task(tables(), &Direct, current()).map_or(0, |t| t.pid as u32)
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
        let gone = k(|k| {
            let mut runners = k.runners.borrow_mut();
            let tid = runners.iter().find(|(_, task)| **task == prev).map(|(&t, _)| t);
            tid.inspect(|t| {
                runners.remove(t);
            })
        });
        if let Some(tid) = gone {
            finish(tid, "exited", 0, Vec::new());
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

const ENOEXEC: i32 = 8;

/// A module's dylink.0 memory info (the tool conventions' dynamic linking):
/// the bytes its data needs, and the table entries.
fn dylink(module: &WebAssembly::Module) -> Option<(u32, u32)> {
    let sections = WebAssembly::Module::custom_sections(module, "dylink.0");
    let b = Uint8Array::new(&sections.get(0)).to_vec();
    let mut at = 0;
    let mut leb = || {
        let (mut v, mut shift) = (0u32, 0);
        loop {
            let byte = *b.get(at)?;
            at += 1;
            v |= ((byte & 0x7f) as u32) << shift;
            shift += 7;
            if byte & 0x80 == 0 {
                return Some(v);
            }
        }
    };
    // (Subsection 1, WASM_DYLINK_MEM_INFO, comes first.)
    let (kind, _len) = (leb()?, leb()?);
    if kind != 1 {
        return None;
    }
    let (mem_size, _align, table_size) = (leb()?, leb()?, leb()?);
    Some((mem_size, table_size))
}

/// Compile program `image` for the current task: the bytes its data
/// needs, or -ENOEXEC.
pub(crate) fn user_load(image: u32, len: u32) -> i32 {
    let Ok(module) = WebAssembly::Module::new(&js::copy_from(image as *const u8, len)) else { return -ENOEXEC };
    let Some((data, _)) = dylink(&module) else { return -ENOEXEC };
    PROGRAMS.with_borrow_mut(|u| u.loaded.insert(current(), module));
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
    let (Some(module), Some(table)) = (module, table) else { return 0 };
    let start = || -> Result<u32, JsValue> {
        let (_, entries) = dylink(&module).ok_or("no dylink.0")?;
        let table_base = table.grow(entries)?;
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
        let entry = |name: &str| -> Result<u32, JsValue> {
            let Ok(f) = js::get(&exports, name).dyn_into::<Function>() else { return Ok(0) };
            let at = table.grow(1)?;
            table.set(at, &f)?;
            Ok(at)
        };
        let (start, clone) = (entry("_start")?, entry("__wasm_clone_start")?);
        bulk(clone_entry, 4);
        Direct.write_u32(clone_entry, clone);
        PROGRAMS.with_borrow_mut(|u| u.users.insert(current(), User { sp, saved: top }));
        Ok(start)
    };
    start().unwrap_or_else(|e| {
        web_sys::console::error_2(&"exec:".into(), &e);
        0
    })
}

// ---- Runners -----------------------------------------------------------------

/// Finish runner `tid`'s call, if it has one (for Machine::quiet); the
/// page waits no more once none is left.
pub(crate) fn finish(tid: u32, how: &'static str, ret: i64, buffers: Vec<Option<Vec<u8>>>) {
    k(|k| {
        let Some(c) = k.calls.borrow_mut().remove(&tid) else { return };
        k.finished.borrow_mut().push(Finished { id: c.id, tid, how, ret, buffers });
        if k.calls.borrow().is_empty() {
            k.waiting.set(None);
        }
    });
}

/// The imports of a runner (user/init.c), module ksee: ready, the call
/// asked of it, and its result. A request's place in the
/// runner's memory (kernel memory):
/// `struct { i32 nr; i32 args[6]; i32 pad; u8 data[CALL_DATA]; }`.
fn runner_imports() -> Object {
    let ksee = Object::new();
    // ksee.ready(): the current thread is a runner, waiting for calls
    // (rung: SIGUSR1). (A call it was making is over: it was an execve, and
    // this is the new program.)
    js::set(&ksee, "ready", func(Closure::<dyn FnMut()>::new(|| {
        let (tid, task) = (tid(), current());
        k(|k| k.runners.borrow_mut().insert(tid, task));
        finish(tid, "exec", 0, Vec::new());
        if tid == 1 && k(|k| k.calls.borrow().is_empty()) {
            // (The boot's over: let what it left for later run now, while
            // nothing is recorded.)
            k(|k| k.waiting.set(Some(k.clock.get())));
        }
    })));
    // ksee.call(req): the request into `req` (buffers: pointers into its
    // data); its number, or -1 if there is none.
    js::set(&ksee, "call", func(Closure::<dyn FnMut(u32) -> i32>::new(|req: u32| {
        let tid = tid();
        k(|k| {
            let mut calls = k.calls.borrow_mut();
            let Some(c) = calls.get_mut(&tid).filter(|c| !c.taken) else { return -1 };
            c.taken = true;
            let mut head = [0u8; 32];
            head[0..4].copy_from_slice(&c.nr.to_le_bytes());
            for (i, &a) in c.args.iter().enumerate() {
                let v = if c.buffers >> i & 1 != 0 { a + req + 32 } else { a };
                head[4 + 4 * i..8 + 4 * i].copy_from_slice(&v.to_le_bytes());
            }
            // (Into the runner's memory, which is kernel memory: recorded.)
            bulk(req, 32 + CALL_DATA as u32);
            let mut data = c.data.clone();
            for &r in &c.relocs {
                let w = r as usize;
                let v = u32::from_le_bytes(data[w..w + 4].try_into().unwrap()) + req + 32;
                data[w..w + 4].copy_from_slice(&v.to_le_bytes());
            }
            unsafe {
                core::ptr::copy_nonoverlapping(head.as_ptr(), req as *mut u8, 32);
                core::ptr::copy_nonoverlapping(data.as_ptr(), (req + 32) as *mut u8, CALL_DATA);
            }
            c.nr as i32
        })
    })));
    // ksee.done(ret, req): the result, and the buffers as the call left them.
    js::set(&ksee, "done", func(Closure::<dyn FnMut(i64, u32)>::new(|ret: i64, req: u32| {
        let tid = tid();
        let mut bytes = vec![0; CALL_DATA];
        Direct.read(req + 32, &mut bytes);
        let buffers = k(|k| {
            let calls = k.calls.borrow();
            let Some(c) = calls.get(&tid) else { return Vec::new() };
            (0..6).map(|i| {
                let at = c.args[i] as usize;
                (c.buffers >> i & 1 != 0).then(|| bytes.get(at..at + c.lens[i] as usize).map(<[u8]>::to_vec)).flatten()
            }).collect()
        });
        finish(tid, "returned", ret, buffers);
    })));
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
