//! The page's API: boot a kernel (one instance, its tasks on this thread:
//! see sched.rs), drive it (console, syscalls, recording), and take
//! snapshots of its recording (see replay.rs).

use std::cell::RefCell;

use js_sys::{Function, Object, Promise, Uint8Array, WebAssembly};
use ksee_core::recording::Recording;
use ksee_core::tables::Tables;
use ksee_core::program::{Arg, Sym};
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
    /// once the program (user/prog.c) is ready for calls.
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
        // (Quiet, with the program ready.)
        while k(|k| k.main.get()).is_none() {
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
            k.main.set(None);
            k.requests.borrow_mut().clear();
            k.heap_of.borrow_mut().clear();
            k.heaps.borrow_mut().clear();
            k.calls.borrow_mut().clear();
            k.finished.borrow_mut().clear();
            k.names.borrow_mut().clone_from(&Default::default());
            k.lines.set(0);
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

    // ---- The page's program, a line at a time ----

    /// Make line `text` of the program (ksee_core::program::statement), the
    /// next: a variable declared, in its runner's process's own memory ({
    /// decl: { name, addr, ty, count, size (an element's) } }: its name
    /// then its address); memory written ({ assigned: what it holds then,
    /// as C writes it }); or a syscall asked of its runner ({ id, tid }: it
    /// finishes when it returns, or, when it blocks, later, as another call
    /// wakes it: see quiet). The host's writes are recorded, as a call's
    /// are; a call from its runner's begin to its end (user/prog.c): the
    /// syscall, entry to return, and whatever the kernel does meanwhile.
    pub fn run(&self, text: &str) -> Result<JsValue, JsValue> {
        let line = k(|k| k.lines.get());
        let (tid, plan) = plan(text, line)?;
        let o = Object::new();
        match plan {
            Plan::Decl(d) => {
                let addr = alloc(tid, d.bytes.len() as u32).map_err(|e| format!("{}: {e}", d.name))?;
                write_logged(addr, &d.bytes);
                k(|k| k.names.borrow_mut().set(&d.name, line, Sym::Var { addr, ty: d.ty, count: d.count }));
                let v = Object::new();
                js::set(&v, "name", d.name.as_str());
                js::set(&v, "addr", addr);
                js::set(&v, "ty", d.ty);
                js::set(&v, "count", d.count.map_or(JsValue::NULL, JsValue::from));
                js::set(&v, "size", tables().types.size_of(d.ty));
                js::set(&o, "decl", v);
            }
            Plan::Assign { addr, ty, count, bytes } => {
                write_logged(addr, &bytes);
                js::set(&o, "assigned", tables().types.show(&Direct, addr, ty, count));
            }
            Plan::Call { nr, args, result } => {
                js::set(&o, "id", call(tid, nr, &args, line, result)?);
                js::set(&o, "tid", tid);
            }
        }
        k(|k| k.lines.set(line + 1));
        Ok(o.into())
    }

    /// Line `text` (see run) as it would be made as line `line`, with the
    /// names of the lines before it: its statement (Machine::statement),
    /// or why it cannot be made; nothing made.
    pub fn check(&self, text: &str, line: usize) -> Result<JsValue, JsValue> {
        plan(text, line)?;
        self.statement(text)
    }

    /// The names line `line` sees (see run): [{ name, value, pointer } (a
    /// result), or { name, addr, ty, count, size } (a variable: `size` all
    /// of it)].
    pub fn names_before(&self, line: usize) -> js_sys::Array {
        k(|k| k.names.borrow().before(line).into_iter().map(|(name, sym)| {
            let o = Object::new();
            js::set(&o, "name", name);
            match *sym {
                Sym::Value { v, to } => {
                    js::set(&o, "value", v as f64);
                    js::set(&o, "pointer", to.is_some());
                }
                Sym::Var { addr, ty, count } => {
                    js::set(&o, "addr", addr);
                    js::set(&o, "ty", ty);
                    js::set(&o, "count", count.map_or(JsValue::NULL, JsValue::from));
                    js::set(&o, "size", tables().types.size_of(ty) * count.unwrap_or(1));
                }
            }
            JsValue::from(o)
        }).collect())
    }

    /// A promise of the kernel quiet (now, or once it is): { finished: [the
    /// calls finished since the last time: { id, tid, how ("returned",
    /// "exec": an execve, the new program running; "exited": its task),
    /// ret }], runners: [the runners' tids] }.
    pub fn quiet(&self) -> Promise {
        Promise::new(&mut |ok, fail| {
            k(|k| k.quiet_waiters.borrow_mut().push((ok, fail)));
            if k(|k| k.quiet.get()) {
                settle();
            }
        })
    }

    /// A program's variable (of type `ty`, an array of `count` if given, at
    /// `addr`: see run) as it is now, as C would write it: `"hello"`,
    /// `{3, 4}`, `{tv_sec = 0, tv_nsec = 30000000}`.
    pub fn show(&self, ty: u32, addr: u32, count: Option<u32>) -> String {
        tables().types.show(&Direct, addr, ty, count)
    }

    /// The `len` bytes at `addr`, passed as argument `k` of syscall `call`,
    /// as the records its parameter's type says they are, if they are of
    /// variable length (Types::records: getdents64's entries); else null.
    pub fn as_records(&self, call: &str, k: usize, addr: u32, len: u32) -> Option<String> {
        use ksee_core::types::Kind;
        let types = &tables().types;
        let &(_, ty) = types.syscall_params(call)?.get(k)?;
        let Kind::Pointer(to) = types.get(types.resolve(ty)).kind else { return None };
        types.records(&Direct, addr, len, to)
    }

    /// The program's own runner (user/prog.c's main thread), its tid: the
    /// one the page's calls are made by unless they say another.
    pub fn main(&self) -> Option<u32> {
        k(|k| k.main.get())
    }

    /// The runners' thread ids.
    pub fn runners(&self) -> Vec<u32> {
        k(|k| k.runners.borrow().keys().copied().collect())
    }
}

