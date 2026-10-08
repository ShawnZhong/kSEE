//! The lines a span ran (View::lines), and the Stack at a moment
//! (View::call_stack): its calls, their arguments and locals, the code
//! inlined into them.

use super::*;

/// A call a line made: its function, what it returned, and where (its
/// Return record).
#[derive(Debug, Clone, PartialEq)]
pub struct Called {
    pub func: String,
    pub value: i64,
    pub at: usize,
}

/// A source line the kernel ran (see stack::Calls::lines), at record `i`, the
/// `nth` of the lines it runs there (see call_stack).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub i: usize,
    /// The record just before it runs (LineStep::at): its values as they
    /// are then.
    pub at: usize,
    pub nth: u32,
    /// A line calling code inlined, not yet in it (LineStep::cut).
    pub cut: u32,
    pub func: String,
    pub file: String,
    pub line: u32,
    /// The function the line is inlined from, if.
    pub inlined: Option<String>,
    /// How deep it runs: calls deep, and inlined (how many functions its
    /// code is inlined from): a stepper's level.
    pub depth: u32,
    pub inl: u32,
    /// Which turn of a loop it is (see LineStep::run).
    pub run: u32,
    /// The comparisons it made that decided where it went ("18 >= 0 ✓"),
    /// in order.
    pub tests: Vec<String>,
    /// It made a call: the function, what it returned, and where (its
    /// return: the result is not known before).
    pub called: Option<Called>,
}

/// A call on a stack, with its arguments and its locals in memory.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub func: String,
    /// The call's first record (its block 0).
    pub first: usize,
    /// Code inlined into the call (the frame outside it), not a call.
    pub inlined: bool,
    /// Where the call is (the line it runs, or the one calling the frame
    /// inside it; else where its function is).
    pub file: Option<String>,
    pub line: Option<u32>,
    pub args: Vec<Object>,
    pub locals: Vec<Object>,
}

/// The line a step goes to from line `k` of `lines`, in direction `dir`
/// (1: on; -1: back), if any, as a debugger's: the next line, into the
/// calls; or, `over` them, the next as deep as line k or shallower (code
/// inlined counts as a level deeper).
pub fn step(lines: &[Line], k: usize, dir: isize, over: bool) -> Option<usize> {
    let here = lines.get(k).map(|l| (l.depth, l.inl))?;
    seek(lines, k, dir, |l| !over || (l.depth, l.inl) <= here)
}

/// The next (`dir` 1) or previous (`dir` -1) run of `file`'s `line`, from
/// line `k` of the recording's ordered `lines`.
pub fn run_to(lines: &[Line], k: usize, file: &str, line: u32, dir: isize) -> Option<usize> {
    seek(lines, k, dir, |l| l.file == file && l.line == line)
}

/// The first of `lines` from line `k` in direction `dir` (none: 0) that `is` says yes to.
fn seek(lines: &[Line], k: usize, dir: isize, is: impl Fn(&Line) -> bool) -> Option<usize> {
    if dir == 0 {
        return None;
    }
    let mut j = k.checked_add_signed(dir)?;
    while j < lines.len() {
        if is(&lines[j]) {
            return Some(j);
        }
        j = j.checked_add_signed(dir)?;
    }
    None
}

