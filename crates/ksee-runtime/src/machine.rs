//! The page's API: boot a kernel in Workers, drive it (console, syscalls,
//! recording), and take snapshots of its recording (see replay.rs).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use js_sys::{Function, Object, Uint8Array, WebAssembly};
use ksee_core::recording::Recording;
use ksee_core::tables::Tables;
use ksee_core::{trace, Memory};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::imports;
use crate::js;
use crate::state::*;

/// A booted machine. Its views of the recording are in replay.rs.
#[wasm_bindgen]
pub struct Machine {
    workers: Rc<RefCell<Vec<web_sys::Worker>>>,
    /// Starts a Worker (see spawner).
    spawn: Rc<dyn Fn(&str)>,
    /// The kernel's memory before it first ran (its data initialized, the
    /// initramfs in place), from KERNEL_BASE: what restart puts back.
    pristine: Vec<u8>,
    /// The last snapshot of the trace.
    pub(crate) rec: RefCell<Recording>,
    /// The moment the views show memory at (a record), or None: now.
    pub(crate) past: RefCell<Option<usize>>,
    /// Kernel memory when recording stopped: memory as it was is that, the
    /// recorded stores undone (the kernel went on, unrecorded).
    pub(crate) frozen: RefCell<Option<Vec<u8>>>,
}

/// Kernel memory as it was when recording stopped.
pub(crate) struct Frozen<'a>(pub &'a [u8]);

impl Memory for Frozen<'_> {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        match self.0.get(addr as usize..addr as usize + buf.len()) {
            Some(b) => buf.copy_from_slice(b),
            None => Direct.read(addr, buf),
        }
    }
}

async fn fetch_bytes(url: &str) -> Result<Uint8Array, JsValue> {
    let fetch: Function = js::get(&js_sys::global(), "fetch").unchecked_into();
    let resp: web_sys::Response = JsFuture::from(js_sys::Promise::from(fetch.call1(&JsValue::NULL, &url.into())?))
        .await?.unchecked_into();
    if !resp.ok() {
        return Err(format!("{url}: {}", resp.status()).into());
    }
    Ok(Uint8Array::new(&JsFuture::from(resp.array_buffer()?).await?))
}

fn section(module: &WebAssembly::Module, name: &str) -> String {
    let mut text = Vec::new();
    for b in WebAssembly::Module::custom_sections(module, name).iter() {
        text.extend(Uint8Array::new(&b).to_vec());
    }
    String::from_utf8_lossy(&text).into_owned()
}

#[wasm_bindgen]
impl Machine {
    /// Boot a kernel. `config`: { kernel, initrd?, cmdline?, worker, ram }
    /// (URLs of vmlinux, an initramfs and web/worker.js; RAM in bytes, at
    /// most the shared memory's initial size).
    pub async fn start(config: JsValue) -> Result<Machine, JsValue> {
        let s = |k: &str| js::get(&config, k).as_string();
        let url = s("kernel").ok_or("no kernel")?;
        let ram = js::get(&config, "ram").as_f64().ok_or("no ram")? as u32;
        let worker_url = s("worker").ok_or("no worker")?;

        let bytes = fetch_bytes(&url).await?;
        let module: WebAssembly::Module =
            JsFuture::from(WebAssembly::compile(&bytes)).await?.unchecked_into();
        let tables = Tables::parse(&section(&module, "ksee.probes"), &section(&module, "ksee.types"));
        RING_WORDS.store(tables.ring_words, Ordering::Relaxed);
        TABLES.set(tables).map_err(|_| "one machine per page")?;
        MEMORY_END.store(ram, Ordering::Relaxed);
        EPOCH_MS.store(js::now_ms().to_bits(), Ordering::Relaxed);

        let mut cmdline = s("cmdline").unwrap_or_default();
        // The initramfs goes at the top of RAM; the kernel reserves it.
        if let Some(initrd) = s("initrd") {
            let cpio = fetch_bytes(&initrd).await?;
            let addr = (ram - cpio.length()) & !0xfff;
            js::copy_to(&cpio, addr as *mut u8);
            cmdline += &format!(" initrd={addr:#x},{}", cpio.length());
        }
        let _ = CMDLINE.set(cmdline.trim().as_bytes().to_vec());

        // Instantiating here runs the kernel's start function, which
        // initializes its data, before any Worker runs it (asynchronously:
        // browsers refuse large synchronous instantiation on the page).
        let workers = Rc::new(RefCell::new(Vec::new()));
        let spawn = Machine::spawner(module.clone(), worker_url, workers.clone());
        let imports = imports::kernel(&wasm_bindgen::memory(), false);
        let inst: WebAssembly::Instance =
            JsFuture::from(WebAssembly::instantiate_module(&module, &imports)).await?.unchecked_into();
        let kernel = inst.exports();

        let global = |name: &str| {
            js::get(&js::get(&kernel, name), "value").as_f64().unwrap_or(0.0) as u32
        };
        CURRENT_TASK.store(global("__current_task"), Ordering::Relaxed);
        PENDING.store(global("wasm_irq_pending"), Ordering::Relaxed);
        DEADLINE.store(global("wasm_timer_deadline"), Ordering::Relaxed);
        let trace: Function = js::get(&kernel, "ksee_trace").unchecked_into();
        // (The page's instance is not needed after this.)
        TRACE.store(trace.call0(&JsValue::NULL)?.as_f64().unwrap_or(0.0) as u32, Ordering::Relaxed);
        let mut pristine = vec![0u8; (ram - KERNEL_BASE) as usize];
        Direct.read(KERNEL_BASE, &mut pristine);
        spawn("timer");
        spawn("boot");
        Ok(Machine { workers, spawn, pristine, rec: Default::default(), past: Default::default(), frozen: Default::default() })
    }

