//! The page's API: boot a kernel in Workers, and look at it.

use std::sync::atomic::Ordering;

use js_sys::{Function, Object, Uint8Array, WebAssembly};
use ksee_core::view::Views;
use ksee_core::tables::Tables;
use ksee_core::{stack, syscalls, task, trace, Memory};
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
    history: std::cell::RefCell<History>,
    /// Memory as it was after a record (memory_at), for the views; None:
    /// now. mem: the trace's stores, indexed (for the snapshot whose last
    /// record's id it has). frozen: kernel memory when recording stopped
    /// (the past is that, stores undone; the kernel went on unrecorded).
    past: std::cell::RefCell<Option<usize>>,
    mem: std::cell::RefCell<Option<(u32, ksee_core::mem::History)>>,
    frozen: std::cell::RefCell<Option<Vec<u8>>>,
}

/// The last snapshot's records, and the task and time at each.
#[derive(Default)]
struct History {
    records: Vec<trace::Record>,
    /// Each record's id (trace::ring_ids).
    ids: Vec<u32>,
    tasks: Vec<u32>,
    times: Vec<u64>,
    /// Who was at a task_struct address, from Task records: address ->
    /// [(record, task)]; and where tasks exited (do_exit): address ->
    /// [record]. The address of an exited task may be another's later.
    idents: std::collections::BTreeMap<u32, Vec<(usize, task::Task)>>,
    exits: std::collections::BTreeMap<u32, Vec<usize>>,
}

impl History {
    /// The task at `addr` at record `i`: the Task record of its program
    /// (before or after `i`, as long as it did not exit in between), else
    /// what the address holds now.
    fn task_at(&self, addr: u32, i: usize) -> Option<task::Task> {
        let exited = |a: usize, b: usize| self.exits.get(&addr).is_some_and(|e| e.iter().any(|&x| a <= x && x < b));
        if let Some(ids) = self.idents.get(&addr) {
            if let Some((at, t)) = ids.iter().rev().find(|(at, _)| *at <= i) {
                if !exited(*at, i) {
                    return Some(t.clone());
                }
            }
            if let Some((at, t)) = ids.iter().find(|(at, _)| *at > i) {
                if !exited(i, *at) {
                    return Some(t.clone());
                }
            }
        }
        live_task(addr)
    }
}

/// Kernel memory as it was when recording stopped.
struct Frozen<'a>(&'a [u8]);

impl ksee_core::Memory for Frozen<'_> {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        match self.0.get(addr as usize..addr as usize + buf.len()) {
            Some(b) => buf.copy_from_slice(b),
            None => Direct.read(addr, buf),
        }
    }
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

