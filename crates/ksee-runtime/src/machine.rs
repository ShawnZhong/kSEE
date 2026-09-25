//! The page's API: boot a kernel in Workers, drive it (console, programs,
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
        let imports = imports::kernel(&wasm_bindgen::memory(), spawn.clone(), false);
        let inst: WebAssembly::Instance =
            JsFuture::from(WebAssembly::instantiate_module(&module, &imports)).await?.unchecked_into();
        let kernel = inst.exports();

        let global = |name: &str| {
            js::get(&js::get(&kernel, name), "value").as_f64().unwrap_or(0.0) as u32
        };
        CURRENT_TASK.store(global("__current_task"), Ordering::Relaxed);
        PENDING.store(global("wasm_irq_pending"), Ordering::Relaxed);
        DEADLINE.store(global("wasm_timer_deadline"), Ordering::Relaxed);
        RUN.store(global("wasm_run"), Ordering::Relaxed);
        let trace: Function = js::get(&kernel, "ksee_trace").unchecked_into();
        // (The page's instance is not needed after this.)
        TRACE.store(trace.call0(&JsValue::NULL)?.as_f64().unwrap_or(0.0) as u32, Ordering::Relaxed);
        let timer = Object::new();
        js::set(&timer, "role", "timer");
        spawn.call1(&JsValue::NULL, &timer)?;
        let boot = Object::new();
        js::set(&boot, "role", "boot");
        spawn.call1(&JsValue::NULL, &boot)?;
        Ok(Machine { workers, rec: Default::default(), past: Default::default(), frozen: Default::default() })
    }

    /// A function that starts a Worker for `msg` (a role, or a task that a
    /// Worker asked for) and keeps it, so that `halted` can stop it.
    fn spawner(module: WebAssembly::Module, url: String,
               workers: Rc<RefCell<Vec<web_sys::Worker>>>) -> Function {
        let this: Rc<RefCell<Option<Function>>> = Default::default();
        let me = this.clone();
        let f = Closure::<dyn FnMut(JsValue)>::new(move |msg: JsValue| {
            let opts = web_sys::WorkerOptions::new();
            opts.set_type(web_sys::WorkerType::Module);
            let w = web_sys::Worker::new_with_options(&url, &opts).expect("worker");
            if js::get(&msg, "role").is_undefined() {
                js::set(&msg, "role", "task");
            }
            js::set(&msg, "kernel", module.clone());
            js::set(&msg, "runtime", wasm_bindgen::module());
            js::set(&msg, "memory", wasm_bindgen::memory());
            // A kernel Worker's messages ask for new tasks.
            let spawn = me.borrow().clone().unwrap();
            let onmessage = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
                let _ = spawn.call1(&JsValue::NULL, &e.data());
            });
            w.set_onmessage(Some(onmessage.into_js_value().unchecked_ref()));
            w.post_message(&msg).expect("post");
            workers.borrow_mut().push(w);
        })
        .into_js_value()
        .unchecked_into::<Function>();
        *this.borrow_mut() = Some(f.clone());
        f
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

    // ---- Running programs ----

    /// Ask the kernel to run program `path` (in the initramfs; see its
    /// arch/wasm/kernel/run.c). False if a program runs already.
    pub fn run(&self, path: &str) -> Result<bool, JsValue> {
        let run = RUN.load(Ordering::Relaxed);
        let state = unsafe { &*(run as *const AtomicU32) };
        if state.load(Ordering::Acquire) != RUN_IDLE {
            return Ok(false);
        }
        if path.len() >= RUN_PATH {
            return Err("path too long".into());
        }
        let buf = unsafe { core::slice::from_raw_parts_mut((run + 12) as *mut u8, RUN_PATH) };
        buf[..path.len()].copy_from_slice(path.as_bytes());
        buf[path.len()] = 0;
        state.store(RUN_REQUESTED, Ordering::Release);
        interrupt(IRQ_RUN);
        Ok(true)
    }

    /// Once the program `run` started has exited: { status (its exit
    /// status, or -errno), pid }; null before.
    pub fn result(&self) -> JsValue {
        let run = RUN.load(Ordering::Relaxed);
        let state = unsafe { &*(run as *const AtomicU32) };
        if state.load(Ordering::Acquire) != RUN_DONE {
            return JsValue::NULL;
        }
        let o = Object::new();
        js::set(&o, "status", Direct.read_u32(run + 4) as i32);
        js::set(&o, "pid", Direct.read_u32(run + 8) as i32);
        state.store(RUN_IDLE, Ordering::Release);
        o.into()
    }
}
