//! The page's views of the last snapshot of the recording (see
//! ksee_core::view): each is a view, as a JS value.

use ksee_core::view::View;
use ksee_core::Memory;
use wasm_bindgen::prelude::*;

use crate::machine::Machine;
use crate::state::{k, tables, Direct};

/// A program's line, for the page (see Machine::statement).
#[derive(serde::Serialize, Default)]
struct Step {
    tid: Option<Tid>,
    text: String,
    r#type: Option<String>,
    name: Option<String>,
    call: Option<String>,
    args: Option<Vec<String>>,
    decl: bool,
    assign: bool,
}

/// A runner as written: a thread id, or a result's name.
#[derive(serde::Serialize)]
#[serde(untagged)]
enum Tid {
    Nr(u32),
    Name(String),
}

/// `v` as a JS value (None: undefined).
pub(crate) fn to_js<T: serde::Serialize>(v: &T) -> JsValue {
    serde_wasm_bindgen::to_value(v).unwrap_or(JsValue::NULL)
}

impl Machine {
    /// Run `f` on a view of the last snapshot (ksee_core::view).
    pub fn view<T>(&self, f: impl FnOnce(&View) -> T) -> T {
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

    /// The calls in records `first..=last` of the functions with `text` in
    /// their names (any case: "read", vfs_read's, filemap_read's): each
    /// call's first record (see Recording::calls: inlined code too).
    pub fn calls_of(&self, text: &str, first: usize, last: usize) -> Vec<u32> {
        let text = text.to_lowercase();
        self.rec.borrow().calls(tables(), |n| n.to_lowercase().contains(&text), first, last, None).map(|i| i as u32).collect()
    }

    /// Where the call from record `first` ends (its return), or None if it
    /// has not when the recording does (see ksee_core::stack::Calls).
    pub fn call_end(&self, first: usize) -> Option<usize> {
        self.rec.borrow().calls.call_end(first)
    }

    /// Show memory (object, global, call_stack's locals) as it
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
        let (task, nr, from) = k(|k| k.calls.borrow().get(&id).map(|c| (c.task, c.nr, c.from)))?;
        let rec = self.rec.borrow();
        rec.syscalls.iter().position(|s| s.nr == nr && s.task == task && rec.ids[s.first] >= from)
    }

    /// Where the kernel was at record `i` (view::At).
    pub fn at(&self, i: usize) -> JsValue {
        self.view(|v| to_js(&v.at(i)))
    }

    /// The tasks the recording has, as they are at record `i` (view::TaskAt).
    pub fn tasks(&self, i: usize) -> JsValue {
        self.view(|v| to_js(&v.tasks(i)))
    }

    /// Who the task at address `task` was at record `at` (view::TaskRef).
    pub fn task(&self, task: u32, at: usize) -> JsValue {
        self.view(|v| to_js(&v.task(task, at)))
    }

    /// The next (dir > 0) or previous record from `i` that runs line `line`
    /// of `file` (see Recording::run_of): "run to" a line.
    pub fn run_of(&self, file: &str, line: u32, i: usize, dir: i32) -> Option<usize> {
        self.rec.borrow().run_of(tables(), file, line, i, dir)
    }