fn task_ref(addr: u32, t: Option<task::Task>) -> ksee_core::view::TaskRef {
    ksee_core::view::TaskRef { addr, pid: t.as_ref().map(|t| t.pid), comm: t.map(|t| t.comm) }
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
        Ok(Machine { kernel, workers, syscalls: Default::default(), history: Default::default(), past: Default::default(), mem: Default::default(),
                    frozen: Default::default() })
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
        let (records, ids) = trace::ring_ids(&Direct, self.ring(), t.ring_words);
        // The running task and the time at each record (from the last enter
        // or exit record, which carry them).
        let (mut task, mut time) = (0, 0);
        let mut tasks = Vec::with_capacity(records.len());
        let mut times = Vec::with_capacity(records.len());
        let mut idents: std::collections::BTreeMap<u32, Vec<(usize, task::Task)>> = Default::default();
        let mut exits: std::collections::BTreeMap<u32, Vec<usize>> = Default::default();
        let do_exit = t.find("do_exit");
        for (i, r) in records.iter().enumerate() {
            if let trace::Record::Enter { current, time: t, .. } | trace::Record::Exit { current, time: t, .. } = *r {
                task = current;
                time = t;
            }
            match *r {
                trace::Record::Task { task: addr, pid, comm } => {
                    task = addr;
                    let bytes: Vec<u8> = comm.iter().flat_map(|w| w.to_le_bytes()).take_while(|&b| b != 0).collect();
                    let who = task::Task { pid: pid as i32, comm: String::from_utf8_lossy(&bytes).into() };
                    idents.entry(addr).or_default().push((i, who));
                }
                trace::Record::Enter { func, current, .. } if Some(func) == do_exit => {
                    exits.entry(current).or_default().push(i);
                }
                _ => {}
            }
            tasks.push(task);
            times.push(time);
        }
        let calls = syscalls::split(entry, records.iter().copied());
        *self.syscalls.borrow_mut() = Some(calls);
        *self.history.borrow_mut() = History { records, ids, tasks, times, idents, exits };
    }

    fn with_syscalls<T>(&self, f: impl FnOnce(&[syscalls::Syscall]) -> T) -> T {
        if self.syscalls.borrow().is_none() {
            self.snapshot();
        }
        f(self.syscalls.borrow().as_deref().unwrap_or(&[]))
    }

    /// Views that name tasks from memory now (for what needs no task).
    fn views(&self) -> Views<'static> {
        fn live(addr: u32, _: usize) -> Option<task::Task> {
            live_task(addr)
        }
        Views { tables: tables(), tasks: &live }
    }

    /// Views that name tasks as they were (see History::task_at).
    fn with_views<T>(&self, f: impl FnOnce(&Views) -> T) -> T {
        let h = self.history.borrow();
        let tasks = |addr: u32, i: usize| h.task_at(addr, i);
        f(&Views { tables: tables(), tasks: &tasks })
    }

    // ---- History: the records of the last snapshot, by position ----

    /// Record the trace (on) or not. Stopping takes a snapshot, and keeps
    /// kernel memory as it is then: memory as it was in the recording is
    /// that, the recorded stores undone (the kernel goes on, unrecorded).
    pub fn record(&self, on: bool) {
        let word = unsafe { &*((self.ring() + trace::ON) as *const std::sync::atomic::AtomicU32) };
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

    /// Memory for the views (object, object_by_id, global) as it was after
    /// record `i` of the last snapshot (the stores since undone; see
    /// ksee_core::mem), or as it is now (None): how many stores are undone.
    pub fn memory_at(&self, i: Option<usize>) -> i32 {
        *self.past.borrow_mut() = i;
        match i {
            Some(i) => self.with_history(|h| h.after(i) as i32),
            None => 0,
        }
    }

    /// The newest write to `addr..addr + len` at or before the moment
    /// memory_at shows (or the end of the trace): { i (the store's record),
    /// func, file, line, task, old }, or null if the trace has none.
    pub fn last_write(&self, addr: u32, len: u32) -> JsValue {
        let at = self.past.borrow().unwrap_or(usize::MAX);
        let found = {
            let h = self.history.borrow();
            let at = at.min(h.records.len().saturating_sub(1));
            self.with_history(|m| m.last_write(&h.records, addr, len, at))
        };
        let Some((i, old)) = found else { return JsValue::NULL };
        let o = self.at(i);
        if o.is_null() {
            return o;
        }
        js::set(&o, "old", old as f64);
        // The line that made it: where the function running then is (after
        // a return into the middle of a block, the caller's).
        let h = self.history.borrow();
        let t = tables();
        if let Some(step) = stack::lines(t, &h.records, &h.tasks, i.saturating_sub(4000), i, h.tasks.get(i).copied()).last() {
            js::set(&o, "func", t.functions.get(&step.func).map_or(String::new(), |f| f.name.clone()));
            js::set(&o, "file", t.file(step.loc));
            js::set(&o, "line", step.loc.line);
            if let Some(k) = step.inlined {
                js::set(&o, "inlined", t.names.get(k as usize).cloned().unwrap_or_default());
            }
        }
        o
    }

    /// Run `f` on the stores of the last snapshot, indexed (once per snapshot).
    fn with_history<T>(&self, f: impl FnOnce(&ksee_core::mem::History) -> T) -> T {
        let key = self.history.borrow().ids.last().copied().unwrap_or(0);
        let mut mem = self.mem.borrow_mut();
        if mem.as_ref().is_none_or(|(k, _)| *k != key) {
            *mem = Some((key, ksee_core::mem::History::new(&self.history.borrow().records)));
        }
        f(&mem.as_ref().unwrap().1)
    }

    /// Run `f` on memory as the views should see it (memory_at).
    fn with_memory<T>(&self, f: impl FnOnce(&dyn Memory) -> T) -> T {
        let frozen = self.frozen.borrow();
        let base: &dyn Memory = match &*frozen {
            Some(ram) => &Frozen(ram),
            None => &Direct,
        };
        match *self.past.borrow() {
            Some(at) => self.with_history(|history| f(&ksee_core::mem::PastMemory { base, history, at })),
            None => f(&Direct),
        }
    }

    /// Record `i`'s id: it names the record in later snapshots too (see
    /// index_of), where its index has moved on.
    pub fn record_id(&self, i: usize) -> Option<u32> {
        self.history.borrow().ids.get(i).copied()
    }

    /// The index of the record with id `id` in the last snapshot (or of the
    /// first after it; the end if none).
    pub fn index_of(&self, id: u32) -> usize {
        self.history.borrow().ids.partition_point(|&x| x < id)
    }

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
            trace::Record::Return { func } => ("return", func, None),
            trace::Record::Line { .. } => ("line", 0, None),
            trace::Record::Task { .. } => ("task", 0, None),
            trace::Record::Args { .. } => ("args", 0, None),
            trace::Record::Store { .. } => ("store", 0, None),
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
        js::set(&o, "task", to_js(&task_ref(h.tasks[i], h.task_at(h.tasks[i], i))));
        let task = h.tasks[i];
        let sys = self.with_syscalls(|calls| calls.iter().position(|s| {
            s.task == task && s.first <= i && s.last.is_none_or(|l| i <= l)
        }));
        js::set(&o, "syscall", sys.map_or(JsValue::NULL, |s| (s as f64).into()));
        o.into()
    }

    /// The position of the next (dir > 0) or previous block record from `i`
    /// that starts a new `unit`: "block", "line" (another source line, on
    /// the task running at `i`), "function" (another function than at `i`),
    /// or "syscall" (a syscall's first record). `i` if none.
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
        let t = tables();
        let line_at = |j: usize| match h.records.get(j) {
            Some(&trace::Record::Block { func, n }) => t.blocks.get(&(func, n)).map(|l| (l.file, l.line)),
            _ => None,
        };
        let (here, here_line, task) = (func_at(i), line_at(i), h.tasks.get(i).copied());
        let mut j = i;
        loop {
            if dir > 0 { if j + 1 >= n { return i } j += 1 } else { if j == 0 { return i } j -= 1 }
            let Some(f) = func_at(j) else { continue };
            let new = match unit {
                "function" => Some(f) != here,
                "line" => h.tasks.get(j).copied() == task && line_at(j).is_some() && line_at(j) != here_line,
                _ => true,
            };
            if new {
                return j;
            }
        }
    }

    /// The calls in records `first..=last` (of the task at address `task`,
    /// or any), rebuilt from the trace (see ksee_core::stack), at least
    /// `min` records long: [{ func, file, depth, first, last }].
    pub fn frames(&self, first: usize, last: usize, task: Option<u32>, min: Option<usize>) -> JsValue {
        let h = self.history.borrow();
        let t = tables();
        let list = js_sys::Array::new();
        for f in stack::frames(&h.records, &h.tasks, first, last, task, min.unwrap_or(1).max(1)) {
            let o = Object::new();
            let func = t.functions.get(&f.func);
            js::set(&o, "func", func.map_or(String::new(), |x| x.name.clone()));
            js::set(&o, "file", func.map_or("", |x| t.file(x.loc)));
            js::set(&o, "depth", f.depth);
            js::set(&o, "first", f.first as f64);
            js::set(&o, "last", f.last as f64);
            list.push(&o);
        }
        list.into()
    }

    /// The latest run of program `pid` (a Task record says which) in the
    /// last snapshot: { task (address), first, last (records), lines: [{ i,
    /// line }] (its Line records: the program's source line, before each
    /// syscall) }, or null.
    pub fn program(&self, pid: i32) -> JsValue {
        let h = self.history.borrow();
        let Some((addr, start)) = h.idents.iter()
            .flat_map(|(&a, ids)| ids.iter().filter(|(_, t)| t.pid == pid).map(move |(i, _)| (a, *i)))
            .max_by_key(|&(_, i)| i) else { return JsValue::NULL };
        // Until the task_struct is another program's (or the trace ends).
        let until = h.idents[&addr].iter().map(|(i, _)| *i).find(|&i| i > start).unwrap_or(h.records.len());
        let mut last = start;
        let lines = js_sys::Array::new();
        for j in start..until {
            if h.tasks[j] != addr && !matches!(h.records[j], trace::Record::Line { .. }) {
                continue;
            }
            last = j;
            if let trace::Record::Line { line } = h.records[j] {
                let o = Object::new();
                js::set(&o, "i", j as f64);
                js::set(&o, "line", line);
                lines.push(&o);
            }
        }
        let o = Object::new();
        js::set(&o, "task", addr);
        js::set(&o, "first", start as f64);
        js::set(&o, "last", last as f64);
        js::set(&o, "lines", lines);
        o.into()
    }

    /// The source lines run in records `first..=last` of the task at address
    /// `task`, in order (see ksee_core::stack::lines): [{ i (the record),
    /// func, depth, file, line, inlined (the function the line is inlined
    /// from, if) }].
    pub fn line_steps(&self, first: usize, last: usize, task: u32) -> JsValue {
        let h = self.history.borrow();
        let t = tables();
        let list = js_sys::Array::new();
        for s in stack::lines(t, &h.records, &h.tasks, first, last, Some(task)) {
            let o = Object::new();
            js::set(&o, "i", s.record as f64);
            js::set(&o, "func", t.functions.get(&s.func).map_or(String::new(), |f| f.name.clone()));
            js::set(&o, "depth", s.depth);
            js::set(&o, "file", t.file(s.loc));
            js::set(&o, "line", s.loc.line);
            if let Some(k) = s.inlined {
                js::set(&o, "inlined", t.names.get(k as usize).cloned().unwrap_or_default());
            }
            list.push(&o);
        }
        list.into()
    }

    /// The call stack at record `i` (of the task running then), outermost
    /// first: [function name].
    pub fn stack_at(&self, i: usize) -> Vec<String> {
        let h = self.history.borrow();
        let t = tables();
        stack::stack_at(&h.records, &h.tasks, i).into_iter()
            .map(|f| t.functions.get(&f).map_or(format!("{f:#x}"), |x| x.name.clone())).collect()
    }

    /// The call stack at record `i` of the last snapshot (or its last: at
    /// a stop, take a snapshot first), innermost first, rebuilt from the
    /// trace: [{ func, file, line, args: [Object (see ksee_core::view)] }]
    /// (arguments are 32 bits: wider ones lose their high half; __user
    /// pointers point into the process, not the kernel).
    pub fn call_stack(&self, i: Option<usize>) -> JsValue {
        let h = self.history.borrow();
        let t = tables();
        let list = js_sys::Array::new();
        let Some(i) = i.or(h.records.len().checked_sub(1)) else { return list.into() };
        for (func, _, args) in stack::calls_at(&h.records, &h.tasks, i).into_iter().rev() {
            let o = Object::new();
            let f = t.functions.get(&func);
            let name = f.map_or(format!("{func:#x}"), |f| f.name.clone());
            js::set(&o, "func", name.clone());
            if let Some(f) = f {
                js::set(&o, "file", t.file(f.loc));
                js::set(&o, "line", f.loc.line);
            }
            if let Some(args) = args {
                let known = f.and_then(|f| f.known).unwrap_or(u32::MAX);
                let list = js_sys::Array::new();
                for (k, ((pname, ty), v)) in t.types.params.get(&name).into_iter().flatten().zip(args).enumerate() {
                    let mut arg = t.types.object(&Bytes(&v.to_le_bytes()), pname, 0, *ty, 0);
                    if known >> k & 1 == 0 {
                        // (Not passed any more: the optimizer took it away.)
                        arg.value = Some("(optimized out)".into());
                        arg.children = None;
                        arg.expand = None;
                        arg.target = None;
                    }
                    list.push(&to_js(&arg));
                }
                js::set(&o, "args", list);
            }
            list.push(&o);
        }
        list.into()
    }

    /// The trace in `n` buckets, for a timeline: [{ pid, comm, syscall }]
    /// (the task and the syscall in progress at the bucket's start).
    pub fn timeline(&self, n: usize) -> JsValue {
        let h = self.history.borrow();
        let total = h.records.len().max(1);
        let spans: Vec<(u32, usize, usize)> = self.with_syscalls(|calls| {
            calls.iter().map(|s| (s.task, s.first, s.last.unwrap_or(usize::MAX))).collect()
        });
        let list = js_sys::Array::new();
        for b in 0..n {
            let i = b * total / n.max(1);
            let task = h.tasks.get(i).copied().unwrap_or(0);
            let info = h.task_at(task, i);
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
        self.with_syscalls(|calls| self.with_views(|v| {
            to_js(&calls.iter().enumerate().map(|(i, s)| v.summary(i, s)).collect::<Vec<_>>())
        }))
    }

    /// What syscall `n` ran, as runs of blocks per function.
    pub fn runs(&self, n: usize) -> JsValue {
        self.with_syscalls(|calls| to_js(&calls.get(n).map(|s| self.views().runs(s)).unwrap_or_default()))
    }

    /// The instrumented functions called `name` (usually one), with how
    /// often each block ran in syscall `n` (or the whole trace).
    pub fn functions(&self, name: &str, n: Option<usize>) -> JsValue {
        let t = tables();
        let ids: Vec<u32> = t.functions.iter().filter(|(_, f)| f.name == name).map(|(&id, _)| id).collect();
        let counts = self.block_counts(n, |f| ids.contains(&f));
        let v = self.views();
        to_js(&ids.iter().filter_map(|&id| v.function(id, &counts)).collect::<Vec<_>>())
    }

    /// How often each block ran in syscall `n` (or the whole trace), for
    /// the functions `keep` accepts.
    fn block_counts(&self, n: Option<usize>, keep: impl Fn(u32) -> bool)
                    -> std::collections::BTreeMap<(u32, u32), u64> {
        let mut counts = std::collections::BTreeMap::new();
        let mut add = |f: u32, b: u32| if keep(f) { *counts.entry((f, b)).or_default() += 1 };
        match n {
            Some(n) => self.with_syscalls(|calls| {
                for &(f, b) in calls.get(n).map(|s| &s.blocks[..]).unwrap_or(&[]) {
                    add(f, b);
                }
            }),
            None => {
                for r in trace::ring(&Direct, self.ring(), tables().ring_words) {
                    if let trace::Record::Block { func, n } = r {
                        add(func, n);
                    }
                }
            }
        }
        counts
    }

    /// The value of the type named `name` at `addr` (an Object, see
    /// ksee_core::view), aggregates expanded `depth` levels.
    pub fn object(&self, name: &str, addr: u32, depth: Option<u32>) -> JsValue {
        let types = &tables().types;
        match types.find(name) {
            Some(id) => self.with_memory(|mem| to_js(&types.object(mem, name, addr, id, depth.unwrap_or(1)))),
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
            Some(&(addr, ty)) => self.with_memory(|mem| to_js(&types.object(mem, name, addr, ty, depth.unwrap_or(1)))),
            None => JsValue::NULL,
        }
    }

    /// Like `object`, by type id (an Object's `expand` or `target`).
    pub fn object_by_id(&self, label: &str, ty: u32, addr: u32, depth: Option<u32>) -> JsValue {
        self.with_memory(|mem| to_js(&tables().types.object(mem, label, addr, ty, depth.unwrap_or(1))))
    }

    // ---- Making syscalls (through /init, the agent on /dev/ksee) ----

    /// Every line of `file` where a basic block starts, with how often
    /// blocks there ran in syscall `n` (or the whole trace): [{ line, count }].
    pub fn file_coverage(&self, file: &str, n: Option<usize>) -> JsValue {
        let t = tables();
        let counts = self.block_counts(n, |_| true);
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

    /// Send init (the process on /dev/ksee, see user/init.c) a request:
    /// `op` with `a`, `b`, up to six `args` and `data`. False if one is
    /// pending. See web/ksee.js for the operations.
    pub fn post(&self, op: u32, a: u32, b: u32, args: Vec<f64>, data: Option<Vec<u8>>) -> Result<bool, JsValue> {
        if MAILBOX.state.load(Ordering::Acquire) != EMPTY {
            return Ok(false);
        }
        let mut out = Vec::with_capacity(64);
        for v in [op, a, b, 0] {
            out.extend(v.to_le_bytes());
        }
        for i in 0..6 {
            out.extend((args.get(i).copied().unwrap_or(0.0) as i64).to_le_bytes());
        }
        out.extend(data.unwrap_or_default());
        if out.len() > MAILBOX_SIZE {
            return Err("request too large".into());
        }
        let buf = unsafe { &mut *MAILBOX.data.get() };
        buf[..out.len()].copy_from_slice(&out);
        MAILBOX.len.store(out.len() as u32, Ordering::Relaxed);
        MAILBOX.state.store(REQUEST, Ordering::Release);
        interrupt(2); // WASM_IRQ_CHANNEL
        Ok(true)
    }

    /// The reply to the last request, once init has sent it: { ret, data }.
    pub fn take(&self) -> JsValue {
        if MAILBOX.state.load(Ordering::Acquire) != REPLY {
            return JsValue::NULL;
        }
        let len = (MAILBOX.len.load(Ordering::Relaxed) as usize).min(MAILBOX_SIZE);
        let reply = unsafe { &*MAILBOX.data.get() }[..len].to_vec();
        MAILBOX.state.store(EMPTY, Ordering::Release);
        let ret = i64::from_le_bytes(reply.get(..8).and_then(|b| b.try_into().ok()).unwrap_or([0; 8]));
        let o = Object::new();
        js::set(&o, "ret", ret as f64);
        js::set(&o, "data", Uint8Array::from(reply.get(8..).unwrap_or(&[])));
        o.into()
    }

    // ---- Stopping the kernel ----

    /// Where the kernel is stopped, if it is: { func, block, file, line,
    /// column, task }.
    pub fn stopped(&self) -> JsValue {
        let Some((func, n, task, stop)) = DEBUG.stopped() else { return JsValue::NULL };
        let t = tables();
        let o = Object::new();
        js::set(&o, "func", t.functions.get(&func).map_or(String::new(), |f| f.name.clone()));
        js::set(&o, "block", n);
        js::set(&o, "stop", stop);
        // (A stop at a return, while stepping: the caller goes on.)
        js::set(&o, "kind", if n == debug::RETURN { "return" } else { "block" });
        if let Some(&l) = t.blocks.get(&(func, n)) {
            js::set(&o, "file", t.file(l));
            js::set(&o, "line", l.line);
            js::set(&o, "column", l.column);
        }
        js::set(&o, "task", to_js(&self.views_task(task)));
        o.into()
    }

    fn views_task(&self, addr: u32) -> ksee_core::view::TaskRef {
        task_ref(addr, live_task(addr))
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

    /// From a stop: run to the next block on the same task (not into
    /// interrupts or other tasks, unless they run on it).
    pub fn step_task(&self) {
        DEBUG.go(debug::TASK);
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

}
