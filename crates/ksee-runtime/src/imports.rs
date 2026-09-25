//! The kernel's imports (arch/wasm/include/asm/wasm.h).
//!
//! Frequent ones (time, console, command line, bulk stores, spawn_task)
//! are this module's exports, so the kernel calls them directly. The rest
//! need JS objects (promises, programs' modules and memories) and are
//! closures reached through wasm-bindgen; switch_task, idle and halt
//! suspend the kernel's stack (see sched.rs).

use std::cell::RefCell;
use std::collections::BTreeMap;

use js_sys::{Function, Object, Promise, Uint8Array, WebAssembly};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::js;
use crate::sched;
use crate::state::*;
use ksee_core::Memory;

// ---- Exports the kernel imports directly -------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn console_write(p: *const u8, n: u32) {
    k(|k| k.console_out.borrow_mut().extend_from_slice(unsafe { core::slice::from_raw_parts(p, n as usize) }));
}

/// Pending console input, without blocking.
#[unsafe(no_mangle)]
pub extern "C" fn console_read(p: *mut u8, n: u32) -> i32 {
    bulk(p as u32, n);
    let out = unsafe { core::slice::from_raw_parts_mut(p, n as usize) };
    k(|k| {
        let mut input = k.console_in.borrow_mut();
        let n = input.len().min(out.len());
        for (o, b) in out.iter_mut().zip(input.drain(..n)) {
            *o = b;
        }
        n as i32
    })
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

/// ksee.bulk, for the kernel (memcpy, memset, wide stores), and for what
/// the runtime itself writes into kernel memory: Store records with the old
/// contents of `addr..addr + len`, in 8-byte stores, up to 256 bytes (the
/// rest as a range of unknown old contents).
#[unsafe(no_mangle)]
pub extern "C" fn bulk(addr: u32, len: u32) {
    use ksee_core::trace::{STORE, UNKNOWN, VALUE_TAG};
    if len == 0 || !recording() {
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
            ring_put(&[addr + off, STORE << 28 | class, v as u32, VALUE_TAG << 28, (v >> 32) as u32, VALUE_TAG << 28 | 1]);
        } else {
            ring_put(&[addr + off, STORE << 28 | class, v as u32, VALUE_TAG << 28]);
        }
        off += size;
    }
    if len > logged {
        ring_put(&[addr + logged, STORE << 28 | UNKNOWN, len - logged, VALUE_TAG << 28]);
    }
}

/// Append a record (pairs of words) to the kernel's trace ring (called
/// while the kernel runs: no kernel code writes it meanwhile).
fn ring_put(words: &[u32]) {
    let (trace, size) = (k(|k| k.trace.get()), tables().ring_words);
    if trace == 0 || size == 0 || !recording() {
        return;
    }
    let p = Direct.read_u32(trace);
    let ring = trace + ksee_core::trace::RING;
    for (i, &w) in words.iter().enumerate() {
        Direct.write_u32(ring + ((p + i as u32) & (size - 1)) * 4, w);
    }
    Direct.write_u32(trace, p + words.len() as u32);
}

/// A Task record for the current task (it enters a user program): its
/// pid and comm, as they are now.
pub(crate) fn task_record() {
    let task = current();
    let Some(t) = ksee_core::task::read_task(tables(), &Direct, task) else { return };
    let mut comm = [0u8; 16];
    let n = t.comm.len().min(16);
    comm[..n].copy_from_slice(&t.comm.as_bytes()[..n]);
    let word = |i: usize| u32::from_le_bytes(comm[4 * i..4 * i + 4].try_into().unwrap());
    let value = ksee_core::trace::VALUE_TAG << 28;
    ring_put(&[task, ksee_core::trace::TASK << 28, t.pid as u32, value, word(0), value | 1,
               word(1), value | 2, word(2), value | 3, word(3), value | 4]);
}

// ---- Programs ----------------------------------------------------------------