impl View<'_> {
    /// Line step `s` as the page has it, with the comparisons it made.
    fn line(&self, s: &LineStep) -> Line {
        // (Its comparisons: among its stretch's records, those on it.)
        let (records, tasks) = (&self.rec.records, &self.rec.tasks);
        let task = tasks.get(s.record).copied();
        let tests = (s.record + 1..records.len()).filter(|&j| Some(tasks[j]) == task)
            .map(|j| records[j]).take_while(|r| r.line().is_some())
            .filter(|r| matches!(r, Record::Compare { line, .. } if *line == s.index))
            .filter_map(|r| compared(&r)).collect();
        Line { tests, ..self.line_only(s) }
    }

    /// Where line step `s` is (a store's, a write's place): the line, not
    /// what it compared.
    pub(super) fn line_only(&self, s: &LineStep) -> Line {
        let chain = &self.tables.chain(s.inlined)[s.cut as usize..];
        Line { i: s.record, at: s.at, nth: s.nth, cut: s.cut, func: self.name(s.func), file: self.tables.file(s.loc).to_string(), line: s.loc.line,
               inlined: chain.first().map(|&(f, _)| self.inline_name(f)), depth: s.depth, inl: chain.len() as u32, run: s.run, tests: Vec::new(),
               called: s.callee.map(|(f, value, at)| Called { func: self.name(f), value, at }) }
    }

    /// The line record `i` is on, if it has one of its own (a store, a
    /// comparison, a variable set): its block's record and which of its
    /// lines (Line::i, Line::nth).
    pub fn line_of(&self, i: usize) -> Option<(usize, u32)> {
        self.rec.line_at(self.tables, i).map(|s| (s.record, s.nth))
    }

    /// The source lines run in records `first..=last` of the task at `task`.
    pub fn lines(&self, first: usize, last: usize, task: u32) -> Vec<Line> {
        self.rec.lines(self.tables, first, last, task).iter().map(|s| self.line(s)).collect()
    }

    /// The call stack at record `i`, innermost first, its values as memory
    /// was after record `at` (a line step's). A call is where its last line
    /// is: at `i`, the `nth` line that record runs (see lines), or its
    /// last. Code inlined there is a frame of its own for each function it
    /// is inlined from (`inlined`, with no arguments: there was no call).
    /// (`cut`: a line calling code inlined, the innermost frames it is not
    /// in yet left out, Line::cut.)
    pub fn call_stack(&self, i: usize, nth: Option<usize>, cut: usize, at: usize) -> Vec<Frame> {
        let t = self.tables;
        let mem = &self.rec.memory_at(self.now, at);
        let mut out = Vec::new();
        // (At a record on a line of its own, a store, a comparison, a
        // variable set: on that line, as `at` says it.)
        let calls = match nth.is_none().then(|| self.rec.line_at(t, i)).flatten() {
            Some(s) => self.rec.calls.stack_at(t, s.record, Some(s.nth as usize)),
            None => self.rec.calls.stack_at(t, i, nth),
        };
        let args_of = |c: &stack::StackCall| self.rec.args.get(c.first).map(|(w, frame)| (t.values(c.func, w), frame));
        // (The syscall, by its entry's arguments (arch::SYSCALL): its name,
        // and its arguments, the entry's and its wrapper's (a wrapper's
        // parameters are the syscall's, as its SYSCALL_DEFINE line has them;
        // its own DWARF one is regs).)
        let entry = t.find(arch::SYSCALL);
        let syscall = calls.iter().find(|c| Some(c.func) == entry).and_then(args_of).and_then(|(a, _)| self.syscall_args(&a, mem));
        for c in calls.into_iter().rev() {
            let name = self.name(c.func);
            let (raw, frame) = args_of(&c).unzip();
            let at_entry = Some(c.func) == entry;
            let args = raw.map(|raw| match &syscall {
                Some(a) if at_entry || name.starts_with(arch::SYSCALL_WRAPPER) => a.clone(),
                _ => self.args(c.func, raw, mem),
            });
            let args = args.unwrap_or_default();
            // (The variables it keeps in wasm locals, as they are by now: a
            // parameter it changed, as it is; the other locals after those
            // in memory. And the code inlined into it: its own.)
            let vars = self.var_values(&c, at);
            // (Its function's, in the order it declares them.)
            let mine = |function: &str, param: bool| {
                let mut o: Vec<((bool, usize), Object)> = vars.iter().filter(|((f, _, _), (v, _))| *f == function && (v.param > 0) == param)
                    .filter_map(|(_, (v, value))| Some((self.declared(v), self.var_object(v, *value, mem)?))).collect();
                o.sort_by_key(|(k, _)| *k);
                o.into_iter().map(|(_, o)| o).collect::<Vec<_>>()
            };
            let changed = mine(&name, true);
            let args = args.into_iter().map(|o| changed.iter().find(|p| p.name == o.name).cloned().unwrap_or(o)).collect();
            let locals = frame.map(|f| self.locals(c.func, f, mem, c.first, at, &mine(&name, false))).unwrap_or_default();
            // (The code inlined into it first, innermost first; then it.)
            let frame = |func: String, inlined, at: Option<Loc>| {
                let (args, locals) = if inlined { (mine(&func, true), mine(&func, false)) } else { (Vec::new(), Vec::new()) };
                Frame { first: c.first, inlined, file: at.map(|l| t.file(l).to_string()), line: at.map(|l| l.line), args, locals, func }
            };
            let mut nest = self.nest(&c);
            if out.is_empty() {
                nest.drain(..cut.min(nest.len() - 1));
            }
            let (&(_, at), inner) = nest.split_last().expect("a call");
            out.extend(inner.iter().map(|&(k, at)| frame(self.inline_name(k.unwrap_or(0)), true, at)));
            out.push(Frame { args, locals, ..frame(name, false, at) });
        }
        out
    }

    /// The variables call `c` keeps in wasm locals (Record::Var), by record
    /// `at`: each one's last value, by its function (the call's, or one
    /// inlined into it: by name) and its name, parameters by number.
    fn var_values(&self, c: &stack::StackCall, at: usize) -> BTreeMap<(&str, u32, &str), (&Var, u64)> {
        let mut out = BTreeMap::new();
        // (One that is the call's argument throughout: that.)
        let args = self.rec.args.get(c.first).map(|(w, _)| self.tables.values(c.func, w)).unwrap_or_default();
        for (_, v) in self.tables.vars.range((c.func, 0)..=(c.func, u32::MAX)).filter(|(_, v)| v.arg > 0) {
            if let Some(&value) = args.get(v.arg as usize - 1) {
                out.insert((&*v.function, v.param, &*v.name), (v, value));
            }
        }
        for j in self.rec.calls.vars_of(c.first, at) {
            if let Record::Var { var, value, .. } = self.rec.records[j]
                && let Some(v) = self.tables.vars.get(&(c.func, var)) {
                out.insert((&*v.function, v.param, &*v.name), (v, value));
            }
        }
        out
    }

    /// Where variable `v` is among its function's: a parameter by its
    /// number, a local after them, in the order it declares them.
    fn declared(&self, v: &Var) -> (bool, usize) {
        match v.param {
            0 => (true, self.tables.types.locals.get(&v.of).and_then(|l| l.iter().position(|l| *l.0 == *v.name)).unwrap_or(0)),
            k => (false, k as usize),
        }
    }

    /// Variable `v` with `value`, as a value of its type (its function's
    /// parameter's, or local's).
    pub(super) fn var_object(&self, v: &Var, value: u64, mem: &dyn Memory) -> Option<Object> {
        Some(self.tables.types.object(&Arg(&value.to_le_bytes(), mem), &v.name, 0, self.var_ty(v)?, 0))
    }

    /// Variable `v`'s type: its function's parameter's, or local's.
    pub(super) fn var_ty(&self, v: &Var) -> Option<u32> {
        let types = &self.tables.types;
        Some(match v.param {
            0 => types.locals.get(&v.of)?.iter().find(|l| *l.0 == *v.name)?.1,
            k => types.params.get(&v.of)?.get(k as usize - 1)?.1,
        })
    }


    /// The syscall entry's arguments (the number, then the syscall's): the
    /// syscall's arguments as its parameters have them (with the constants
    /// that name them).
    fn syscall_args(&self, entry: &[u64], mem: &dyn Memory) -> Option<Vec<Object>> {
        let types = &self.tables.types;
        let name = types.syscalls.get(&(*entry.first()? as u32))?;
        let params = types.syscall_params(name)?;
        let args = params.iter().zip(entry.iter().skip(1).copied()).map(|((pname, ty), v)| {
            let mut o = types.object(&Arg(&v.to_le_bytes(), mem), pname, 0, *ty, 0);
            let names = constant_name(&types.constants, crate::arch::consts_of(name, pname), v as i32 as i64);
            o.value = names.or(o.value);
            o
        }).collect();
        Some(args)
    }

    /// Function `func`'s arguments, their values `args`: the source's
    /// parameters, as values of their types.
    fn args(&self, func: u32, args: Vec<u64>, mem: &dyn Memory) -> Vec<Object> {
        let t = self.tables;
        let known = t.functions.get(&func).map_or(u32::MAX, |f| f.known);
        t.types.params.get(&func).into_iter().flatten().zip(args).enumerate().map(|(k, ((pname, ty), v))| {
            let arg = t.types.object(&Arg(&v.to_le_bytes(), mem), pname, 0, *ty, 0);
            // (Not passed any more: the optimizer took it away.)
            if known >> k & 1 == 0 {
                return arg.missing(Missing::OptimizedOut);
            }
            arg
        }).collect()
    }

    /// Function `func`'s locals, each name once, in the order it declares them (the same
    /// rows at every moment, as a debugger's), as the call from record
    /// `first` has them at record `at`: one in memory at its offset from
    /// `frame`, uninitialized if its slot has not been stored to (its
    /// value then is its slot's leftovers, another call's); one in a wasm
    /// local as `set` has it (Record::Var), else not recorded (set before
    /// the trace began, or not followed: not uninitialized).
    fn locals(&self, func: u32, frame: u32, mem: &dyn Memory, first: usize, at: usize, set: &[Object]) -> Vec<Object> {
        let types = &self.tables.types;
        // (A name once: two variables of one name, in two of its scopes, are
        // one to the recording, which knows a variable by its name.)
        let mut seen = alloc::collections::BTreeSet::new();
        types.locals.get(&func).into_iter().flatten().filter(|(var, _, _)| seen.insert(var.as_str())).map(|(var, ty, off)| match off {
            Some(off) => {
                let addr = frame.wrapping_add(*off);
                let o = types.object(mem, var, addr, *ty, 0);
                // (Stored to in the call, by `at`.)
                let written = self.rec.stores().last_write(addr, types.size_of(*ty).max(1), at).is_some_and(|(i, _)| i >= first);
                if written { o } else { o.missing(Missing::Uninitialized) }
            }
            None => set.iter().find(|o| &o.name == var).cloned()
                .unwrap_or_else(|| types.object(&Arg(&[0; 8], mem), var, 0, *ty, 0).missing(Missing::NotRecorded)),
        }).collect()
    }

    /// Call `c` and the code inlined into it where it is, innermost first:
    /// each inlined function (Some: its index, see inline_name) at its line
    /// there, then the call (None) at its own line (where the outermost
    /// inlined code is inlined, or the line it is at).
    pub(crate) fn nest(&self, c: &stack::StackCall) -> Vec<(Option<u32>, Option<Loc>)> {
        let mut at = c.step.map(|s| s.loc).or(self.tables.functions.get(&c.func).map(|f| f.loc));
        let mut out = Vec::new();
        for &(k, site) in self.tables.chain(c.step.and_then(|s| s.inlined)) {
            out.push((Some(k), at));
            at = Some(site);
        }
        out.push((None, at));
        out
    }

    /// Inlined function `k`'s name (Tables::names).
    pub(crate) fn inline_name(&self, k: u32) -> String {
        self.tables.names.get(k as usize).cloned().unwrap_or_default()
    }
}

