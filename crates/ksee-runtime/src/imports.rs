//! The kernel's imports.
//!
//! Frequent ones (time, console, command line, halt, bulk stores) are
//! this module's exports, so the kernel calls them directly. The rest need
//! JS objects (Workers, user programs' modules and memories) and are
//! closures reached through wasm-bindgen.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use js_sys::{Function, Object, Uint8Array, WebAssembly};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::js;
use crate::state::*;
use ksee_core::Memory;

/// Thrown to unwind a kernel Worker's stack when its task is dead or the
/// machine halts.
pub const EXIT: &str = "ksee:exit";
pub const HALT: &str = "ksee:halt";

// ---- Exports the kernel imports directly -------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn console_write(p: *const u8, n: u32) {
    CONSOLE_OUT.put(unsafe { core::slice::from_raw_parts(p, n as usize) });
}

/// Pending console input, without blocking.
#[unsafe(no_mangle)]
pub extern "C" fn console_read(p: *mut u8, n: u32) -> i32 {
    bulk(p as u32, n);
    CONSOLE_IN.take(unsafe { core::slice::from_raw_parts_mut(p, n as usize) }) as i32
}

/// Copy the command line (NUL-terminated) to `buf`.
#[unsafe(no_mangle)]
pub extern "C" fn get_cmdline(buf: *mut u8, len: u32) -> u32 {
    let cmd = CMDLINE.get().map_or(&[][..], Vec::as_slice);
    let n = cmd.len().min(len as usize - 1);
    unsafe {
        core::ptr::copy_nonoverlapping(cmd.as_ptr(), buf, n);
        *buf.add(n) = 0;
    }
    n as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn memory_end() -> u32 {
    MEMORY_END.load(Ordering::Relaxed)
}

/// Nanoseconds since the machine started (the kernel's clock).
#[unsafe(no_mangle)]
pub extern "C" fn time_ns() -> u64 {
    ((js::now_ms() - f64::from_bits(EPOCH_MS.load(Ordering::Relaxed))) * 1e6) as u64
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

/// Append a record (pairs of words) to the kernel's trace ring, from a
/// task's Worker while it holds the CPU (so no kernel code writes too).
fn ring_put(words: &[u32]) {
    let (trace, size) = (TRACE.load(Ordering::Relaxed), RING_WORDS.load(Ordering::Relaxed));
    if trace == 0 || size == 0 || !recording() {
        return;
    }
    let pos = unsafe { &*(trace as *const AtomicU32) };
    let p = pos.load(Ordering::Relaxed);
    let ring = trace + ksee_core::trace::RING;
    for (i, &w) in words.iter().enumerate() {
        let at = ring + ((p + i as u32) & (size - 1)) * 4;
        unsafe { (at as *mut u32).write_volatile(w) };
    }
    pos.store(p + words.len() as u32, Ordering::Release);
}

/// A Task record for the current task (it enters a user program): its
/// pid and comm, as they are now.
pub(crate) fn task_record() {
    let task = Direct.read_u32(CURRENT_TASK.load(Ordering::Relaxed));
    let Some(t) = ksee_core::task::read_task(tables(), &Direct, task) else { return };
    let mut comm = [0u8; 16];
    let n = t.comm.len().min(16);
    comm[..n].copy_from_slice(&t.comm.as_bytes()[..n]);
    let word = |i: usize| u32::from_le_bytes(comm[4 * i..4 * i + 4].try_into().unwrap());
    let value = ksee_core::trace::VALUE_TAG << 28;
    ring_put(&[task, ksee_core::trace::TASK << 28, t.pid as u32, value, word(0), value | 1,
               word(1), value | 2, word(2), value | 3, word(3), value | 4]);
}

/// A new task: a Worker takes it (see worker.rs), one waiting already or,
/// if none waits, one the page starts.
#[unsafe(no_mangle)]
pub extern "C" fn spawn_task(task: u32, sp: u32) {
    if SPAWN.put(task, sp) {
        MORE.with_borrow(|more| {
            if let Some(f) = more {
                let _ = f.call0(&JsValue::NULL);
            }
        });
    }
}

thread_local! {
    /// Asks the page for another Worker (see worker.rs).
    pub static MORE: RefCell<Option<Function>> = const { RefCell::new(None) };
}

/// Record the halt for the page and stop this Worker.
#[unsafe(no_mangle)]
pub extern "C" fn halt(code: i32) {
    HALTED.store(code as u32 + 1, Ordering::Release);
    wasm_bindgen::throw_str(HALT);
}

// ---- The rest ----------------------------------------------------------------

/// This Worker's user program state.
#[derive(Default)]
struct User {
    kernel_syscall: Option<Function>,
    loaded: Option<WebAssembly::Instance>,
    running: Option<WebAssembly::Instance>,
    memory: Option<WebAssembly::Memory>,
}

thread_local! {
    static USER: RefCell<User> = RefCell::default();
}

/// The part of the running program's memory at `p..p+n`, if in bounds.
fn user_bytes(p: u32, n: u32) -> Option<Uint8Array> {
    USER.with_borrow(|u| {
        let buf = u.memory.as_ref()?.buffer();
        let len = js::get(&buf, "byteLength").as_f64()? as u64;
        (p as u64 + n as u64 <= len).then(|| Uint8Array::new_with_byte_offset_and_length(&buf, p, n))
    })
}

fn func<F: ?Sized + wasm_bindgen::closure::WasmClosure>(c: Closure<F>) -> JsValue {
    c.into_js_value()
}

/// The import object for a kernel instance. On the page (`worker` false)
/// the rare imports fail: the page's instance only initializes memory and
/// reads the kernel.
pub fn kernel(memory: &JsValue, worker: bool) -> Object {
    let rt = wasm_bindgen::exports();
    let export = |name: &str| js::get(&rt, name);

    let kernel = Object::new();
    for name in ["console_write", "console_read", "get_cmdline", "memory_end", "time_ns", "halt", "spawn_task"] {
        js::set(&kernel, name, export(name));
    }
    js::set(&kernel, "exit_task", func(Closure::<dyn FnMut()>::new(|| wasm_bindgen::throw_str(EXIT))));
    js::set(&kernel, "print_stack", func(Closure::<dyn FnMut()>::new(|| {
        web_sys::console::log_1(&js::get(&js_sys::Error::new("kernel stack"), "stack"));
    })));
    let (call, done) = call_imports();
    js::set(&kernel, "user_load", func(Closure::<dyn FnMut(u32, u32) -> i32>::new(
        move |image: u32, len: u32| {
            assert!(worker, "exec on the page");
            let bytes = js::copy_from(image as *const u8, len);
            let result = WebAssembly::Module::new(&bytes).and_then(|m| {
                let linux = Object::new();
                USER.with_borrow(|u| js::set(&linux, "syscall", u.kernel_syscall.clone().unwrap()));
                let ksee = Object::new();
                js::set(&ksee, "call", call.clone());
                js::set(&ksee, "done", done.clone());
                let imports = Object::new();
                js::set(&imports, "linux", linux);
                js::set(&imports, "ksee", ksee);
                WebAssembly::Instance::new(&m, &imports)
            });
            match result {
                Ok(inst) => {
                    USER.with_borrow_mut(|u| u.loaded = Some(inst));
                    0
                }
                Err(e) => {
                    web_sys::console::error_2(&"exec:".into(), &e);
                    1
                }
            }
        })));
    js::set(&kernel, "user_enter", func(Closure::<dyn FnMut()>::new(|| {
        let inst = USER.with_borrow_mut(|u| {
            let inst = u.loaded.take().expect("no program loaded");
            u.memory = Some(js::get(&inst.exports(), "memory").unchecked_into());
            u.running = Some(inst.clone());
            inst
        });
        task_record();
        let start: Function = js::get(&inst.exports(), "_start").unchecked_into();
        // A user program's exit unwinds through here (see EXIT); anything
        // else it throws is a crash of the program.
        if let Err(e) = start.call0(&JsValue::NULL) {
            wasm_bindgen::throw_val(e);
        }
    })));
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
    js::set(&env, "memory", memory.clone());
    let imports = Object::new();
    js::set(&imports, "env", env);
    js::set(&imports, "kernel", kernel);
    js::set(&imports, "ksee", ksee);
    imports
}

/// ksee.call and ksee.done, for /init (user/init.c): the syscall the page
/// asks for (state::CALL). The request's place in the program's memory:
/// `struct { i32 nr; i32 args[6]; i32 pad; u8 data[CALL_DATA]; }`.
fn call_imports() -> (JsValue, JsValue) {
    // ksee.call(req): the request into `req` (buffers: pointers into its
    // data); its number, or -1 if there is none.
    let call = func(Closure::<dyn FnMut(u32) -> i32>::new(|req: u32| {
        if CALL.state.load(Ordering::Acquire) != CALL_ASKED {
            return -1;
        }
        let Some(mem) = user_bytes(req, 32 + CALL_DATA as u32) else { return -1 };
        let mut head = [0u8; 32];
        let buffers = CALL.buffers.load(Ordering::Relaxed);
        head[0..4].copy_from_slice(&CALL.nr.load(Ordering::Relaxed).to_le_bytes());
        for (i, a) in CALL.args.iter().enumerate() {
            let mut v = a.load(Ordering::Relaxed);
            if buffers >> i & 1 != 0 {
                v += req + 32;
            }
            head[4 + 4 * i..8 + 4 * i].copy_from_slice(&v.to_le_bytes());
        }
        mem.subarray(0, 32).copy_from(&head);
        mem.subarray(32, 32 + CALL_DATA as u32).copy_from(CALL.data());
        CALL.nr.load(Ordering::Relaxed) as i32
    }));
    // ksee.done(ret, req): the result, and the buffers as the call left them.
    let done = func(Closure::<dyn FnMut(i64, u32)>::new(|ret: i64, req: u32| {
        if let Some(mem) = user_bytes(req + 32, CALL_DATA as u32) {
            mem.copy_to(CALL.data());
        }
        CALL.ret.store(ret as u64, Ordering::Relaxed);
        CALL.state.store(CALL_DONE, Ordering::Release);
    }));
    (call, done)
}

/// Remember this Worker's kernel, for user programs' syscall import.
pub fn set_kernel(exports: &Object) {
    USER.with_borrow_mut(|u| u.kernel_syscall = Some(js::get(exports, "syscall").unchecked_into()));
}

/// Forget the last task's program (the Worker runs another task).
pub fn reset_user() {
    USER.with_borrow_mut(|u| {
        u.loaded = None;
        u.running = None;
        u.memory = None;
    });
}