/// Write `bytes` at `addr` into kernel memory, as the host: recorded (as a
/// call's writes into its request are), so memory as it was stays exact.
fn write_logged(addr: u32, bytes: &[u8]) {
    let trace = k(|k| k.trace.get());
    // (Stores recorded from the first write of the host's on, a variable
    // declared before any call (see programs::on_off: after the boot,
    // what it left for later runs unrecorded): memory as it was exact from
    // there.)
    if Direct.read_u32(trace + trace::ON) == 0 {
        Direct.write_u32(trace + trace::ON, 2);
    }
    imports::log(addr, bytes.len() as u32, 0, 0);
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len()) };
}

/// `len` bytes of the memory of runner `tid`'s program (user/prog.c's heap:
/// its process's): their address, on 16 bytes; a page or more, on a page
/// (as mmap gives it: memory the kernel can pin, a ring's).
fn alloc(tid: u32, len: u32) -> Result<u32, String> {
    k(|k| {
        let base = *k.heap_of.borrow().get(&tid).ok_or_else(|| format!("no runner {tid}"))?;
        let mut heaps = k.heaps.borrow_mut();
        let (next, end) = heaps.get_mut(&base).ok_or("no memory")?;
        let page = ksee_core::arch::PAGE;
        let align = if len >= page { page } else { 16 };
        let at = (*next + align - 1) & !(align - 1);
        if at + len > *end {
            return Err("the program's memory is full".into());
        }
        *next = at + len;
        Ok(at)
    })
}

/// What a line of the program makes (see Machine::run), checked: a
/// variable (its bytes, as it starts), memory written, or a syscall.
enum Plan {
    Decl(ksee_core::program::Decl),
    Assign { addr: u32, ty: u32, count: Option<u32>, bytes: Vec<u8> },
    Call { nr: u32, args: Vec<Arg>, result: Option<(String, String)> },
}

/// Line `text` as line `line` of the program: its runner (a thread id, or
/// a result's name; else the program's own, Machine::main), and what it
/// makes, with the names the lines before it made.
fn plan(text: &str, line: usize) -> Result<(u32, Plan), String> {
    use ksee_core::program::{expression, lvalue, statement, typed, value, Line};
    let (st, types, env) = (statement(text)?, &tables().types, Seen(line));
    let tid = match &st.runner {
        None => k(|k| k.main.get()).ok_or("the program is not ready")?,
        Some(r) => value(&expression(r)?, Some(types), &env)? as u32,
    };
    if !k(|k| k.runners.borrow().contains_key(&tid)) {
        return Err(format!("no runner {tid}: the runners are {:?}", k(|k| k.runners.borrow().keys().copied().collect::<Vec<_>>())));
    }
    Ok((tid, match st.line {
        Line::Decl { .. } => Plan::Decl(types.declare(&st.line, &env)?),
        Line::Assign { target, value } => {
            // (As a declaration's value: an expression, "…" or {…}; an array's, whole.)
            let (addr, ty, count) = lvalue(&target, types, &env)?;
            Plan::Assign { addr, ty, count, bytes: types.assign(ty, count, &value, &env)? }
        }
        Line::Call { result, call, args, texts } => {
            let nr = types.syscalls.iter().find(|(_, n)| **n == call).map(|(&nr, _)| nr)
                .ok_or_else(|| format!("this kernel has no syscall {call}"))?;
            let params = types.syscall_params(&call).cloned().unwrap_or_default();
            if texts.len() > params.len() {
                return Err(format!("{call}: {} arguments, not {}", params.len(), texts.len()));
            }
            let args = args.iter().enumerate().map(|(k, a)| types.arg(a, params.get(k).map_or(0, |p| p.1), &env))
                .collect::<Result<Vec<_>, _>>()?;
            let result = match result {
                Some((ty, name)) => {
                    typed(0, &ty.text(), types).ok_or_else(|| format!("{name}: no type {} in the kernel", ty.text()))?;
                    Some((name, ty.text()))
                }
                None => None,
            };
            Plan::Call { nr, args, result }
        }
    }))
}

