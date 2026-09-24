//! The page's API: boot a kernel in Workers, and look at it.

use std::sync::atomic::Ordering;

use js_sys::{Function, Object, Uint8Array, WebAssembly};
use ksee_core::types::Kind;
use ksee_core::view::Views;
use ksee_core::tables::Tables;
use ksee_core::{cfg, dump, syscalls, trace, Memory};
use serde_wasm_bindgen as _;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use crate::debug::{self, DEBUG};
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
    /// The sizes of the pending call's output buffers.
    pending: std::cell::RefCell<Option<Vec<u32>>>,
    history: std::cell::RefCell<History>,
}

/// The last snapshot's records, and the task and time at each.
#[derive(Default)]
struct History {
    records: Vec<trace::Record>,
    tasks: Vec<u32>,
    times: Vec<u64>,
}

#[derive(serde::Deserialize)]
struct CallRequest {
    nr: u32,
    #[serde(default)]
    args: Vec<i64>,
    #[serde(default)]
    buffers: Vec<CallBuffer>,
}

#[derive(serde::Deserialize)]
struct CallBuffer {
    arg: u32,
    #[serde(default)]
    data: Option<Vec<u8>>,
    len: Option<u32>,
}

#[derive(serde::Serialize)]
struct SyscallInfo {
    nr: u32,
    name: String,
    built: bool,
    params: Option<Vec<ParamInfo>>,
}

#[derive(serde::Serialize)]
struct ParamInfo {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    kind: &'static str,
    /// For a pointer: the type pointed to (0: void), its size, and whether
    /// it is a struct or union.
    target: u32,
    size: u32,
    aggregate: bool,
}

#[derive(serde::Serialize)]
struct FieldInfo {
    name: String,
    offset: u32,
    size: u32,
    #[serde(rename = "type")]
    ty: String,
}

/// Bytes decoded as if they were memory at address 0.
struct Bytes<'a>(&'a [u8]);

