//! The lines a span ran (View::lines), and the Stack at a moment
//! (View::call_stack): its calls, their arguments and locals, the code
//! inlined into them.

use super::*;

/// A source line the kernel ran (see stack::lines), at record `i`, the
/// `nth` of the lines it runs there (see call_stack).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Line {
    pub i: usize,
    /// The record the line is at: its first store's, else `i` (LineStep::at).
    pub at: usize,
    pub nth: u32,
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
    /// The call returns after it: what it returned (32 bits).
    pub ret: Option<i64>,
    /// It made a call: the function, what it returned, and where (its
    /// return: the result is not known before).
    pub called: Option<(String, i64, usize)>,
    /// Inlined: the line of its function's own where the code is inlined
    /// (the outermost inline site: `CLASS(fd_pos, f)(fd)`'s line).
    pub site: Option<u32>,
}

/// A call on a stack, with its arguments (32 bits each: wider ones lose
/// their high half) and its locals in memory.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Frame {
    pub func: String,
    /// The call's first record (its block 0).
    pub first: usize,
    /// How many calls deep it is (as line steps count: see Line::depth).
    pub depth: u32,
    /// The syscall entry (arch::SYSCALL): the syscall's name; its
    /// arguments are the syscall's, typed as its parameters.
    pub syscall: Option<String>,
    /// Code inlined into the call (the frame outside it), not a call.
    pub inlined: bool,
    /// Where the call is (the line it runs, or the one calling the frame
    /// inside it; else where its function is).
    pub file: Option<String>,
    pub line: Option<u32>,
    pub args: Option<Vec<Object>>,
    pub locals: Option<Vec<Object>>,
}

