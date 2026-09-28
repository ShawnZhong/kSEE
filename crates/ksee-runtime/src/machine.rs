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
            k.requests.borrow_mut().clear();
            k.heap_of.borrow_mut().clear();
            k.heaps.borrow_mut().clear();
            k.scratch.borrow_mut().clear();
            k.calls.borrow_mut().clear();
            k.finished.borrow_mut().clear();
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
        let (records, ids, args) = trace::ring(&Direct, k(|k| k.trace.get()), t.ring_words);
        *self.rec.borrow_mut() = Recording::new(t, records, ids, args);
    }

    // ---- Syscalls, made by the runners ----

    /// Have runner `tid` (1: /init; see user/init.c) make syscall `name`
    /// (e.g. "openat") with `args`: each a number, a string (as a
    /// NUL-terminated buffer), { bytes } (a buffer of them: an array or a
    /// Uint8Array), { size } (a buffer of that many zero bytes, for the
    /// call to fill), or { strings } (an array of char *, null for NULL:
    /// argv). The call's id: it finishes when it returns; or, when
    /// it blocks, later, as another call wakes it (see quiet). Recorded
    /// from the runner's begin to its end (user/init.c): the syscall,
    /// entry to return, and whatever the kernel does meanwhile.
    pub fn call(&self, name: &str, args: Vec<JsValue>, tid: Option<u32>) -> Result<u32, JsValue> {
        let tid = tid.unwrap_or(1);
        let nr = tables().types.syscalls.iter().find(|(_, n)| n.as_str() == name).map(|(&nr, _)| nr)
            .ok_or_else(|| format!("no syscall {name}"))?;
        let Some((task, req)) = k(|k| Some((*k.runners.borrow().get(&tid)?, *k.requests.borrow().get(&tid)?))) else {
            return Err(format!("no runner {tid}").into());
        };
        if k(|k| k.in_flight(tid).is_some()) {
            return Err(format!("{tid} is still making its call").into());
        }
        let id = k(|k| k.asked.get() + 1);
        // (Its strings and buffers: in the runner's room, in its program's
        // memory, taken on its first call.)
        let scratch = match k(|k| k.scratch.borrow().get(&tid).copied()) {
            Some(at) => at,
            None => {
                let at = alloc(tid, SCRATCH)?;
                k(|k| k.scratch.borrow_mut().insert(tid, at));
                at
            }
        };
        let (mut c, data) = pack(id, &args, scratch)?;
        (c.tid, c.nr) = (tid, nr);
        let head_args = c.args;
        let trace = k(|k| k.trace.get());
        k(|k| {
            k.asked.set(id);
            k.calls.borrow_mut().insert(id, c);
            // (The page waits for the kernel now: see Kernel::waiting.)
            k.waiting.set(Some(k.clock.get()));
            // (Ring the runner: it waits on the request's number.)
            k.doorbell.borrow_mut().push_back(task);
        });
        // (The call into the runner's request, in its memory, which is
        // kernel memory: recorded; its number last, the word waited on.)
        let mut head = [0u8; REQUEST as usize];
        head[0..4].copy_from_slice(&nr.to_le_bytes());
        for (i, a) in head_args.iter().enumerate() {
            head[4 + 4 * i..8 + 4 * i].copy_from_slice(&a.to_le_bytes());
        }
        if Direct.read_u32(trace + trace::ON) == 0 {
            Direct.write_u32(trace + trace::ON, 2);
        }
        imports::log(req, REQUEST, 0);
        imports::log(scratch, data.len() as u32, 0);
        unsafe {
            core::ptr::copy_nonoverlapping(head[4..].as_ptr(), (req + 4) as *mut u8, head.len() - 4);
            core::ptr::copy_nonoverlapping(data.as_ptr(), scratch as *mut u8, data.len());
        }
        Direct.write_u32(req, nr);

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

    /// Declare a variable of the program runner `tid` makes calls for:
    /// `text` as in C (`char buf[64]`, `struct __kernel_timespec ts = {0,
    /// 30000000}`: see ksee_core::args's Types::declare; `known` as for
    /// arg), in its process's own memory, as it starts (the host's writes
    /// there recorded, as a call's are). { name, addr, ty, count, size (an
    /// element's) }: its name
    /// then stands for its address in the calls after.
    pub fn declare(&self, tid: u32, text: &str, known: JsValue) -> Result<JsValue, JsValue> {
        let d = tables().types.declare(text, &crate::replay::lookup(&known))?;
        let addr = alloc(tid, d.bytes.len() as u32).map_err(|e| format!("{}: {e}", d.name))?;
        let trace = k(|k| k.trace.get());
        if Direct.read_u32(trace + trace::ON) == 0 {
            Direct.write_u32(trace + trace::ON, 2);
        }
        imports::log(addr, d.bytes.len() as u32, 0);
        unsafe { core::ptr::copy_nonoverlapping(d.bytes.as_ptr(), addr as *mut u8, d.bytes.len()) };
        let o = Object::new();
        js::set(&o, "name", d.name.as_str());
        js::set(&o, "addr", addr);
        js::set(&o, "ty", d.ty);
        js::set(&o, "count", d.count.map_or(JsValue::NULL, JsValue::from));
        js::set(&o, "size", tables().types.size_of(d.ty));
        Ok(o.into())
    }

    /// Whether `text` declares a variable (see declare): its name, or an
    /// error; nothing made.
    pub fn declaration(&self, text: &str, known: JsValue) -> Result<String, JsValue> {
        Ok(tables().types.declare(text, &crate::replay::lookup(&known))?.name)
    }

    /// A program's variable (of type `ty`, an array of `count` if given, at
    /// `addr`: see declare) as it is now, as C would write it: `"hello"`,
    /// `{3, 4}`, `{tv_sec = 0, tv_nsec = 30000000}`.
    pub fn show(&self, ty: u32, addr: u32, count: Option<u32>) -> String {
        tables().types.show(&Direct, addr, ty, count)
    }

    /// The runners' thread ids.
    pub fn runners(&self) -> Vec<u32> {
        k(|k| k.runners.borrow().keys().copied().collect())
    }
}

