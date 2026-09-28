//! The page's views of the last snapshot of the recording (see
//! ksee_core::view): each is a view, as a JS value.

use ksee_core::view::View;
use ksee_core::Memory;
use wasm_bindgen::prelude::*;

use crate::machine::Machine;
use crate::state::{k, tables, Direct};

/// `{ key: value }`.
fn obj(key: &str, value: JsValue) -> JsValue {
    let o = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&o, &key.into(), &value);
    o.into()
}

fn to_js<T: serde::Serialize>(v: &T) -> JsValue {
    serde_wasm_bindgen::to_value(v).unwrap_or(JsValue::NULL)
}

impl Machine {
    fn view<T>(&self, f: impl FnOnce(&View) -> T) -> T {
        let rec = self.rec.borrow();
        f(&View { tables: tables(), rec: &rec, now: &Direct })
    }

    /// Run `f` on memory as the views should see it (see memory_at).
    fn with_memory<T>(&self, f: impl FnOnce(&dyn Memory) -> T) -> T {
        let Some(at) = *self.past.borrow() else { return f(&Direct) };
        f(&self.rec.borrow().memory_at(&Direct, at))
    }
}

#[wasm_bindgen]
impl Machine {
    /// How many records the last snapshot holds.
    pub fn records(&self) -> usize {
        self.rec.borrow().len()
    }

    /// How many steps the snapshot holds: its blocks, the timeline's unit
    /// of time (a store takes none).
    pub fn steps(&self) -> usize {
        self.rec.borrow().steps()
    }

    /// Record `i`'s step (see Recording::step_of).
    pub fn step_of(&self, i: usize) -> usize {
        self.rec.borrow().step_of(i)
    }

    /// The record step `k` begins at (see Recording::record_of).
    pub fn record_of(&self, k: usize) -> usize {
        self.rec.borrow().record_of(k)
    }

    /// Record `i`'s id: it names the record in later snapshots too, where
    /// its index has moved on.
    pub fn record_id(&self, i: usize) -> Option<u32> {
        self.rec.borrow().ids.get(i).copied()
    }

    /// The record with id `id` (see record_id), if the snapshot still has
    /// it. (A call's arguments have its block's id: the block's.)
    pub fn record_index(&self, id: u32) -> Option<usize> {
        let ids = &self.rec.borrow().ids;
        let i = ids.partition_point(|&x| x < id);
        (ids.get(i) == Some(&id)).then_some(i)
    }

    /// The calls of `task` in records `first..=last` (a call's: its
    /// callees) as a tree (ksee_core::stack::call_tree: in order, inlined
    /// code as calls of its own; a syscall's wrappers are not nodes), as JS
    /// values.
    pub fn call_tree(&self, first: usize, last: usize, task: u32) -> JsValue {
        let rec = self.rec.borrow();
        to_js(&rec.calls.call_tree(tables(), &rec.records, &rec.tasks, first + 1, last, task))
    }

    /// The calls of function `name` in records `first..=last`: each call's
    /// first record.
    pub fn calls_of(&self, name: &str, first: usize, last: usize) -> Vec<u32> {
        let ids: Vec<u32> = tables().functions.iter().filter(|(_, f)| f.name == name).map(|(&id, _)| id).collect();
        ksee_core::stack::calls_of(&self.rec.borrow().records, first, last, &ids).into_iter().map(|i| i as u32).collect()
    }

    /// Where the call from record `first` ends (its return), or None if it
    /// has not when the recording does (see ksee_core::stack::Calls).
    pub fn call_end(&self, first: usize) -> Option<usize> {
        self.rec.borrow().calls.call_end(first)
    }

    /// Show memory (object, object_by_id, global, call_stack's locals) as it
    /// was after record `i`, or as it is now (None): how many stores that
    /// undoes.
    pub fn memory_at(&self, i: Option<usize>) -> usize {
        *self.past.borrow_mut() = i;
        i.map_or(0, |i| self.rec.borrow().stores().after(i))
    }

    /// The syscalls (view::SyscallSummary), in the order they began.
    pub fn syscalls(&self) -> JsValue {
        self.view(|v| to_js(&v.syscalls()))
    }

    /// Call `id`'s syscall (see Machine::call), an index in syscalls(): the
    /// first of its number its runner made since it was asked, if the
    /// snapshot has it.
    pub fn syscall_of(&self, id: u32) -> Option<usize> {
        let (tid, nr, from) = k(|k| k.calls.borrow().get(&id).map(|c| (c.tid, c.nr, c.from)))?;
        let rec = self.rec.borrow();
        rec.syscalls.iter().position(|s| {
            s.nr == nr && rec.ids[s.first] >= from
                && ksee_core::task::read_task(tables(), &Direct, s.task).is_some_and(|t| t.pid as u32 == tid)
        })
    }

    /// Where the kernel was at record `i` (view::At).
    pub fn at(&self, i: usize) -> JsValue {
        self.view(|v| to_js(&v.at(i)))
    }

