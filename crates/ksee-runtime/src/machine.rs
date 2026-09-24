//! The page's API: boot a kernel in Workers, and look at it.

use std::sync::atomic::Ordering;

use js_sys::{Function, Object, Uint8Array, WebAssembly};
use ksee_core::render::Renderer;
use ksee_core::view::Views;
use ksee_core::tables::Tables;
use ksee_core::{cfg, dump, syscalls, trace};
use serde_wasm_bindgen as _;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::imports;
use crate::js;
use crate::state::*;

/// A booted machine.
#[wasm_bindgen]
pub struct Machine {
    /// The page's kernel instance (it only reads the kernel).
    kernel: Object,
    workers: std::rc::Rc<std::cell::RefCell<Vec<web_sys::Worker>>>,
    syscalls: std::cell::RefCell<Option<Vec<syscalls::Syscall>>>,
}

fn to_js<T: serde::Serialize>(v: &T) -> JsValue {
    serde_wasm_bindgen::to_value(v).unwrap_or(JsValue::NULL)
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
        let workers = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let spawn = Machine::spawner(module.clone(), worker_url, workers.clone());
        let imports = imports::kernel(&wasm_bindgen::memory(), spawn.clone(), false);
        let inst: WebAssembly::Instance =
            JsFuture::from(WebAssembly::instantiate_module(&module, &imports)).await?.unchecked_into();
        let kernel = inst.exports();

        let global = |name: &str| {
            js::get(&js::get(&kernel, name), "value").as_f64().unwrap_or(0.0) as u32
        };
        let timer = Object::new();
        js::set(&timer, "role", "timer");
        js::set(&timer, "deadline", global("wasm_timer_deadline"));
        js::set(&timer, "pending", global("wasm_irq_pending"));
        spawn.call1(&JsValue::NULL, &timer)?;
        let boot = Object::new();
        js::set(&boot, "role", "boot");
        spawn.call1(&JsValue::NULL, &boot)?;
        Ok(Machine { kernel, workers, syscalls: Default::default() })
    }

    /// A function that starts a Worker for `msg` (a role, or a task that a
    /// Worker asked for) and keeps it, so that `halted` can stop it.
    fn spawner(module: WebAssembly::Module, url: String,
               workers: std::rc::Rc<std::cell::RefCell<Vec<web_sys::Worker>>>) -> Function {
        let this: std::rc::Rc<std::cell::RefCell<Option<Function>>> = Default::default();
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

    /// Input for the kernel's console.
    pub fn type_in(&self, text: &str) {
        CONSOLE_IN.put(text.as_bytes());
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

    fn ring(&self) -> u32 {
        let f: Function = js::get(&self.kernel, "ksee_trace").unchecked_into();
        f.call0(&JsValue::NULL).ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as u32
    }

    /// Decode the trace ring (again). The views below use the last snapshot.
    pub fn snapshot(&self) {
        let t = tables();
        let entry = t.find(syscalls::ENTRY).unwrap_or(0);
        let calls = syscalls::split(entry, trace::ring(&Direct, self.ring(), t.ring_words));
        *self.syscalls.borrow_mut() = Some(calls);
    }

    fn with_syscalls<T>(&self, f: impl FnOnce(&[syscalls::Syscall]) -> T) -> T {
        if self.syscalls.borrow().is_none() {
            self.snapshot();
        }
        f(self.syscalls.borrow().as_deref().unwrap_or(&[]))
    }

    fn views(&self) -> Views<'static> {
        Views { tables: tables(), tasks: &live_task }
    }

    /// The syscalls in the trace (a list of SyscallSummary, see ksee_core::view).
    pub fn syscalls(&self) -> JsValue {
        let v = self.views();
        self.with_syscalls(|calls| to_js(&calls.iter().enumerate().map(|(i, s)| v.summary(i, s)).collect::<Vec<_>>()))
    }

    /// What syscall `n` ran, as runs of blocks per function.
    pub fn runs(&self, n: usize) -> JsValue {
        self.with_syscalls(|calls| to_js(&calls.get(n).map(|s| self.views().runs(s)).unwrap_or_default()))
    }

    /// The instrumented functions called `name` (usually one), with block
    /// and edge counts from syscall `n` (or the whole trace).
    pub fn functions(&self, name: &str, n: Option<usize>) -> JsValue {
        let t = tables();
        let ids: Vec<u32> = t.functions.iter().filter(|(_, f)| f.name == name).map(|(&id, _)| id).collect();
        let keep = |f: u32| ids.contains(&f);
        let cov = match n {
            None => cfg::coverage(t, trace::ring(&Direct, self.ring(), t.ring_words), keep),
            Some(n) => self.with_syscalls(|calls| {
                let blocks = calls.get(n).map(|s| s.blocks.clone()).unwrap_or_default();
                cfg::coverage(t, blocks.into_iter().map(|(func, n)| trace::Record::Block { func, n }), keep)
            }),
        };
        let v = self.views();
        to_js(&ids.iter().filter_map(|&id| v.function(id, &cov)).collect::<Vec<_>>())
    }

    /// The value of the type named `name` at `addr` (an Object, see
    /// ksee_core::view), aggregates expanded `depth` levels.
    pub fn object(&self, name: &str, addr: u32, depth: Option<u32>) -> JsValue {
        let types = &tables().types;
        match types.find(name) {
            Some(id) => to_js(&types.object(&Direct, name, addr, id, depth.unwrap_or(1))),
            None => JsValue::NULL,
        }
    }

    /// Like `object`, by type id (an Object's `expand` or `target`).
    pub fn object_by_id(&self, label: &str, ty: u32, addr: u32, depth: Option<u32>) -> JsValue {
        to_js(&tables().types.object(&Direct, label, addr, ty, depth.unwrap_or(1)))
    }

    /// The task_struct of `pid`, among the tasks the trace mentions.
    pub fn task_address(&self, pid: i32) -> Option<u32> {
        let t = tables();
        dump::tasks_in(t, trace::ring(&Direct, self.ring(), t.ring_words)).into_iter()
            .find(|&p| live_task(p).is_some_and(|task| task.pid == pid))
    }

    // ---- Text views (for tests and the CLI's format) ----

    /// The syscalls, one per line (`#n ...`).
    pub fn view_syscalls(&self) -> String {
        let r = Renderer { tables: tables(), tasks: &live_task };
        self.with_syscalls(|calls| calls.iter().enumerate()
            .map(|(i, s)| format!("#{i} {}\n", r.syscall_text(s, false))).collect())
    }

    /// Syscall `n` with the blocks it ran.
    pub fn view_syscall(&self, n: usize) -> String {
        let r = Renderer { tables: tables(), tasks: &live_task };
        self.with_syscalls(|calls| calls.get(n).map_or(String::new(), |s| r.syscall_text(s, true) + "\n"))
    }

    /// The trace dump (read it with ./ksee.sh).
    pub fn dump(&self) -> Vec<u8> {
        let mut out = Vec::new();
        dump::write(tables(), &Direct, self.ring(), &mut |b| out.extend_from_slice(b));
        out
    }
}
