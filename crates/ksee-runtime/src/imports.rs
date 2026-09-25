//! The kernel's imports.
//!
//! Frequent ones (time, console, command line, halt, the probes' clock) are
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

/// The /dev/ksee channel's mailbox address; its data's size goes to
/// `*capacity`.
#[unsafe(no_mangle)]
pub extern "C" fn channel_mailbox(capacity: *mut u32) -> u32 {
    unsafe { *capacity = MAILBOX_SIZE as u32 };
    &MAILBOX as *const Mailbox as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn memory_end() -> u32 {
    MEMORY_END.load(Ordering::Relaxed)
}

/// Nanoseconds since the machine started (also the probes' clock).
#[unsafe(no_mangle)]
pub extern "C" fn time_ns() -> u64 {
    // The kernel's clock stands still while the debugger has it stopped
    // (else it would catch up on every missed tick when it goes on).
    let at = STOPPED_AT.load(Ordering::Acquire);
    (if at != 0 { at } else { wall_ns() }).saturating_sub(PAUSED_NS.load(Ordering::Acquire))
}

/// Nanoseconds since boot, the stops included.
pub fn wall_ns() -> u64 {
    ((js::now_ms() - f64::from_bits(EPOCH_MS.load(Ordering::Relaxed))) * 1e6) as u64
}

/// ksee.line, for user programs (user/ksee.h): the source line the program
/// is at, as a record in the kernel's trace ring. (The program holds the
/// CPU, so no kernel code writes the ring meanwhile.)
#[unsafe(no_mangle)]
pub extern "C" fn user_line(line: u32) {
    ring_put(&[line, ksee_core::trace::LINE << 28]);
}

/// Append a record (pairs of words) to the kernel's trace ring, from a
/// task's Worker while it holds the CPU (so no kernel code writes too).
fn ring_put(words: &[u32]) {
    let (trace, size) = (TRACE.load(Ordering::Relaxed), RING_WORDS.load(Ordering::Relaxed));
    if trace == 0 || size == 0 {
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
fn task_record() {
    let task = Direct.read_u32(CURRENT_TASK.load(Ordering::Relaxed));
    let Some(t) = live_task(task) else { return };
    let mut comm = [0u8; 16];
    let n = t.comm.len().min(16);
    comm[..n].copy_from_slice(&t.comm.as_bytes()[..n]);
    let word = |i: usize| u32::from_le_bytes(comm[4 * i..4 * i + 4].try_into().unwrap());
    let value = ksee_core::trace::VALUE_TAG << 28;
    ring_put(&[task, ksee_core::trace::TASK << 28, t.pid as u32, value, word(0), value | 1,
               word(1), value | 2, word(2), value | 3, word(3), value | 4]);
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

/// The import object for a kernel instance. `spawn(task, sp)` creates a
/// new task's Worker. On the page (`worker` false) the rare imports fail:
/// the page's instance only initializes memory and reads the kernel.
pub fn kernel(memory: &JsValue, spawn: Function, worker: bool) -> Object {
    let rt = wasm_bindgen::exports();
    let export = |name: &str| js::get(&rt, name);

    let kernel = Object::new();
    for name in ["console_write", "console_read", "get_cmdline", "memory_end", "time_ns", "halt",
                 "channel_mailbox"] {
        js::set(&kernel, name, export(name));
    }
    js::set(&kernel, "spawn_task", func(Closure::<dyn FnMut(u32, u32)>::new(
        move |task: u32, sp: u32| {
            let _ = spawn.call2(&JsValue::NULL, &task.into(), &sp.into());
        })));
    js::set(&kernel, "exit_task", func(Closure::<dyn FnMut()>::new(|| wasm_bindgen::throw_str(EXIT))));
    js::set(&kernel, "print_stack", func(Closure::<dyn FnMut()>::new(|| {
        web_sys::console::log_1(&js::get(&js_sys::Error::new("kernel stack"), "stack"));
    })));
    let line = export("user_line");
    js::set(&kernel, "user_load", func(Closure::<dyn FnMut(u32, u32) -> i32>::new(
        move |image: u32, len: u32| {
            assert!(worker, "exec on the page");
            let bytes = js::copy_from(image as *const u8, len);
            let result = WebAssembly::Module::new(&bytes).and_then(|m| {
                let linux = Object::new();
                USER.with_borrow(|u| js::set(&linux, "syscall", u.kernel_syscall.clone().unwrap()));
                let ksee = Object::new();
                js::set(&ksee, "line", line.clone());
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
    js::set(&ksee, "now", export("time_ns"));
    js::set(&ksee, "stop", export("stop"));
    let env = Object::new();
    js::set(&env, "memory", memory.clone());
    let imports = Object::new();
    js::set(&imports, "env", env);
    js::set(&imports, "kernel", kernel);
    js::set(&imports, "ksee", ksee);
    imports
}

/// Remember this Worker's kernel, for user programs' syscall import.
pub fn set_kernel(exports: &Object) {
    USER.with_borrow_mut(|u| u.kernel_syscall = Some(js::get(exports, "syscall").unchecked_into()));
}