/// Have runner `tid` make syscall `nr` with `args`, for line `line` (its
/// result named `result`): the call's id.
fn call(tid: u32, nr: u32, args: &[Arg], line: usize, result: Option<(String, String)>) -> Result<u32, String> {
    let Some((task, req)) = k(|k| Some((*k.runners.borrow().get(&tid)?, *k.requests.borrow().get(&tid)?))) else {
        return Err(format!("no runner {tid}"));
    };
    if k(|k| k.in_flight(tid).is_some()) {
        return Err(format!("{tid} is still making its call"));
    }
    let id = k(|k| k.asked.get() + 1);
    let (words, data) = pack(args, req + DATA)?;
    let c = Call { id, tid, nr, task, line, result, ..Default::default() };
    k(|k| {
        k.asked.set(id);
        k.calls.borrow_mut().insert(id, c);
        // (The page waits for the kernel now: see Kernel::waiting.)
        k.waiting.set(Some(k.clock.get()));
        // (Ring the runner: it waits on the request's number.)
        k.doorbell.borrow_mut().push_back(task);
    });
    // (The call into the runner's request, in its memory, which is
    // kernel memory: recorded, as its strings are.)
    let head: Vec<u8> = core::iter::once(nr).chain(words).flat_map(u32::to_le_bytes).collect();
    write_logged(req + DATA, &data);
    write_logged(req, &head);
    sched::interrupt(IRQ_DOORBELL);
    Ok(id)
}

/// A call's `args`, each a number, or a buffer (bytes, a string's with its
/// NUL; an array of strings, argv's) packed into the data (its bytes, to
/// go at `base`, the request's data: a buffer argument is its address
/// there): the argument words, and the data.
fn pack(args: &[Arg], base: u32) -> Result<([u32; 6], Vec<u8>), String> {
    let mut words = [0u32; 6];
    let mut data = Vec::new();
    // (Bytes `b` into the data, 8-byte aligned: their address.)
    let mut put = |b: &[u8]| -> Result<u32, String> {
        if data.len() + b.len() > DATA_SIZE as usize {
            return Err("the call's buffers are too big".into());
        }
        let at = base + data.len() as u32;
        data.extend_from_slice(b);
        data.resize((data.len() + 7) & !7, 0);
        Ok(at)
    };
    for (i, a) in args.iter().enumerate() {
        words[i] = match a {
            Arg::Number(v) => *v as u32,
            Arg::Bytes(b) => put(b)?,
            Arg::Strings(list) => {
                let mut pointers = Vec::new();
                for s in list {
                    pointers.push(match s {
                        Some(s) => put(&[s.as_bytes(), &[0]].concat())?,
                        None => 0,
                    });
                }
                put(&pointers.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>())?
            }
        };
    }
    Ok((words, data))
}

/// The page's program's names, as line `line` sees them (see
/// ksee_core::program::Names), and the kernel's constants; memory as it is
/// now.
struct Seen(usize);

impl ksee_core::program::Env for Seen {
    fn sym(&self, n: &str) -> Option<Sym> {
        if let Some(&v) = tables().types.constants.get(n) {
            return Some(Sym::Value { v, to: None });
        }
        k(|k| k.names.borrow().get(n, self.0).cloned())
    }

    fn memory(&self) -> Option<&dyn Memory> {
        Some(&Direct)
    }
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
        finished.push(&o);
    }
    let news = Object::new();
    js::set(&news, "finished", finished);
    js::set(&news, "runners", k(|k| k.runners.borrow().keys().map(|&t| JsValue::from(t)).collect::<js_sys::Array>()));
    for (ok, _) in waiters {
        let _ = ok.call1(&JsValue::NULL, &news);
    }
}