    /// Start the kernel again, from scratch: its Workers stopped, its memory
    /// as before it first ran, the runtime's shared state emptied, and a new
    /// boot. (The page stays: no reload.)
    pub async fn restart(&self) -> Result<(), JsValue> {
        // (Any Worker that outlives terminate is stale now, and stops.)
        GEN.fetch_add(1, Ordering::AcqRel);
        for w in self.workers.borrow_mut().drain(..) {
            w.terminate();
        }
        // (A Worker stops soon after, not at once: let them.)
        let pause = js_sys::Promise::new(&mut |ok, _| {
            let set: Function = js::get(&js_sys::global(), "setTimeout").unchecked_into();
            let _ = set.call2(&JsValue::NULL, &ok, &50.into());
        });
        JsFuture::from(pause).await?;
        unsafe { core::ptr::copy_nonoverlapping(self.pristine.as_ptr(), KERNEL_BASE as *mut u8, self.pristine.len()) };
        SPAWN.reset();
        CALL.state.store(CALL_IDLE, Ordering::Release);
        CONSOLE_OUT.reset();
        CONSOLE_IN.reset();
        HALTED.store(0, Ordering::Release);
        EPOCH_MS.store(js::now_ms().to_bits(), Ordering::Relaxed);
        *self.rec.borrow_mut() = Default::default();
        *self.past.borrow_mut() = None;
        *self.frozen.borrow_mut() = None;
        (self.spawn)("timer");
        (self.spawn)("boot");
        Ok(())
    }