/// The tasks' programs: the one each loaded (exec), and the one it runs.
#[derive(Default)]
struct Programs {
    loaded: BTreeMap<u32, WebAssembly::Instance>,
    running: BTreeMap<u32, WebAssembly::Memory>,
    /// The kernel's syscall export (a program's import), and its function
    /// table (where a program's _start goes, for the kernel to call it).
    syscall: Option<Function>,
    table: Option<WebAssembly::Table>,
}

thread_local! {
    static PROGRAMS: RefCell<Programs> = RefCell::default();
}

fn current() -> u32 {
    Direct.read_u32(k(|k| k.current.get()))
}

/// The part of the current task's program's memory at `p..p+n`, if in bounds.
fn user_bytes(p: u32, n: u32) -> Option<Uint8Array> {
    PROGRAMS.with_borrow(|u| {
        let buf = u.running.get(&current())?.buffer();
        let len = js::get(&buf, "byteLength").as_f64()? as u64;
        (p as u64 + n as u64 <= len).then(|| Uint8Array::new_with_byte_offset_and_length(&buf, p, n))
    })
}

/// Load program `image` for the current task: its _start's index in the
/// kernel's function table, or 0.
fn user_load(image: u32, len: u32) -> u32 {
    let bytes = js::copy_from(image as *const u8, len);
    let (syscall, table) = PROGRAMS.with_borrow(|u| (u.syscall.clone(), u.table.clone()));
    let instance = WebAssembly::Module::new(&bytes).and_then(|m| {
        let linux = Object::new();
        js::set(&linux, "syscall", syscall.unwrap());
        let (ready, call, done) = init_imports();
        let ksee = Object::new();
        js::set(&ksee, "ready", ready);
        js::set(&ksee, "call", call);
        js::set(&ksee, "done", done);
        let imports = Object::new();
        js::set(&imports, "linux", linux);
        js::set(&imports, "ksee", ksee);
        WebAssembly::Instance::new(&m, &imports)
    });
    let inst = match instance {
        Ok(i) => i,
        Err(e) => {
            web_sys::console::error_2(&"exec:".into(), &e);
            return 0;
        }
    };
    let table = table.unwrap();
    let Ok(at) = table.grow(1) else { return 0 };
    let start: Function = js::get(&inst.exports(), "_start").unchecked_into();
    if table.set(at, &start).is_err() {
        return 0;
    }
    PROGRAMS.with_borrow_mut(|u| u.loaded.insert(current(), inst));
    at
}

/// The current task enters the program it loaded.
fn user_enter() {
    PROGRAMS.with_borrow_mut(|u| {
        let task = current();
        if let Some(inst) = u.loaded.remove(&task) {
            u.running.insert(task, js::get(&inst.exports(), "memory").unchecked_into());
        }
    });
    task_record();
}

fn func<F: ?Sized + wasm_bindgen::closure::WasmClosure>(c: Closure<F>) -> JsValue {
    c.into_js_value()
}

