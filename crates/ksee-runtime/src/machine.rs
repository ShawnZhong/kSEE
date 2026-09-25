//! The page's API: boot a kernel (one instance, its tasks on this thread:
//! see sched.rs), drive it (console, syscalls, recording), and take
//! snapshots of its recording (see replay.rs).

use std::cell::RefCell;

use js_sys::{Function, Promise, Uint8Array, WebAssembly};
use ksee_core::recording::Recording;
use ksee_core::tables::Tables;
use ksee_core::trace;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::imports;
use crate::js;
use crate::sched;
use crate::state::*;

/// A booted machine. Its views of the recording are in replay.rs.
#[wasm_bindgen]
pub struct Machine {
    /// The kernel, compiled (a new instance of it each boot); and the
    /// initramfs (at the top of RAM).
    module: WebAssembly::Module,
    initrd: Vec<u8>,
    /// The last snapshot of the trace.
    pub(crate) rec: RefCell<Recording>,
    /// The moment the views show memory at (a record), or None: now.
    pub(crate) past: RefCell<Option<usize>>,
}

/// Where the initramfs goes: at the top of RAM (page-aligned).
fn initrd_at(ram: u32, initrd: &[u8]) -> u32 {
    (ram - initrd.len() as u32) & !0xfff
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
    /// Boot a kernel. `config`: { kernel, initrd?, cmdline?, ram } (URLs
    /// of vmlinux and an initramfs; RAM in bytes).
    pub async fn start(config: JsValue) -> Result<Machine, JsValue> {
        let s = |k: &str| js::get(&config, k).as_string();
        let url = s("kernel").ok_or("no kernel")?;
        let ram = js::get(&config, "ram").as_f64().ok_or("no ram")? as u32;
        // The kernel's RAM: this module's memory, grown to it first (the
        // runtime's heap grows above it, its data and stack are below).
        let memory: WebAssembly::Memory = wasm_bindgen::memory().unchecked_into();
        let have = Uint8Array::new(&memory.buffer()).length();
        if have < ram {
            memory.grow((ram - have).div_ceil(65536));
        }
        k(|k| k.ram.set(ram));

        let bytes = fetch_bytes(&url).await?;
        let module: WebAssembly::Module =
            JsFuture::from(WebAssembly::compile(&bytes)).await?.unchecked_into();
        let tables = Tables::parse(&section(&module, "ksee.probes"), &section(&module, "ksee.types"));
        TABLES.set(tables).map_err(|_| "one machine per page")?;

        let mut cmdline = s("cmdline").unwrap_or_default();
        // The initramfs goes at the top of RAM; the kernel reserves it.
        let initrd = match s("initrd") {
            Some(url) => fetch_bytes(&url).await?.to_vec(),
            None => Vec::new(),
        };
        if !initrd.is_empty() {
            cmdline += &format!(" initrd={:#x},{}", initrd_at(ram, &initrd), initrd.len());
        }
        k(|k| *k.cmdline.borrow_mut() = cmdline.trim().as_bytes().to_vec());
        let m = Machine { module, initrd, rec: Default::default(), past: Default::default() };
        m.boot().await?;
        Ok(m)
    }

    /// Boot: kernel memory as new (zeros, and the initramfs), a new kernel
    /// instance (instantiating initializes its data), and its boot CPU; done
    /// once /init is ready for calls.
    async fn boot(&self) -> Result<(), JsValue> {
        let ram = k(|k| k.ram.get());
        unsafe { core::ptr::write_bytes(KERNEL_BASE as *mut u8, 0, (ram - KERNEL_BASE) as usize) };
        if !self.initrd.is_empty() {
            let at = initrd_at(ram, &self.initrd) as usize;
            unsafe { core::ptr::copy_nonoverlapping(self.initrd.as_ptr(), at as *mut u8, self.initrd.len()) };
        }
        let inst: WebAssembly::Instance =
            JsFuture::from(WebAssembly::instantiate_module(&self.module, &imports::kernel())).await?.unchecked_into();
        let kernel = inst.exports();
        let global = |name: &str| js::get(&js::get(&kernel, name), "value").as_f64().unwrap_or(0.0) as u32;
        let trace: Function = js::get(&kernel, "ksee_trace").unchecked_into();
        let trace = trace.call0(&JsValue::NULL)?.as_f64().unwrap_or(0.0) as u32;
        let ready = Promise::new(&mut |ok, fail| k(|k| {
            k.trace.set(trace);
            k.current.set(global("__current_task"));
            k.pending.set(global("wasm_irq_pending"));
            k.deadline.set(global("wasm_timer_deadline"));
            k.clock.set(0);
            k.waiting.set(Some(0));
            k.halted.set(None);
            *k.ready.borrow_mut() = Some((ok, fail));
        }));
        imports::instantiated(&kernel);
        sched::boot(&kernel)?;
        JsFuture::from(ready).await?;
        Ok(())
    }

    /// Start the kernel again, from scratch: the machine stopped (its tasks
    /// dropped, and what it had under way let end: it only suspends, for
    /// ever), the runtime's state emptied, and a new boot.
    pub async fn restart(&self) -> Result<(), JsValue> {
        sched::stop();
        JsFuture::from(js_sys::Promise::new(&mut |ok, _| {
            let set: Function = js::get(&js_sys::global(), "setTimeout").unchecked_into();
            let _ = set.call2(&JsValue::NULL, &ok, &0.into());
        })).await?;
        sched::stop();
        // (A call under way gives up.)
        if let Some(fail) = k(|k| k.call.take()).reject {
            let _ = fail.call1(&JsValue::NULL, &"the kernel started again".into());
        }
        k(|k| {
            k.console_out.borrow_mut().clear();
            k.console_in.borrow_mut().clear();
        });
        *self.rec.borrow_mut() = Default::default();
        *self.past.borrow_mut() = None;
        self.boot().await
    }

    /// Console output since the last call.
    pub fn console(&self) -> String {
        let out = k(|k| k.console_out.take());
        String::from_utf8_lossy(&out).into_owned()
    }

    /// The halt code once the kernel has halted.
    pub fn halted(&self) -> Option<i32> {
        k(|k| k.halted.get())
    }

    /// Decode the trace ring (again). The views use the last snapshot.
    pub fn snapshot(&self) {
        let t = tables();
        let (records, ids) = trace::ring(&Direct, k(|k| k.trace.get()), t.ring_words);
        *self.rec.borrow_mut() = Recording::new(t, records, ids);
    }

    // ---- Syscalls, made by /init ----

    /// Have /init make syscall `name` (e.g. "openat") with `args` (see
    /// user/init.c): each a number, a string (as a NUL-terminated buffer),
    /// { bytes } (a buffer of them: an array or a Uint8Array), or { size }
    /// (a buffer of that many zero bytes, for the call to fill). A promise
    /// of { ret, buffers: [the bytes of each buffer argument, as the call
    /// left them (null for the others)] }. One call at a time, recorded:
    /// from here until the kernel idles again, waiting for the page (see
    /// sched::idle); so memory now is memory as the recording ends.
    pub fn call(&self, name: &str, args: Vec<JsValue>) -> Result<Promise, JsValue> {
        let nr = tables().types.syscalls.iter().find(|(_, n)| n.as_str() == name).map(|(&nr, _)| nr)
            .ok_or_else(|| format!("no syscall {name}"))?;
        if k(|k| k.call.borrow().asked) {
            return Err("a call is under way".into());
        }
        let mut c = Call { asked: true, nr, data: vec![0; CALL_DATA], ..Default::default() };
        let mut used = 0usize;
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
            c.args[i] = match buffer {
                Some(b) => {
                    if used + b.len() > CALL_DATA {
                        return Err("the call's buffers are too big".into());
                    }
                    c.data[used..used + b.len()].copy_from_slice(&b);
                    c.buffers |= 1 << i;
                    c.lens[i] = b.len() as u32;
                    let at = used;
                    used += (b.len() + 7) & !7; // (8-byte aligned)
                    at as u32
                }
                None => a.as_f64().ok_or("an argument: a number, a string, { bytes } or { size }")? as i64 as u32,
            };
        }
        let p = Promise::new(&mut |ok, fail| {
            c.resolve = Some(ok);
            c.reject = Some(fail);
        });
        // (The page waits for the kernel now: see Kernel::waiting.)
        k(|k| {
            *k.call.borrow_mut() = c;
            k.waiting.set(Some(k.clock.get()));
            // (The doorbell: /init waits reading its terminal.)
            k.console_in.borrow_mut().push_back(b'\n');
        });
        Direct.write_u32(k(|k| k.trace.get()) + trace::ON, 1);
        sched::interrupt(IRQ_CONSOLE);
        Ok(p)
    }
}