/// Value `v` as the constants whose names start with one of `prefixes`
/// name it: one equal to it, or flags (one bit each) that make it up.
fn constant_name(constants: &BTreeMap<String, i64>, prefixes: &[&str], v: i64) -> Option<String> {
    let fits: Vec<(&String, i64)> = constants.iter().filter(|(n, _)| prefixes.iter().any(|p| n.starts_with(p))).map(|(n, &x)| (n, x)).collect();
    if let Some((n, _)) = fits.iter().find(|(_, x)| *x == v) {
        return Some((*n).clone());
    }
    if v <= 0 {
        return None;
    }
    let names: Vec<&str> = (0..32).filter(|b| v >> b & 1 != 0)
        .filter_map(|b| fits.iter().find(|(_, x)| *x == 1 << b).map(|(n, _)| n.as_str())).collect();
    (names.len() == v.count_ones() as usize).then(|| names.join(" | "))
}

/// An argument's value, as if it were memory at address 0; past it, memory
/// (what a pointer argument points to).
struct Arg<'a>(&'a [u8], &'a dyn Memory);

impl Memory for Arg<'_> {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        if addr as usize >= self.0.len() {
            return self.1.read(addr, buf);
        }
        for (i, b) in buf.iter_mut().enumerate() {
            *b = self.0.get(addr as usize + i).copied().unwrap_or(0);
        }
    }
}

/// A comparison, its operands and whether it held: "18 >= 0 ✓" (signed
/// ones as such, else as numbers are).
fn compared(r: &Record) -> Option<String> {
    let (lhs, op, rhs, signed, held) = r.comparison()?;
    let v = |x: i128| if signed { x.to_string() } else { num(x as u64) };
    Some(format!("{} {op} {} {}", v(lhs), v(rhs), if held { "✓" } else { "✗" }))
}