/// The import object for the kernel (and the programs' syscall import,
/// once it is instantiated: see instantiated).
pub fn kernel() -> Object {
    let rt = wasm_bindgen::exports();
    let export = |name: &str| js::get(&rt, name);

    let kernel = Object::new();
    for name in ["console_write", "console_read", "get_cmdline", "memory_end", "time_ns", "spawn_task"] {
        js::set(&kernel, name, export(name));
    }
    js::set(&kernel, "switch_task", sched::suspending(func(Closure::<dyn FnMut(u32, u32, u32) -> Promise>::new(sched::switch_task))));
    js::set(&kernel, "idle", sched::suspending(func(Closure::<dyn FnMut() -> Promise>::new(sched::idle))));
    js::set(&kernel, "halt", sched::suspending(func(Closure::<dyn FnMut(i32) -> Promise>::new(|code: i32| {
        k(|k| k.halted.set(Some(code)));
        if let Some((_, fail)) = k(|k| k.ready.take()) {
            let _ = fail.call1(&JsValue::NULL, &format!("the kernel halted ({code}) before /init was ready").into());
        }
        sched::never()
    }))));
    js::set(&kernel, "print_stack", func(Closure::<dyn FnMut()>::new(|| {
        web_sys::console::log_1(&js::get(&js_sys::Error::new("kernel stack"), "stack"));
    })));
    js::set(&kernel, "user_load", func(Closure::<dyn FnMut(u32, u32) -> u32>::new(user_load)));
    js::set(&kernel, "user_enter", func(Closure::<dyn FnMut()>::new(user_enter)));
    // Both return the number of bytes not copied.
    js::set(&kernel, "user_read", func(Closure::<dyn FnMut(u32, u32, u32) -> u32>::new(
        |to: u32, from: u32, n: u32| match user_bytes(from, n) {
            Some(src) => {
                bulk(to, n); // (what it overwrites, for the memory history)
                js::copy_to(&src, to as *mut u8);
                0
            }
            None => n,
        })));
    js::set(&kernel, "user_write", func(Closure::<dyn FnMut(u32, u32, u32) -> u32>::new(
        |to: u32, from: u32, n: u32| match user_bytes(to, n) {
            Some(dst) => {
                dst.copy_from(unsafe { core::slice::from_raw_parts(from as *const u8, n as usize) });
                0
            }
            None => n,
        })));

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

/// The kernel is instantiated (its `exports`): programs import its syscall
/// export, and their _start goes in its function table. None of the tasks'
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

/// ksee.ready, ksee.call and ksee.done, for /init (user/init.c): it is
/// ready (the boot is over), the syscall the page asks for (Kernel::call),
/// and its result. The request's place in the program's memory:
/// `struct { i32 nr; i32 args[6]; i32 pad; u8 data[CALL_DATA]; }`.
fn init_imports() -> (JsValue, JsValue, JsValue) {
    let ready = func(Closure::<dyn FnMut()>::new(|| {
        k(|k| k.waiting.set(None));
        if let Some((ok, _)) = k(|k| k.ready.take()) {
            let _ = ok.call0(&JsValue::NULL);
        }
    }));
    // ksee.call(req): the request into `req` (buffers: pointers into its
    // data); its number, or -1 if there is none.
    let call = func(Closure::<dyn FnMut(u32) -> i32>::new(|req: u32| {
        k(|k| {
            let c = k.call.borrow();
            if !c.asked {
                return -1;
            }
            let Some(mem) = user_bytes(req, 32 + CALL_DATA as u32) else { return -1 };
            let mut head = [0u8; 32];
            head[0..4].copy_from_slice(&c.nr.to_le_bytes());
            for (i, &a) in c.args.iter().enumerate() {
                let v = if c.buffers >> i & 1 != 0 { a + req + 32 } else { a };
                head[4 + 4 * i..8 + 4 * i].copy_from_slice(&v.to_le_bytes());
            }
            mem.subarray(0, 32).copy_from(&head);
            mem.subarray(32, 32 + CALL_DATA as u32).copy_from(&c.data);
            c.nr as i32
        })
    }));
    // ksee.done(ret, req): the result, and the buffers as the call left
    // them: Machine::call's promise resolved with { ret, buffers: [the bytes
    // of each buffer argument (null for the others)] }.
    let done = func(Closure::<dyn FnMut(i64, u32)>::new(|ret: i64, req: u32| {
        let bytes = user_bytes(req + 32, CALL_DATA as u32).map(|m| m.to_vec()).unwrap_or_default();
        let c = k(|k| {
            k.waiting.set(None);
            k.call.take()
        });
        let buffers = js_sys::Array::new();
        for i in 0..6 {
            let at = c.args[i] as usize;
            buffers.push(&match bytes.get(at..at + c.lens[i] as usize) {
                Some(b) if c.buffers >> i & 1 != 0 => Uint8Array::from(b).into(),
                _ => JsValue::NULL,
            });
        }
        let o = Object::new();
        js::set(&o, "ret", ret as f64);
        js::set(&o, "buffers", buffers);
        if let Some(r) = c.resolve {
            let _ = r.call1(&JsValue::NULL, &o);
        }
    }));
    (ready, call, done)
}