    /// Who the task at address `task` is (view::TaskRef).
    pub fn task(&self, task: u32) -> JsValue {
        self.view(|v| to_js(&v.task(task)))
    }

    /// The next (dir > 0) or previous record from `i` that runs line `line`
    /// of `file` (see Recording::run_of): "run to" a line.
    pub fn run_of(&self, file: &str, line: u32, i: usize, dir: i32) -> Option<usize> {
        self.rec.borrow().run_of(tables(), file, line, i, dir)
    }

    /// See ksee_core::recording::Recording::seek.
    pub fn seek(&self, i: usize, dir: i32, unit: &str) -> usize {
        self.rec.borrow().seek(tables(), i, dir, unit)
    }

    /// The first call of function `name` in records `first..=last` (of the
    /// task at address `task`, or any): its record.
    pub fn first_call(&self, name: &str, first: usize, last: usize, task: Option<u32>) -> Option<usize> {
        self.rec.borrow().first_call(tables(), name, first, last, task)
    }

    /// The source lines run in records `first..=last` of the task at address
    /// `task`, in order (view::Line).
    pub fn line_steps(&self, first: usize, last: usize, task: u32) -> JsValue {
        self.view(|v| to_js(&v.lines(first, last, task)))
    }

    /// A stack chart of records `first..=last` in `n` columns of equal
    /// steps (see ksee_core::stack::Calls::chart, Recording::columns), flattened:
    /// per column, the task, the
    /// depth, then per call, outermost first, (function, first record,
    /// inlined): inlined 0 for a call; for code inlined into the call
    /// before, 1 + the inlined function (see inline_name), and the
    /// function it is inlined into.
    pub fn chart(&self, first: usize, last: usize, n: usize) -> Vec<u32> {
        let rec = self.rec.borrow();
        let mut out = Vec::new();
        if rec.is_empty() || n == 0 || first > last {
            return out;
        }
        let columns = rec.columns(first, last.min(rec.len() - 1), n);
        self.view(|v| {
            for (task, calls) in rec.calls.chart(v.tables, &rec.tasks, &columns) {
                // (Each call, then the code inlined into it: outermost first.)
                let rows: Vec<[u32; 3]> = calls.iter().flat_map(|c| {
                    v.nest(c).into_iter().rev().map(|(k, _)| [c.func, c.first as u32, k.map_or(0, |k| k + 1)])
                }).collect();
                out.extend([task, rows.len() as u32]);
                out.extend(rows.into_iter().flatten());
            }
        });
        out
    }

    /// The name of inlined function `k` (see chart).
    pub fn inline_name(&self, k: u32) -> String {
        self.view(|v| v.inline_name(k))
    }

    /// The call stack at record `i` (or the last), innermost first
    /// (view::Frame), a call where its last line is (at `i`: the `nth` line
    /// that record runs, see line_steps); its locals as memory was then
    /// (memory_at).
    pub fn call_stack(&self, i: Option<usize>, nth: Option<usize>) -> JsValue {
        let Some(i) = i.or(self.records().checked_sub(1)) else { return js_sys::Array::new().into() };
        self.with_memory(|mem| self.view(|v| to_js(&v.call_stack(i, nth, mem))))
    }

    /// The newest write to `addr..addr + len` at or before the moment
    /// memory_at shows (view::Write), or null if the recording has none.
    pub fn last_write(&self, addr: u32, len: u32) -> JsValue {
        let at = self.past.borrow().unwrap_or(usize::MAX);
        self.view(|v| to_js(&v.last_write(addr, len, at)))
    }

    /// Where the value at `addr..addr + len` came from, as memory_at shows
    /// it: its newest write, and back through the copies it was made by
    /// (view::Write, newest first; see View::origin).
    pub fn origin(&self, addr: u32, len: u32) -> JsValue {
        let at = self.past.borrow().unwrap_or(usize::MAX);
        self.view(|v| to_js(&v.origin(addr, len, at, 8)))
    }

    /// Records `from..=to` as text, a line each (the raw trace):
    /// [[record, text]].
    pub fn raw(&self, from: usize, to: usize) -> JsValue {
        self.view(|v| to_js(&v.raw(from, to)))
    }

    /// The stores in records `first..=last` (a call's), in order, each as
    /// the code names what it hit (view::Stored).
    pub fn stores(&self, first: usize, last: usize) -> JsValue {
        self.view(|v| to_js(&v.stores(first, last)))
    }

    /// The objects records `first..=last` (a call's) wrote, and their
    /// pointers to each other as memory was at record `at` (view::Node).
    pub fn objects(&self, first: usize, last: usize, at: usize) -> JsValue {
        self.view(|v| to_js(&v.objects(first, last, at)))
    }

    /// An object's history: the newest `limit` writes (or all) to the value
    /// of type `ty` at `addr` in records `from..=to` (its lifetime: a local's
    /// frame; else the whole recording), oldest first (view::Change).
    pub fn writes(&self, addr: u32, ty: u32, from: Option<usize>, to: Option<usize>, limit: Option<usize>) -> JsValue {
        self.view(|v| to_js(&v.writes(addr, ty, from.unwrap_or(0)..=to.unwrap_or(usize::MAX), limit.unwrap_or(usize::MAX))))
    }