impl View<'_> {
    /// Line step `s` as the page has it, with the comparisons it made.
    fn line(&self, s: &LineStep) -> Line {
        // (Its comparisons: among its stretch's records, those on it.)
        let (records, tasks) = (&self.rec.records, &self.rec.tasks);
        let task = tasks.get(s.record).copied();
        let tests = (s.record + 1..records.len()).filter(|&j| Some(tasks[j]) == task)
            .map(|j| records[j]).take_while(|r| matches!(r, Record::Store { .. } | Record::Compare { .. } | Record::Var { .. }))
            .filter(|r| matches!(r, Record::Compare { line, .. } if *line == s.index))
            .filter_map(|r| compared(&r)).collect();
        Line { tests, ..self.line_only(s) }
    }

    /// Where line step `s` is (a store's, a write's place): the line, not
    /// what it compared.
    pub(super) fn line_only(&self, s: &LineStep) -> Line {
        Line { i: s.record, at: s.at, nth: s.nth, func: self.name(s.func), file: self.tables.file(s.loc).to_string(), line: s.loc.line,
               inlined: self.inlined_from(s.inlined), depth: s.depth, inl: self.chain(s.inlined).len() as u32, run: s.run, tests: Vec::new(), ret: s.ret,
               called: s.callee.map(|(f, v, at)| (self.name(f), v, at)), site: self.chain(s.inlined).last().map(|&(_, l)| l.line) }
    }

    /// The source lines run in records `first..=last` of the task at `task`.
    pub fn lines(&self, first: usize, last: usize, task: u32) -> Vec<Line> {
        self.rec.lines(self.tables, first, last, task).iter().map(|s| self.line(s)).collect()
    }

    /// The call stack at record `i`, innermost first; locals read from `mem`
    /// (memory as it was then). A call is where its last line is: at `i`,
    /// the `nth` line that record runs (see lines), or its last. Code
    /// inlined there is a frame of its own for each function it is inlined
    /// from (`inlined`, with no arguments: there was no call).
    /// (`at`: the moment `mem` is memory as it was at, for the variables'
    /// values too: a line step's.)
    pub fn call_stack(&self, i: usize, nth: Option<usize>, mem: &dyn Memory, at: usize) -> Vec<Frame> {
        let t = self.tables;
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
        for (depth, c) in calls.into_iter().enumerate().rev() {
            let depth = depth as u32;
            let name = self.name(c.func);
            let (raw, frame) = args_of(&c).unzip();
            let at_entry = Some(c.func) == entry;
            let args = raw.map(|raw| match &syscall {
                Some((_, a)) if at_entry || name.starts_with(arch::SYSCALL_WRAPPER) => a.clone(),
                _ => self.args(c.func, raw, mem),
            });
            let syscall = syscall.as_ref().filter(|_| at_entry && args.is_some()).map(|(n, _)| n.clone());
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
            let args = args.map(|a| a.into_iter().map(|o| changed.iter().find(|p| p.name == o.name).cloned().unwrap_or(o)).collect());
            let locals = frame.map(|f| self.locals(c.func, f, mem, c.first, at, &mine(&name, false)));
            // (The code inlined into it first, innermost first; then it.)
            let frame = |func: String, inlined, at: Option<Loc>| {
                let (a, l) = if inlined { (mine(&func, true), mine(&func, false)) } else { (Vec::new(), Vec::new()) };
                Frame { first: c.first, depth, syscall: None, inlined, file: at.map(|l| t.file(l).to_string()), line: at.map(|l| l.line),
                        args: (!a.is_empty()).then_some(a), locals: (!l.is_empty()).then_some(l), func }
            };
            let nest = self.nest(&c);
            let (&(_, at), inner) = nest.split_last().expect("a call");
            out.extend(inner.iter().map(|&(k, at)| frame(self.inline_name(k.unwrap_or(0)), true, at)));
            out.push(Frame { args, locals, syscall, ..frame(name, false, at) });
        }
        out
    }

    /// Variable `var` of the function running at record `i` (a Var
    /// record's).
    pub(super) fn var_at(&self, i: usize, var: u32) -> Option<&Var> {
        self.tables.vars.get(&(self.rec.calls.func_at(i)?, var))
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
                out.insert((v.function.as_str(), v.param, v.name.as_str()), (v, value));
            }
        }
        for j in self.rec.calls.vars_of(c.first, at) {
            if let Record::Var { var, value, .. } = self.rec.records[j]
                && let Some(v) = self.tables.vars.get(&(c.func, var)) {
                out.insert((v.function.as_str(), v.param, v.name.as_str()), (v, value));
            }
        }
        out
    }

    /// Where variable `v` is among its function's: a parameter by its
    /// number, a local after them, in the order it declares them.
    fn declared(&self, v: &Var) -> (bool, usize) {
        match v.param {
            0 => (true, self.tables.types.locals.get(&v.of).and_then(|l| l.iter().position(|l| l.0 == v.name)).unwrap_or(0)),
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
            0 => types.locals.get(&v.of)?.iter().find(|l| l.0 == v.name)?.1,
            k => types.params.get(&v.of)?.get(k as usize - 1)?.1,
        })
    }


    /// The syscall entry's arguments (the number, then the syscall's): the
    /// syscall's name, and its arguments as its parameters have them (with
    /// the constants that name them).
    fn syscall_args(&self, entry: &[u64], mem: &dyn Memory) -> Option<(String, Vec<Object>)> {
        let types = &self.tables.types;
        let name = types.syscalls.get(&(*entry.first()? as u32))?;
        let params = types.syscall_params(name)?;
        let args = params.iter().zip(entry.iter().skip(1).copied()).map(|((pname, ty), v)| {
            let mut o = types.object(&Arg(&v.to_le_bytes(), mem), pname, 0, *ty, 0);
            let names = constant_name(&types.constants, consts_of(name, pname), v as i32 as i64);
            o.value = names.or(o.value);
            o
        }).collect();
        Some((name.clone(), args))
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
                return Object { value: Some("(optimized out)".into()), children: None, expand: None, target: None, ..arg };
            }
            arg
        }).collect()
    }

    /// Function `func`'s locals, each name once, in the order it declares them (the same
    /// rows at every moment, as a debugger's), as the call from record
    /// `first` has them at record `at`: one in memory at its offset from
    /// `frame`, uninitialized if its slot has not been stored to (its
    /// value then is its slot's leftovers, another call's); one in a wasm
    /// local as `set` has it (Record::Var), else not set yet.
    fn locals(&self, func: u32, frame: u32, mem: &dyn Memory, first: usize, at: usize, set: &[Object]) -> Vec<Object> {
        let types = &self.tables.types;
        let not = |o: Object, why: &str| Object { value: Some(why.into()), children: None, expand: None, target: None, ..o };
        // (A name once: two variables of one name, in two of its scopes, are
        // one to the recording, which knows a variable by its name.)
        let mut seen = alloc::collections::BTreeSet::new();
        types.locals.get(&func).into_iter().flatten().filter(|(var, _, _)| seen.insert(var.as_str())).map(|(var, ty, off)| match off {
            Some(off) => {
                let addr = frame.wrapping_add(*off);
                let o = types.object(mem, var, addr, *ty, 0);
                if self.rec.stores().written(addr, types.size_of(*ty).max(1), first, at) { o } else { not(o, "(uninitialized)") }
            }
            None => set.iter().find(|o| &o.name == var).cloned()
                .unwrap_or_else(|| not(types.object(&Arg(&[0; 8], mem), var, 0, *ty, 0), "(uninitialized)")),
        }).collect()
    }

    /// Call `c` and the code inlined into it where it is, innermost first:
    /// each inlined function (Some: its index, see inline_name) at its line
    /// there, then the call (None) at its own line (where the outermost
    /// inlined code is inlined, or the line it is at).
    pub fn nest(&self, c: &stack::StackCall) -> Vec<(Option<u32>, Option<Loc>)> {
        let mut at = c.step.map(|s| s.loc).or(self.tables.functions.get(&c.func).map(|f| f.loc));
        let mut out = Vec::new();
        for &(k, site) in self.chain(c.step.and_then(|s| s.inlined)) {
            out.push((Some(k), at));
            at = Some(site);
        }
        out.push((None, at));
        out
    }

    /// Inline chain `k` (Tables::inlines), or none.
    pub fn chain(&self, k: Option<u32>) -> &[(u32, Loc)] {
        k.and_then(|k| self.tables.inlines.get(k as usize)).map_or(&[], Vec::as_slice)
    }

    /// The function code of inline chain `k` is inlined from (the innermost),
    /// if it is inlined.
    pub(super) fn inlined_from(&self, k: Option<u32>) -> Option<String> {
        self.chain(k).first().map(|&(f, _)| self.inline_name(f))
    }

    /// Inlined function `k`'s name (Tables::names).
    pub fn inline_name(&self, k: u32) -> String {
        self.tables.names.get(k as usize).cloned().unwrap_or_default()
    }
}