/// `len` bytes of the memory of runner `tid`'s program (user/init.c's heap:
/// its process's): their address.
fn alloc(tid: u32, len: u32) -> Result<u32, String> {
    k(|k| {
        let base = *k.heap_of.borrow().get(&tid).ok_or_else(|| format!("no runner {tid}"))?;
        let mut heaps = k.heaps.borrow_mut();
        let (next, end) = heaps.get_mut(&base).ok_or("no memory")?;
        let at = (*next + 15) & !15;
        if at + len > *end {
            return Err("the program's memory is full".into());
        }
        *next = at + len;
        Ok(at)
    })
}

/// Call `id` with `args` (see Machine::call): each a number, or a buffer
/// (a string, { bytes }, { size }, { strings }) packed into the data (its
/// bytes, to go at `base` in the runner's request: a buffer argument is
/// its address there).
fn pack(id: u32, args: &[JsValue], base: u32) -> Result<(Call, Vec<u8>), JsValue> {
    let mut c = Call { id, ..Default::default() };
    let mut data = Vec::new();
    // (Bytes `b` into the data, 8-byte aligned: their address.)
    let mut put = |b: &[u8]| -> Result<u32, JsValue> {
        if data.len() + b.len() > SCRATCH as usize {
            return Err("the call's buffers are too big".into());
        }
        let at = base + data.len() as u32;
        data.extend_from_slice(b);
        data.resize((data.len() + 7) & !7, 0);
        Ok(at)
    };
    for (i, a) in args.iter().take(6).enumerate() {
        // { strings: [s, …, null] }: an array of char * (null: NULL), as argv.
        if let Ok(list) = js::get(a, "strings").dyn_into::<js_sys::Array>() {
            let mut words = Vec::new();
            for s in list.iter() {
                words.push(match s.as_string() {
                    Some(s) => put(&[s.as_bytes(), &[0]].concat())?,
                    None => 0,
                });
            }
            let array: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
            c.args[i] = put(&array)?;
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
                put(&b)?
            }
            None => a.as_f64().ok_or("an argument: a number, a string, { bytes } or { size }")? as i64 as u32,
        };
    }
    Ok((c, data))
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