    /// A function that starts a Worker for `role` ("timer", "boot", or
    /// "task": new tasks, see worker.rs) and keeps it, so that `halted` can
    /// stop it. A task Worker's message asks for another task Worker.
    fn spawner(module: WebAssembly::Module, url: String,
               workers: Rc<RefCell<Vec<web_sys::Worker>>>) -> Rc<dyn Fn(&str)> {
        let this: Rc<RefCell<Option<Rc<dyn Fn(&str)>>>> = Default::default();
        let me = this.clone();
        let spawn: Rc<dyn Fn(&str)> = Rc::new(move |role: &str| {
            let opts = web_sys::WorkerOptions::new();
            opts.set_type(web_sys::WorkerType::Module);
            let w = web_sys::Worker::new_with_options(&url, &opts).expect("worker");
            let msg = Object::new();
            js::set(&msg, "role", role);
            js::set(&msg, "kernel", module.clone());
            js::set(&msg, "runtime", wasm_bindgen::module());
            js::set(&msg, "memory", wasm_bindgen::memory());
            let more = me.borrow().clone().unwrap();
            let onmessage = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |_: web_sys::MessageEvent| more("task"));
            w.set_onmessage(Some(onmessage.into_js_value().unchecked_ref()));
            w.post_message(&msg).expect("post");
            workers.borrow_mut().push(w);
        });
        *this.borrow_mut() = Some(spawn.clone());
        spawn
    }

    /// Console output since the last call.
    pub fn console(&self) -> String {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = CONSOLE_OUT.take(&mut buf);
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// The halt code once the kernel has halted (its Workers are stopped).
    pub fn halted(&self) -> Option<i32> {
        let h = HALTED.load(Ordering::Acquire);
        if h == 0 {
            return None;
        }
        for w in self.workers.borrow_mut().drain(..) {
            w.terminate();
        }
        Some(h as i32 - 1)
    }

    /// Decode the trace ring (again). The views use the last snapshot.
    pub fn snapshot(&self) {
        let t = tables();
        let (records, ids) = trace::ring(&Direct, TRACE.load(Ordering::Relaxed), t.ring_words);
        *self.rec.borrow_mut() = Recording::new(t, records, ids);
    }

    /// Record the trace (on) or not. Stopping takes a snapshot, and keeps
    /// kernel memory as it is then: memory as it was in the recording is
    /// that, the recorded stores undone (the kernel goes on, unrecorded).
    pub fn record(&self, on: bool) {
        let word = unsafe { &*((TRACE.load(Ordering::Relaxed) + trace::ON) as *const AtomicU32) };
        if on {
            *self.frozen.borrow_mut() = None;
            word.store(1, Ordering::Release);
            // Who runs now (the trace names tasks by their switches).
            imports::task_record();
            return;
        }
        word.store(0, Ordering::Release);
        self.snapshot();
        let mut ram = vec![0u8; MEMORY_END.load(Ordering::Relaxed) as usize];
        // (Not from address 0: a null pointer, to Rust. The kernel's memory
        // starts well above it anyway.)
        Direct.read(8, &mut ram[8..]);
        *self.frozen.borrow_mut() = Some(ram);
    }

    /// Whether the trace records.
    pub fn recording(&self) -> bool {
        recording()
    }

    // ---- Syscalls, made by /init ----

    /// Ask /init to make syscall `nr` with `args` (user/init.c): each a
    /// number, a string (as a NUL-terminated buffer), { bytes } (a buffer
    /// of them: an array or a Uint8Array), or { size } (a buffer of that
    /// many zero bytes, for the call to fill). False if a call is under way
    /// already.
    pub fn call(&self, nr: u32, args: Vec<JsValue>) -> Result<bool, JsValue> {
        if CALL.state.load(Ordering::Acquire) != CALL_IDLE {
            return Ok(false);
        }
        let data = CALL.data();
        data.fill(0);
        let (mut used, mut buffers) = (0usize, 0u32);
        for (i, a) in args.iter().take(6).enumerate() {
            let buffer: Option<Vec<u8>> = if let Some(s) = a.as_string() {
                Some([s.as_bytes(), &[0]].concat())
            } else if !js::get(a, "bytes").is_undefined() {
                Some(Uint8Array::new(&js::get(a, "bytes")).to_vec())
            } else if let Some(n) = js::get(a, "size").as_f64() {
                Some(vec![0; n as usize])
            } else {
                None
            };
            let v = match buffer {
                Some(b) => {
                    if used + b.len() > CALL_DATA {
                        return Err("the call's buffers are too big".into());
                    }
                    let at = used;
                    data[at..at + b.len()].copy_from_slice(&b);
                    buffers |= 1 << i;
                    CALL.lens[i].store(b.len() as u32, Ordering::Relaxed);
                    used += (b.len() + 7) & !7; // (8-byte aligned)
                    at as u32
                }
                None => a.as_f64().ok_or("an argument: a number, a string, { bytes } or { size }")? as i64 as u32,
            };
            CALL.args[i].store(v, Ordering::Relaxed);
        }
        for i in args.len()..6 {
            CALL.args[i].store(0, Ordering::Relaxed);
        }
        CALL.nr.store(nr, Ordering::Relaxed);
        CALL.buffers.store(buffers, Ordering::Relaxed);
        CALL.state.store(CALL_ASKED, Ordering::Release);
        // (The doorbell: /init waits reading its terminal.)
        CONSOLE_IN.put(b"\n");
        interrupt(IRQ_CONSOLE);
        Ok(true)
    }

    /// Once the call is made: { ret, buffers: [the bytes of each buffer
    /// argument, as the call left them (null for the others)] }; null before.
    pub fn call_result(&self) -> JsValue {
        if CALL.state.load(Ordering::Acquire) != CALL_DONE {
            return JsValue::NULL;
        }
        let o = Object::new();
        js::set(&o, "ret", CALL.ret.load(Ordering::Relaxed) as i64 as f64);
        let buffers = js_sys::Array::new();
        let bits = CALL.buffers.load(Ordering::Relaxed);
        for i in 0..6 {
            if bits >> i & 1 == 0 {
                buffers.push(&JsValue::NULL);
                continue;
            }
            let (at, n) = (CALL.args[i].load(Ordering::Relaxed) as usize, CALL.lens[i].load(Ordering::Relaxed) as usize);
            buffers.push(&Uint8Array::from(&CALL.data()[at..at + n]));
        }
        js::set(&o, "buffers", buffers);
        CALL.state.store(CALL_IDLE, Ordering::Release);
        o.into()
    }

}