    /// See ksee_core::recording::Recording::next_block.
    pub fn next_block(&self, i: usize, other: bool) -> usize {
        self.rec.borrow().next_block(i, other)
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

    /// A stack chart of records `first..=last` in `n` columns (view::Chart).
    pub fn chart(&self, first: usize, last: usize, n: usize) -> JsValue {
        self.view(|v| to_js(&v.chart(first, last, n)))
    }

    /// The call stack at record `i` (or the last), innermost first
    /// (view::Frame), a call where its last line is (at `i`: the `nth` line
    /// that record runs, see line_steps); its locals as memory was then
    /// (memory_at).
    pub fn call_stack(&self, i: Option<usize>, nth: Option<usize>) -> JsValue {
        let Some(i) = i.or(self.records().checked_sub(1)) else { return js_sys::Array::new().into() };
        let at = self.past.borrow().unwrap_or(i);
        self.with_memory(|mem| self.view(|v| to_js(&v.call_stack(i, nth, mem, at))))
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

    /// What each line of `file` stored or set in records `first..=last` by
    /// record `cur` (view::Note): the source's notes.
    pub fn notes(&self, first: usize, last: usize, file: &str, cur: usize) -> JsValue {
        self.view(|v| to_js(&v.notes(first, last, file, cur)))
    }

    /// What records `first..=last` (a call's) did to the kernel's data: the
    /// objects they changed, each member from what to what, and their
    /// pointers to each other as the span ends (view::Node).
    pub fn objects(&self, first: usize, last: usize) -> JsValue {
        self.view(|v| to_js(&v.objects(first, last)))
    }

    /// The task running at the start of each of `n` buckets of the
    /// recording, for its overview (view::TaskRef).
    pub fn timeline(&self, n: usize) -> JsValue {
        self.view(|v| to_js(&v.timeline(n)))
    }

    /// Syscall `name`'s parameters, each as C declares it (Types::params_of):
    /// ["int dfd", "char *filename", …]; null if this kernel has no syscall `name`.
    pub fn params(&self, name: &str) -> Option<Vec<String>> {
        tables().types.params_of(name)
    }

    /// A program's line (ksee_core::program::statement): { tid?, text (as
    /// written, but for its runner) } and a call's { type?, name?, call,
    /// args (as written) }, a declaration's { decl: true }, or an
    /// assignment's { assign: true }; tid a number if it is one, else the
    /// name of a result (a clone's).
    pub fn statement(&self, text: &str) -> Result<JsValue, JsValue> {
        use ksee_core::program::{statement, Line};
        let st = statement(text)?;
        let mut s = Step {
            tid: st.runner.map(|t| t.parse().map_or(Tid::Name(t), Tid::Nr)),
            text: st.text,
            ..Default::default()
        };
        match st.line {
            Line::Call { result, call, texts, .. } => {
                (s.r#type, s.name) = result.map(|(ty, name)| (ty.text(), name)).unzip();
                (s.call, s.args) = (Some(call), Some(texts));
            }
            Line::Decl { .. } => s.decl = true,
            Line::Assign { .. } => s.assign = true,
        }
        Ok(to_js(&s))
    }

    /// The names in `text`, a line of C source, each with the fields
    /// before it (ksee_core::program::names): what a hover is of.
    pub fn names(&self, text: &str) -> JsValue {
        to_js(&ksee_core::program::names(text))
    }

    /// A program written on one line: its lines (ksee_core::program::lines,
    /// `;` outside strings between them).
    pub fn program_lines(&self, text: &str) -> Vec<String> {
        ksee_core::program::lines(text).into_iter().map(String::from).collect()
    }

    /// Where the cursor at `at` (UTF-16 units) is in `text`, a program's
    /// line being written (ksee_core::program::context): { call,
    /// call_start, word, word_start, param }.
    pub fn context(&self, text: &str, at: usize) -> JsValue {
        to_js(&ksee_core::program::context(text, at))
    }

    /// What fits at the cursor `at` of `text`, line `line` of the program
    /// (ksee_core::program::completions; `common`: the syscalls to list
    /// first): [{ text, label, detail, name }].
    pub fn completions(&self, text: &str, at: usize, line: usize, common: Vec<String>) -> JsValue {
        let c = ksee_core::program::context(text, at);
        k(|k| to_js(&ksee_core::program::completions(&tables().types, &c, &k.names.borrow().before(line), &common)))
    }

    /// A call's result as the page says it (Types::result): "3", "0x1d20000", "-2 (ENOENT)".
    pub fn result(&self, v: f64) -> String {
        tables().types.result(v as i64)
    }

    /// The kernel's areas (ksee_core::arch::AREAS): [[a file's start, its
    /// name], …], for the timeline's colors.
    pub fn areas(&self) -> JsValue {
        ksee_core::arch::AREAS.iter().map(|&(p, n)| js_sys::Array::of2(&p.into(), &n.into())).collect::<js_sys::Array>().into()
    }

    /// A program's variable (see Machine::declare) as memory_at shows it (at
    /// the cursor), as C writes it.
    pub fn show_at_cursor(&self, ty: u32, addr: u32, count: Option<u32>) -> String {
        self.with_memory(|mem| tables().types.show(mem, addr, ty, count))
    }

    /// Where `name` is defined (view::Place): a function, a type, a global
    /// or a member ("file.f_pos": see member_definition); or null.
    pub fn definition(&self, name: &str) -> JsValue {
        self.view(|v| to_js(&v.definition(name)))
    }

    /// Where member `member` of type `ty` (or of what a pointer of it
    /// points to; none: of the one struct with it) is declared
    /// (view::Place), or null.
    pub fn member_definition(&self, ty: Option<u32>, member: &str) -> JsValue {
        self.view(|v| to_js(&v.member_definition(ty, member)))
    }

    /// Every line of `file` where a block starts, with how often blocks
    /// there ran in syscall `n` (or the whole recording): [{ line, count }].
    pub fn file_coverage(&self, file: &str, n: Option<usize>) -> JsValue {
        self.view(|v| to_js(&v.coverage(file, n)))
    }

    /// The fields of the task at `addr` as memory_at shows them
    /// (view::TaskFields): the key ones first.
    pub fn task_fields(&self, addr: u32) -> JsValue {
        self.with_memory(|mem| self.view(|v| to_js(&v.task_fields(mem, addr))))
    }

    /// The value of type `ty` at `addr` (view::Object: what an Object's
    /// `open` says), named `label`, aggregates read `depth` levels, as
    /// memory_at shows it.
    pub fn object(&self, label: &str, ty: u32, addr: u32, depth: Option<u32>) -> JsValue {
        self.with_memory(|mem| to_js(&tables().types.object(mem, label, addr, ty, depth.unwrap_or(1))))
    }

    /// The value of the variable `name` (view::Object), or null.
    pub fn global(&self, name: &str, depth: Option<u32>) -> JsValue {
        match tables().types.globals.get(name) {
            Some(&(addr, ty)) => self.object(name, ty, addr, depth),
            None => JsValue::NULL,
        }
    }
}