    /// The task running at the start of each of `n` buckets of the
    /// recording, for its overview (view::TaskRef).
    pub fn timeline(&self, n: usize) -> JsValue {
        self.view(|v| to_js(&v.timeline(n)))
    }

    /// Syscall `name` (e.g. "openat"): view::SyscallInfo, or null.
    pub fn syscall(&self, name: &str) -> JsValue {
        self.view(|v| to_js(&v.syscall(name)))
    }

    /// Argument `text` (written as in C: see ksee_core::args) of syscall
    /// `syscall`'s parameter `k`, as Machine::call takes it: a number, a
    /// string, { bytes }, { size } or { strings }; or an error. `known`: {
    /// name: value }, the results so far ($1, fd).
    pub fn arg(&self, syscall: &str, k: usize, text: &str, known: JsValue) -> Result<JsValue, JsValue> {
        use ksee_core::args::Arg;
        let types = &tables().types;
        let ty = types.syscall_params(syscall).and_then(|p| p.get(k)).map_or(0, |(_, t)| *t);
        let lookup = |n: &str| types.constants.get(n).copied().or_else(|| js_sys::Reflect::get(&known, &n.into()).ok()?.as_f64().map(|v| v as i64));
        Ok(match types.arg(text, ty, &lookup)? {
            Arg::Number(v) => JsValue::from_f64(v as f64),
            Arg::Size(n) => obj("size", n.into()),
            Arg::Bytes(b) => obj("bytes", js_sys::Uint8Array::from(&b[..]).into()),
            Arg::Strings(v) => obj("strings", v.into_iter().map(|s| s.map_or(JsValue::NULL, JsValue::from)).collect::<js_sys::Array>().into()),
        })
    }

    /// The value of `text`, a C constant expression (a runner's thread id,
    /// say: "19", "child", "$2"); `known` as for arg.
    pub fn value(&self, text: &str, known: JsValue) -> Result<f64, JsValue> {
        let types = &tables().types;
        let lookup = |n: &str| types.constants.get(n).copied().or_else(|| js_sys::Reflect::get(&known, &n.into()).ok()?.as_f64().map(|v| v as i64));
        Ok(ksee_core::args::eval(text, &lookup)? as f64)
    }

    /// Constants from the kernel's UAPI headers (O_*, AT_*, CLOCK_*, and
    /// errno's E*): { name: value }.
    pub fn constants(&self) -> JsValue {
        let o = js_sys::Object::new();
        for (name, &v) in &tables().types.constants {
            let _ = js_sys::Reflect::set(&o, &name.into(), &(v as f64).into());
        }
        o.into()
    }

    /// The syscalls' names (the kernel's syscall table).
    pub fn syscall_names(&self) -> Vec<String> {
        tables().types.syscalls.values().cloned().collect()
    }

    /// Function `id`: where it is (view::Place).
    pub fn function(&self, id: u32) -> JsValue {
        self.view(|v| to_js(&v.place(id)))
    }

    /// Where `name` is defined (view::Place): a function, a type, a global
    /// or a member ("file.f_pos"); or null.
    pub fn definition(&self, name: &str) -> JsValue {
        self.view(|v| to_js(&v.definition(name)))
    }

    /// The functions called `name` (view::Function).
    pub fn functions(&self, name: &str) -> JsValue {
        self.view(|v| to_js(&v.functions(name)))
    }

    /// Every line of `file` where a block starts, with how often blocks
    /// there ran in syscall `n` (or the whole recording): [{ line, count }].
    pub fn file_coverage(&self, file: &str, n: Option<usize>) -> JsValue {
        self.view(|v| to_js(&v.coverage(file, n)))
    }

    /// The value of the type named `name` at `addr` (view::Object),
    /// aggregates expanded `depth` levels.
    pub fn object(&self, name: &str, addr: u32, depth: Option<u32>) -> JsValue {
        match tables().types.find(name) {
            Some(ty) => self.object_by_id(name, ty, addr, depth),
            None => JsValue::NULL,
        }
    }

    /// The fields of the task at `addr` as memory_at shows them
    /// (view::TaskFields): the key ones, or `all`.
    pub fn task_fields(&self, addr: u32, all: bool) -> JsValue {
        self.with_memory(|mem| self.view(|v| to_js(&v.task_fields(mem, addr, all))))
    }

    /// Like `object`, by type id (an Object's `expand` or `target`).
    pub fn object_by_id(&self, label: &str, ty: u32, addr: u32, depth: Option<u32>) -> JsValue {
        self.with_memory(|mem| to_js(&tables().types.object(mem, label, addr, ty, depth.unwrap_or(1))))
    }

    /// The value of the variable `name` (view::Object), or null.
    pub fn global(&self, name: &str, depth: Option<u32>) -> JsValue {
        match tables().types.globals.get(name) {
            Some(&(addr, ty)) => self.object_by_id(name, ty, addr, depth),
            None => JsValue::NULL,
        }
    }
}