impl ksee_core::Memory for Bytes<'_> {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = self.0.get(addr as usize + i).copied().unwrap_or(0);
        }
    }
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
        CURRENT_TASK.store(global("__current_task"), Ordering::Relaxed);
        PENDING.store(global("wasm_irq_pending"), Ordering::Relaxed);
        let trace: Function = js::get(&kernel, "ksee_trace").unchecked_into();
        TRACE.store(trace.call0(&JsValue::NULL)?.as_f64().unwrap_or(0.0) as u32, Ordering::Relaxed);
        let timer = Object::new();
        js::set(&timer, "role", "timer");
        js::set(&timer, "deadline", global("wasm_timer_deadline"));
        js::set(&timer, "pending", global("wasm_irq_pending"));
        spawn.call1(&JsValue::NULL, &timer)?;
        let boot = Object::new();
        js::set(&boot, "role", "boot");
        spawn.call1(&JsValue::NULL, &boot)?;
        Ok(Machine { kernel, workers, syscalls: Default::default(), pending: Default::default(), history: Default::default() })
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

    /// Input for the kernel's console (raises its interrupt).
    pub fn type_in(&self, text: &str) {
        CONSOLE_IN.put(text.as_bytes());
        interrupt(1); // WASM_IRQ_CONSOLE
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
        let records: Vec<trace::Record> = trace::ring(&Direct, self.ring(), t.ring_words).collect();
        // The running task and the time at each record (from the last enter
        // or exit record, which carry them).
        let (mut task, mut time) = (0, 0);
        let mut tasks = Vec::with_capacity(records.len());
        let mut times = Vec::with_capacity(records.len());
        for r in &records {
            if let trace::Record::Enter { current, time: t, .. } | trace::Record::Exit { current, time: t, .. } = *r {
                task = current;
                time = t;
            }
            tasks.push(task);
            times.push(time);
        }
        let calls = syscalls::split(entry, records.iter().copied());
        *self.syscalls.borrow_mut() = Some(calls);
        *self.history.borrow_mut() = History { records, tasks, times };
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

    // ---- History: the records of the last snapshot, by position ----

    /// How many records the last snapshot holds.
    pub fn records(&self) -> usize {
        self.history.borrow().records.len()
    }

    /// Where the kernel was at record `i`: { i, kind, func, block, file,
    /// line, column, time_ns, task: { addr, pid, comm }, syscall } (syscall:
    /// the index of the syscall in progress on that task, if any).
    pub fn at(&self, i: usize) -> JsValue {
        let h = self.history.borrow();
        let Some(r) = h.records.get(i) else { return JsValue::NULL };
        let t = tables();
        let o = Object::new();
        let (kind, func, block) = match *r {
            trace::Record::Block { func, n } => ("block", func, Some(n)),
            trace::Record::Enter { func, .. } => ("enter", func, None),
            trace::Record::Exit { func, .. } => ("exit", func, None),
        };
        js::set(&o, "i", i as f64);
        js::set(&o, "kind", kind);
        js::set(&o, "func", t.functions.get(&func).map_or(String::new(), |f| f.name.clone()));
        let loc = match block {
            Some(n) => {
                js::set(&o, "block", n);
                t.blocks.get(&(func, n)).copied()
            }
            None => t.functions.get(&func).map(|f| f.loc),
        };
        if let Some(l) = loc {
            js::set(&o, "file", t.file(l));
            js::set(&o, "line", l.line);
            js::set(&o, "column", l.column);
        }
        js::set(&o, "time_ns", h.times[i] as f64);
        js::set(&o, "task", to_js(&self.views_task(h.tasks[i])));
        let task = h.tasks[i];
        let sys = self.with_syscalls(|calls| calls.iter().position(|s| {
            s.task == task && s.first <= i && s.last.is_none_or(|l| i <= l)
        }));
        js::set(&o, "syscall", sys.map_or(JsValue::NULL, |s| (s as f64).into()));
        o.into()
    }

    /// The position of the next (dir > 0) or previous block record from `i`
    /// that starts a new `unit`: "block", "function" (another function than
    /// at `i`), or "syscall" (a syscall's first record). `i` if none.
    pub fn seek(&self, i: usize, dir: i32, unit: &str) -> usize {
        let h = self.history.borrow();
        let n = h.records.len();
        let func_at = |j: usize| match h.records.get(j) {
            Some(trace::Record::Block { func, .. }) => Some(*func),
            _ => None,
        };
        if unit == "syscall" {
            return self.with_syscalls(|calls| {
                let starts = calls.iter().map(|s| s.first);
                if dir > 0 { starts.filter(|&f| f > i).min() } else { starts.filter(|&f| f < i).max() }
            }).unwrap_or(i);
        }
        let here = func_at(i);
        let mut j = i;
        loop {
            if dir > 0 { if j + 1 >= n { return i } j += 1 } else { if j == 0 { return i } j -= 1 }
            let Some(f) = func_at(j) else { continue };
            if unit != "function" || Some(f) != here {
                return j;
            }
        }
    }

    /// The trace in `n` buckets, for a timeline: [{ pid, comm, syscall }]
    /// (the task and the syscall in progress at the bucket's start).
    pub fn timeline(&self, n: usize) -> JsValue {
        let h = self.history.borrow();
        let total = h.records.len().max(1);
        let spans: Vec<(u32, usize, usize)> = self.with_syscalls(|calls| {
            calls.iter().map(|s| (s.task, s.first, s.last.unwrap_or(usize::MAX))).collect()
        });
        let mut names: std::collections::BTreeMap<u32, Option<dump::Task>> = Default::default();
        let list = js_sys::Array::new();
        for b in 0..n {
            let i = b * total / n.max(1);
            let task = h.tasks.get(i).copied().unwrap_or(0);
            let info = names.entry(task).or_insert_with(|| live_task(task)).clone();
            let o = Object::new();
            js::set(&o, "pid", info.as_ref().map_or(-1, |t| t.pid));
            js::set(&o, "comm", info.map_or(String::new(), |t| t.comm));
            let sys = spans.iter().position(|&(t, f, l)| t == task && f <= i && i <= l);
            js::set(&o, "syscall", sys.map_or(-1.0, |s| s as f64));
            list.push(&o);
        }
        list.into()
    }

    /// Where the kernel is now: its most recent block (from the live ring)
    /// and the current task: { func, file, line, task }.
    pub fn now(&self) -> JsValue {
        let t = tables();
        let ring = self.ring();
        let pos = Direct.read_u32(ring) & !1;
        let pairs = t.ring_words / 2;
        // The newest block record, among the last few pairs.
        for k in 1..=64.min(pos / 2) {
            let slot = (pos / 2 - k) % pairs.max(1);
            let a = ring + trace::RING + slot * 8;
            let (func, tag) = (Direct.read_u32(a), Direct.read_u32(a + 4));
            if tag >> 28 == 1 {
                let o = Object::new();
                js::set(&o, "func", t.functions.get(&func).map_or(String::new(), |f| f.name.clone()));
                if let Some(&l) = t.blocks.get(&(func, tag & 0x0fff_ffff)) {
                    js::set(&o, "file", t.file(l));
                    js::set(&o, "line", l.line);
                }
                let task = Direct.read_u32(CURRENT_TASK.load(Ordering::Relaxed));
                js::set(&o, "task", to_js(&self.views_task(task)));
                return o.into();
            }
        }
        JsValue::NULL
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

    /// The names of variables with fixed addresses (runqueues, init_task, ...).
    pub fn globals(&self) -> Vec<String> {
        tables().types.globals.keys().cloned().collect()
    }

    /// The value of the variable `name` (an Object), or null.
    pub fn global(&self, name: &str, depth: Option<u32>) -> JsValue {
        let types = &tables().types;
        match types.globals.get(name) {
            Some(&(addr, ty)) => to_js(&types.object(&Direct, name, addr, ty, depth.unwrap_or(1))),
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

    // ---- Making syscalls (through /init, the agent on /dev/ksee) ----

    /// The kernel's syscalls: [{ nr, name, built, params: [{ name, type,
    /// kind }] | null }], kind "in" (a const pointer: data the call reads),
    /// "out" (a pointer it writes), or "value".
    pub fn syscall_table(&self) -> JsValue {
        let types = &tables().types;
        let list: Vec<SyscallInfo> = types.syscalls.iter().map(|(&nr, s)| SyscallInfo {
            nr,
            name: s.name.clone(),
            built: s.built,
            params: s.params.as_ref().map(|ps| ps.iter().map(|p| {
                let target = match types.get(types.resolve(p.ty)).kind {
                    Kind::Pointer(t) => types.resolve(t),
                    _ => 0,
                };
                ParamInfo {
                    name: p.name.clone(),
                    ty: p.c_type.clone(),
                    kind: if !p.c_type.ends_with('*') { "value" }
                          else if p.c_type.starts_with("const ") { "in" } else { "out" },
                    target,
                    size: types.size_of(target),
                    aggregate: matches!(types.get(target).kind, Kind::Struct(_) | Kind::Union(_)),
                }
            }).collect()),
        }).collect();
        to_js(&list)
    }

    /// The scalar fields of struct type `ty` (flattened, with byte offsets),
    /// e.g. to build a struct argument: [{ name, offset, size, type }].
    pub fn layout(&self, ty: u32) -> JsValue {
        let types = &tables().types;
        let mut out = Vec::new();
        fn walk(types: &ksee_core::types::Types, ty: u32, prefix: &str, base: u32, out: &mut Vec<FieldInfo>) {
            let t = types.get(types.resolve(ty));
            match &t.kind {
                Kind::Struct(members) | Kind::Union(members) => {
                    for m in members {
                        let name = if prefix.is_empty() { m.name.clone() } else { format!("{prefix}.{}", m.name) };
                        walk(types, m.ty, &name, base + m.bit_offset / 8, out);
                    }
                }
                _ => out.push(FieldInfo { name: prefix.to_string(), offset: base,
                                          size: types.size_of(ty), ty: types.name(ty) }),
            }
        }
        walk(types, ty, "", 0, &mut out);
        to_js(&out)
    }

    /// Decode `bytes` (a call's output buffer, say) as type `ty`: an Object.
    pub fn decode_bytes(&self, label: &str, ty: u32, bytes: &[u8]) -> JsValue {
        to_js(&tables().types.object(&Bytes(bytes), label, 0, ty, 3))
    }

    /// Every line of `file` where a basic block starts, with how often
    /// blocks there ran in syscall `n` (or the whole trace): [{ line, count }].
    pub fn file_coverage(&self, file: &str, n: Option<usize>) -> JsValue {
        let t = tables();
        let mut counts: std::collections::BTreeMap<(u32, u32), u64> = Default::default();
        let mut add = |f: u32, b: u32| *counts.entry((f, b)).or_default() += 1;
        match n {
            Some(n) => self.with_syscalls(|calls| {
                for &(f, b) in calls.get(n).map(|s| &s.blocks[..]).unwrap_or(&[]) {
                    add(f, b);
                }
            }),
            None => {
                for r in trace::ring(&Direct, self.ring(), t.ring_words) {
                    if let trace::Record::Block { func, n } = r {
                        add(func, n);
                    }
                }
            }
        }
        let mut lines: std::collections::BTreeMap<u32, u64> = Default::default();
        for (&(f, b), l) in &t.blocks {
            if t.file(*l) == file {
                let c = lines.entry(l.line).or_default();
                *c = (*c).max(counts.get(&(f, b)).copied().unwrap_or(0));
            }
        }
        let list = js_sys::Array::new();
        for (line, count) in lines {
            let o = Object::new();
            js::set(&o, "line", line);
            js::set(&o, "count", count as f64);
            list.push(&o);
        }
        list.into()
    }

    /// Ask the agent to make a syscall: `{ nr, args: [6 numbers],
    /// buffers: [{ arg, data?: Uint8Array | number[] (input), len?:
    /// number (output) }] }`: each buffer is placed in the agent's memory and
    /// replaces argument `arg` with its address. False if a call is pending.
    pub fn call(&self, request: JsValue) -> Result<bool, JsValue> {
        let req: CallRequest = serde_wasm_bindgen::from_value(request)?;
        if MAILBOX.state.load(Ordering::Acquire) != EMPTY {
            return Ok(false);
        }
        // u32 nr, nbufs; i64 args[6]; { u32 arg, dir, len, off }[nbufs]; data
        let mut out = Vec::new();
        out.extend(req.nr.to_le_bytes());
        out.extend((req.buffers.len() as u32).to_le_bytes());
        for i in 0..6 {
            out.extend(req.args.get(i).copied().unwrap_or(0).to_le_bytes());
        }
        let mut off = (out.len() + 16 * req.buffers.len()) as u32;
        let mut data = Vec::new();
        for b in &req.buffers {
            let (dir, bytes) = match (&b.data, b.len) {
                (Some(d), _) => (0u32, d.clone()),
                (None, len) => (1, vec![0; len.unwrap_or(0) as usize]),
            };
            let len = bytes.len() as u32;
            for v in [b.arg, dir, len, off] {
                out.extend(v.to_le_bytes());
            }
            off += len.next_multiple_of(8);
            data.extend(&bytes);
            data.resize(data.len().next_multiple_of(8), 0);
        }
        out.extend(data);
        if out.len() > MAILBOX_SIZE {
            return Err("request too large".into());
        }
        let data = unsafe { &mut *MAILBOX.data.get() };
        data[..out.len()].copy_from_slice(&out);
        MAILBOX.len.store(out.len() as u32, Ordering::Relaxed);
        MAILBOX.state.store(REQUEST, Ordering::Release);
        interrupt(2); // WASM_IRQ_AGENT
        *self.pending.borrow_mut() = Some(req.buffers.iter().filter(|b| b.data.is_none())
            .map(|b| b.len.unwrap_or(0)).collect());
        Ok(true)
    }

    /// The last call's result once the agent has replied: { ret, outputs:
    /// [Uint8Array] } (the output buffers, in order); else null.
    pub fn call_result(&self) -> JsValue {
        if MAILBOX.state.load(Ordering::Acquire) != REPLY {
            return JsValue::NULL;
        }
        let len = MAILBOX.len.load(Ordering::Relaxed) as usize;
        let reply = unsafe { &*MAILBOX.data.get() }[..len.min(MAILBOX_SIZE)].to_vec();
        MAILBOX.state.store(EMPTY, Ordering::Release);
        let sizes = self.pending.borrow_mut().take().unwrap_or_default();
        let ret = i64::from_le_bytes(reply.get(..8).and_then(|b| b.try_into().ok()).unwrap_or([0; 8]));
        let o = Object::new();
        js::set(&o, "ret", ret as f64);
        let outputs = js_sys::Array::new();
        let mut at = 8;
        for n in sizes {
            let end = (at + n as usize).min(reply.len());
            outputs.push(&Uint8Array::from(&reply[at.min(end)..end]));
            at = end;
        }
        js::set(&o, "outputs", outputs);
        o.into()
    }

    // ---- Stopping the kernel ----

    /// Where the kernel is stopped, if it is: { func, block, file, line,
    /// column, task }.
    pub fn stopped(&self) -> JsValue {
        let Some((func, n, task)) = DEBUG.stopped() else { return JsValue::NULL };
        let t = tables();
        let o = Object::new();
        js::set(&o, "func", t.functions.get(&func).map_or(String::new(), |f| f.name.clone()));
        js::set(&o, "block", n);
        if let Some(&l) = t.blocks.get(&(func, n)) {
            js::set(&o, "file", t.file(l));
            js::set(&o, "line", l.line);
            js::set(&o, "column", l.column);
        }
        js::set(&o, "task", to_js(&self.views_task(task)));
        o.into()
    }

    fn views_task(&self, addr: u32) -> ksee_core::view::TaskRef {
        let t = live_task(addr);
        ksee_core::view::TaskRef { addr, pid: t.as_ref().map(|t| t.pid), comm: t.map(|t| t.comm) }
    }

    /// Stop at the next block the kernel runs.
    pub fn pause(&self) {
        DEBUG.pause();
    }

    /// From a stop: run to the next block.
    pub fn step(&self) {
        DEBUG.go(debug::STEP);
    }

    /// From a stop: run to the next block of the same function, on the
    /// same task (over calls).
    pub fn next(&self) {
        DEBUG.go(debug::NEXT);
    }

    /// From a stop: run until a breakpoint.
    pub fn resume(&self) {
        DEBUG.go(debug::RUN);
    }

    /// Break at the start of every function called `name`.
    pub fn break_function(&self, name: &str, on: bool) -> bool {
        let t = tables();
        let ids: Vec<u32> = t.functions.iter().filter(|(_, f)| f.name == name).map(|(&id, _)| id).collect();
        for &id in &ids {
            if on { DEBUG.add(id, 0); } else { DEBUG.remove(id, 0); }
        }
        !ids.is_empty()
    }

    /// Break at the blocks that start at `file:line`; the number of blocks.
    pub fn break_line(&self, file: &str, line: u32, on: bool) -> usize {
        let t = tables();
        let blocks: Vec<(u32, u32)> = t.blocks.iter()
            .filter(|(_, l)| l.line == line && t.file(**l) == file).map(|(&k, _)| k).collect();
        for &(f, n) in &blocks {
            if on { DEBUG.add(f, n); } else { DEBUG.remove(f, n); }
        }
        blocks.len()
    }

    /// The breakpoints, as [{ func, block, file, line }].
    pub fn breakpoints(&self) -> JsValue {
        let t = tables();
        let list = js_sys::Array::new();
        for (f, n) in DEBUG.list() {
            let o = Object::new();
            js::set(&o, "func", t.functions.get(&f).map_or(String::new(), |f| f.name.clone()));
            js::set(&o, "block", n);
            if let Some(&l) = t.blocks.get(&(f, n)) {
                js::set(&o, "file", t.file(l));
                js::set(&o, "line", l.line);
            }
            list.push(&o);
        }
        list.into()
    }

    /// The trace dump (read it with ./ksee.sh).
    pub fn dump(&self) -> Vec<u8> {
        let mut out = Vec::new();
        dump::write(tables(), &Direct, self.ring(), &mut |b| out.extend_from_slice(b));
        out
    }
}
