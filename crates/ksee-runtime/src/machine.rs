//! The page's API: boot a kernel (one instance, its tasks on this thread:
//! see sched.rs), drive it (console, syscalls, recording), and take
//! snapshots of its recording (see replay.rs).

use std::cell::RefCell;

use js_sys::{Function, Object, Promise, Uint8Array, WebAssembly};
use ksee_core::recording::Recording;
use ksee_core::tables::Tables;
use ksee_core::{trace, Memory};
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
        k(|k| {
            k.trace.set(trace);
            k.current.set(global("__current_task"));
            k.pending.set(global("wasm_irq_pending"));
            k.deadline.set(global("wasm_timer_deadline"));
            k.clock.set(0);
            k.waiting.set(Some(0));
            k.booting.set(true);
            k.quiet.set(false);
            k.halted.set(None);
        });
        crate::programs::instantiated(&kernel);
        sched::boot(&kernel)?;
        // (Quiet, with /init ready.)
        while !k(|k| k.runners.borrow().contains_key(&1)) {
            JsFuture::from(self.quiet()).await?;
        }
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
        // (What waits for the old machine gives up.)
        for (_, fail) in k(|k| k.quiet_waiters.take()) {
            let _ = fail.call1(&JsValue::NULL, &"the kernel started again".into());
        }
        k(|k| {
            k.console_out.borrow_mut().clear();
            k.doorbell.borrow_mut().clear();
            k.runners.borrow_mut().clear();
            k.calls.borrow_mut().clear();
            k.finished.borrow_mut().clear();
            k.made.borrow_mut().clear();
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

    // ---- Syscalls, made by the runners ----

    /// Have runner `tid` (1: /init; see user/init.c) make syscall `name`
    /// (e.g. "openat") with `args`: each a number, a string (as a
    /// NUL-terminated buffer), { bytes } (a buffer of them: an array or a
    /// Uint8Array), { size } (a buffer of that many zero bytes, for the
    /// call to fill), or { strings } (an array of char *, null for NULL:
    /// argv). The call's id: it finishes when it returns; or, when
    /// it blocks, later, as another call wakes it (see quiet). Recorded:
    /// from here until the kernel is quiet again (see sched::idle); so
    /// memory now is memory as the recording ends.
    pub fn call(&self, name: &str, args: Vec<JsValue>, tid: Option<u32>) -> Result<u32, JsValue> {
        let tid = tid.unwrap_or(1);
        let nr = tables().types.syscalls.iter().find(|(_, n)| n.as_str() == name).map(|(&nr, _)| nr)
            .ok_or_else(|| format!("no syscall {name}"))?;
        let Some(task) = k(|k| k.runners.borrow().get(&tid).copied()) else {
            return Err(format!("no runner {tid}").into());
        };
        if k(|k| k.calls.borrow().contains_key(&tid)) {
            return Err(format!("{tid} is still making its call").into());
        }
        let id = k(|k| k.asked.get() + 1);
        let c = pack(id, nr, &args)?;
        let trace = k(|k| k.trace.get());
        k(|k| {
            k.asked.set(id);
            k.calls.borrow_mut().insert(tid, c);
            // (Records' ids count the pairs of words ever written: see trace::ring.)
            k.made.borrow_mut().insert(id, (tid, nr, Direct.read_u32(trace) / 2));
            // (The page waits for the kernel now: see Kernel::waiting.)
            k.waiting.set(Some(k.clock.get()));
            // (Ring the runner: it waits for SIGUSR1.)
            k.doorbell.borrow_mut().push_back(task);
        });
        Direct.write_u32(trace + trace::ON, 1);
        sched::interrupt(IRQ_DOORBELL);
        Ok(id)
    }

    /// A promise of the kernel quiet (now, or once it is): { finished: [the
    /// calls finished since the last time: { id, tid, how ("returned",
    /// "exec": an execve, the new program running; "exited": its task),
    /// ret, buffers: [the bytes of each buffer argument, as the call left
    /// them (null for the others)] }], runners: [the runners' tids] }.
    pub fn quiet(&self) -> Promise {
        Promise::new(&mut |ok, fail| {
            k(|k| k.quiet_waiters.borrow_mut().push((ok, fail)));
            if k(|k| k.quiet.get()) {
                settle();
            }
        })
    }

    /// The runners' thread ids.
    pub fn runners(&self) -> Vec<u32> {
        k(|k| k.runners.borrow().keys().copied().collect())
    }
}

/// Call `id`, syscall `nr` with `args` (see Machine::call): each a number,
/// or a buffer (a string, { bytes }, { size }, { strings }) packed into its
/// data.
fn pack(id: u32, nr: u32, args: &[JsValue]) -> Result<Call, JsValue> {
    let mut c = Call { id, nr, data: vec![0; CALL_DATA], ..Default::default() };
    let mut used = 0usize;
    // (Bytes `b` into the data, 8-byte aligned: their offset there.)
    let mut put = |c: &mut Call, b: &[u8]| -> Result<u32, JsValue> {
        if used + b.len() > CALL_DATA {
            return Err("the call's buffers are too big".into());
        }
        c.data[used..used + b.len()].copy_from_slice(b);
        let at = used;
        used += (b.len() + 7) & !7;
        Ok(at as u32)
    };
    for (i, a) in args.iter().take(6).enumerate() {
        // { strings: [s, …, null] }: an array of char * (null: NULL), as argv.
        if let Ok(list) = js::get(a, "strings").dyn_into::<js_sys::Array>() {
            let mut words = Vec::new();
            for s in list.iter() {
                words.push(match s.as_string() {
                    Some(s) => Some(put(&mut c, &[s.as_bytes(), &[0]].concat())?),
                    None => None,
                });
            }
            let array: Vec<u8> = words.iter().flat_map(|w| w.unwrap_or(0).to_le_bytes()).collect();
            let at = put(&mut c, &array)?;
            c.relocs.extend(words.iter().enumerate().filter(|(_, w)| w.is_some()).map(|(k, _)| at + 4 * k as u32));
            c.args[i] = at;
            c.buffers |= 1 << i;
            c.lens[i] = array.len() as u32;
            continue;
        }
        let buffer: Option<Vec<u8>> = if let Some(s) = a.as_string() {
            Some([s.as_bytes(), &[0]].concat())
        } else if !js::get(a, "bytes").is_undefined() {
            Some(Uint8Array::new(&js::get(a, "bytes")).to_vec())
        } else {
            js::get(a, "size").as_f64().map(|n| vec![0; n as usize])
        };
        c.args[i] = match buffer {
            Some(b) => {
                c.buffers |= 1 << i;
                c.lens[i] = b.len() as u32;
                put(&mut c, &b)?
            }
            None => a.as_f64().ok_or("an argument: a number, a string, { bytes } or { size }")? as i64 as u32,
        };
    }
    Ok(c)
}

/// The kernel is quiet: settle what waits for it (see Machine::quiet).
pub(crate) fn settle() {
    let waiters = k(|k| k.quiet_waiters.take());
    if waiters.is_empty() {
        return;
    }
    let finished = js_sys::Array::new();
    for f in k(|k| k.finished.take()) {
        let o = Object::new();
        js::set(&o, "id", f.id);
        js::set(&o, "tid", f.tid);
        js::set(&o, "how", f.how);
        js::set(&o, "ret", f.ret as f64);
        let buffers = js_sys::Array::new();
        for b in &f.buffers {
            buffers.push(&b.as_deref().map_or(JsValue::NULL, |b| Uint8Array::from(b).into()));
        }
        js::set(&o, "buffers", buffers);
        finished.push(&o);
    }
    let news = Object::new();
    js::set(&news, "finished", finished);
    js::set(&news, "runners", k(|k| k.runners.borrow().keys().map(|&t| JsValue::from(t)).collect::<js_sys::Array>()));
    for (ok, _) in waiters {
        let _ = ok.call1(&JsValue::NULL, &news);
    }
}
