//! A program as the host runs it, a line at a time: its runners (the
//! tasks that make its calls, user/prog.c), their memory, the calls asked
//! of them and how each line went. The host does what the kernel needs
//! (memory, interrupts); the rules are here.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::eval::Sym;
use super::names::Names;

/// A runner: its task (its task_struct), its request (where a call is put:
/// its first word, the number, is what the runner waits on), its program's
/// heap (its start), and the result-bearing call that created it (none for
/// the main runner).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Runner {
    pub task: u32,
    pub req: u32,
    pub heap: u32,
    pub source: Option<u32>,
}

/// A syscall a runner makes: asked (written into its request), begun
/// (recorded from `from`, the id of the record there), then done.
#[derive(Default, Clone, Debug)]
pub struct Call {
    pub id: u32,
    pub tid: u32,
    /// Its runner's task (its task_struct): as the recording names it.
    pub task: u32,
    pub nr: u32,
    pub from: u32,
    pub begun: bool,
    pub done: bool,
    /// The program's line that made it, and its result's name and what its
    /// type points to, if declared (`int fd = openat(…)`).
    pub line: usize,
    pub result: Option<(String, Option<u32>)>,
    /// The syscall's name, and the variable each argument names (if it is one).
    pub said: (String, Vec<Option<String>>),
}

/// How a line of the program went: a variable declared (as it starts), or
/// a call (its id): finished (its result) or not yet (it blocks); or why it
/// could not be made.
#[derive(Clone, PartialEq, Debug)]
pub enum Outcome {
    Declared(String),
    /// (`during`: the line during which it finished, if a later one's;
    /// `changed`: the variables it changed, as they are then.)
    Call { id: u32, done: bool, ret: Option<i64>, during: Option<usize>, changed: Vec<String> },
    Failed(String),
}

/// A variable of the program's: memory of type `ty` (an array of `count`)
/// at `addr`, `bytes` long.
#[derive(Clone, Debug)]
pub struct Var {
    pub name: String,
    pub addr: u32,
    pub ty: u32,
    pub count: Option<u32>,
    pub bytes: u32,
}

#[derive(Default)]
pub struct Session {
    /// The runners ready for calls, by thread id; the program's own (its
    /// main thread: the calls' by default), once it is ready.
    pub runners: BTreeMap<u32, Runner>,
    pub main: Option<u32>,
    /// The programs' own memory (a process's, its threads' and vfork
    /// children's too, the same memory): what of each is handed out (its
    /// start -> (next free, end)).
    pub heaps: BTreeMap<u32, (u32, u32)>,
    /// The calls asked, by id; and the last id (ids are not used again, a
    /// boot after another's: see again).
    pub calls: BTreeMap<u32, Call>,
    pub asked: u32,
    /// The program's names (a variable's, a result's), how many of its
    /// lines have been made, how each went, and the variables declared,
    /// each as it was shown last.
    pub names: Names,
    pub lines: usize,
    pub outcomes: Vec<Outcome>,
    pub vars: Vec<(Var, String)>,
}

impl Session {
    /// A new machine's: nothing but the ids already used.
    pub fn again(&self) -> Session {
        Session { asked: self.asked, ..Default::default() }
    }

    /// The call runner `tid` is making (asked, not done), if any: its id.
    pub fn in_flight(&self, tid: u32) -> Option<u32> {
        self.calls.values().find(|c| c.tid == tid && !c.done).map(|c| c.id)
    }

    /// Whether every call asked is done.
    pub fn all_done(&self) -> bool {
        self.calls.values().all(|c| c.done)
    }

    /// Whether a call is under way, begun and not done: what is recorded.
    pub fn recording(&self) -> bool {
        self.calls.values().any(|c| c.begun && !c.done)
    }

    /// The tasks of the calls not done.
    pub fn waiting_tasks(&self) -> Vec<u32> {
        self.calls.values().filter(|c| !c.done).map(|c| c.task).collect()
    }

    /// Task `task`'s runner (task_struct), if it is one: its tid.
    pub fn runner_of(&self, task: u32) -> Option<u32> {
        self.runners.iter().find(|(_, r)| r.task == task).map(|(&t, _)| t)
    }

    /// Runner `tid` (task `task`) is ready, waiting on its request at `req`,
    /// its program's memory `size` bytes at `heap`; the program's own, if
    /// `main`. One not the program's own is the latest result-bearing
    /// call's (a clone's child): that result's name stands for it already,
    /// before the clone returns. Whether the boot is over (the program
    /// ready, no call under way).
    pub fn ready(&mut self, tid: u32, task: u32, req: u32, heap: u32, size: u32, main: bool) -> bool {
        if main {
            self.main = Some(tid);
        }
        let source = if main { None } else { self.calls.values().next_back().and_then(|c| c.result.as_ref().map(|_| c.id)) };
        self.runners.insert(tid, Runner { task, req, heap, source });
        self.heaps.entry(heap).or_insert((heap, heap + size));
        if let Some(Call { line, result: Some((name, _)), .. }) = source.and_then(|id| self.calls.get(&id))
            && !self.names.has(name, *line)
        {
            self.names.set(name, *line, Sym::Value { v: tid as i64, to: None });
        }
        main && self.all_done()
    }

    /// Runner `tid` is one no more (its task exited, or it ran another
    /// program): its room and heap forgotten (a heap no runner has left,
    /// its process's gone: another's may come to the same address).
    pub fn forget(&mut self, tid: u32) {
        if let Some(r) = self.runners.remove(&tid) && !self.runners.values().any(|o| o.heap == r.heap) {
            self.heaps.remove(&r.heap);
        }
        if self.main == Some(tid) {
            self.main = None; // (the program exited: /init starts it again)
        }
    }

    /// The runner for row `line`'s task: the program's own (`None`), or the
    /// one the latest earlier result named `task` created.
    pub fn task_tid(&self, task: Option<&str>, line: usize) -> Result<u32, String> {
        match task {
            None => self.main.filter(|tid| self.runners.contains_key(tid)).ok_or_else(|| "the program is not ready".into()),
            Some(name) => self.named_runners(line).into_iter().filter(|&(_, n, _)| n == name).max_by_key(|&(id, _, _)| id)
                .map(|(_, _, tid)| tid).ok_or_else(|| format!("no task {name}")),
        }
    }

    /// The runners a result-bearing call before line `line` created: (that
    /// call's id, its result's name, the runner's tid). (Ids grow line by line.)
    fn named_runners(&self, line: usize) -> Vec<(u32, &str, u32)> {
        self.runners.iter().filter_map(|(&tid, runner)| {
            let call = self.calls.get(&runner.source?)?;
            let (name, _) = call.result.as_ref()?;
            (call.line < line).then_some((call.id, name.as_str(), tid))
        }).collect()
    }

    /// The task names row `line` sees (the program's own is `None`, not
    /// one), in the order made.
    pub fn task_columns(&self, line: usize) -> Vec<String> {
        let mut columns = self.named_runners(line);
        columns.sort_by_key(|&(id, _, _)| id);
        let mut names: Vec<String> = Vec::new();
        for (_, name, _) in columns {
            if !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
        names
    }

    /// `len` bytes of runner `tid`'s program memory, aligned to 16 bytes.
    pub fn alloc(&mut self, tid: u32, len: u32) -> Result<u32, String> {
        let base = self.runners.get(&tid).ok_or_else(|| format!("no runner {tid}"))?.heap;
        let (next, end) = self.heaps.get_mut(&base).ok_or("no memory")?;
        let at = (*next + 15) & !15;
        if at + len > *end {
            return Err("the program's memory is full".into());
        }
        *next = at + len;
        Ok(at)
    }

    /// The next call's id, and the runner `tid` that would make it, if it
    /// is free.
    pub fn next_call(&self, tid: u32) -> Result<(u32, Runner), String> {
        let runner = self.runners.get(&tid).copied().ok_or_else(|| format!("no runner {tid}"))?;
        if self.in_flight(tid).is_some() {
            return Err(format!("{tid} is still making its call"));
        }
        Ok((self.asked + 1, runner))
    }

    /// Call `c` asked (its id the next: see next_call).
    pub fn ask(&mut self, c: Call) {
        self.asked = c.id;
        self.calls.insert(c.id, c);
    }

    /// Runner `tid`'s call begins, at record `from`: whether it had not.
    pub fn begin(&mut self, tid: u32, from: u32) -> bool {
        let Some(c) = self.in_flight(tid).and_then(|id| self.calls.get_mut(&id)) else { return false };
        if c.begun {
            return false;
        }
        (c.begun, c.from) = (true, from);
        true
    }

    /// Whether runner `tid`'s call has begun.
    pub fn begun(&self, tid: u32) -> bool {
        self.in_flight(tid).and_then(|id| self.calls.get(&id)).is_some_and(|c| c.begun)
    }

    /// Runner `tid`'s call is over: returned (`ret`), or not (an execve ran
    /// a new program, its task exited). Its result's name stands for the
    /// result now; its line's outcome is done (during the last line, if a
    /// later one). Whether it had a call.
    pub fn finish(&mut self, tid: u32, ret: Option<i64>) -> bool {
        let Some(id) = self.in_flight(tid) else { return false };
        let c = self.calls.get_mut(&id).unwrap();
        c.done = true;
        if let (Some((name, to)), Some(v)) = (&c.result, ret) {
            self.names.set(name, c.line, Sym::Value { v, to: *to });
        }
        let last = self.outcomes.len().saturating_sub(1);
        for (j, o) in self.outcomes.iter_mut().enumerate() {
            if let Outcome::Call { id: i, done, ret: r, during, .. } = o && *i == id {
                (*done, *r, *during) = (true, ret, (j != last).then_some(last));
            }
        }
        true
    }
}

impl Session {
    /// The variables the last line's call changed, by now, in its outcome:
    /// each as `show` shows it now, if not as it was shown last; one passed
    /// to the call as an argument, as `records` reads its first `len`
    /// bytes (the result's, if positive), if it is records of a variable
    /// length (a directory's entries).
    pub fn changed(&mut self, show: impl Fn(&Var) -> String, records: impl Fn(&str, usize, &Var, u32) -> Option<String>) {
        let Some(Outcome::Call { id, ret, changed, .. }) = self.outcomes.last_mut() else { return };
        let Some(Call { said: (call, names), .. }) = self.calls.get(id) else { return };
        changed.clear();
        for (v, last) in self.vars.iter_mut() {
            let now = show(v);
            if now != *last {
                let read = names.iter().position(|n| n.as_deref() == Some(v.name.as_str())).zip(ret.filter(|r| *r > 0))
                    .and_then(|(k, r)| records(call, k, v, (r as u32).min(v.bytes)));
                changed.push(format!("{} = {}", v.name, read.unwrap_or_else(|| now.clone())));
                *last = now;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session with the program's runner (tid 10) ready.
    fn ready() -> Session {
        let mut s = Session::default();
        assert!(s.ready(10, 0x100, 0x200, 0x1000, 0x100, true));
        s
    }

    fn ask(s: &mut Session, tid: u32, line: usize, result: Option<&str>) -> u32 {
        let (id, r) = s.next_call(tid).unwrap();
        s.ask(Call { id, tid, task: r.task, line, result: result.map(|n| (n.into(), None)), ..Default::default() });
        s.outcomes.push(Outcome::Call { id, done: false, ret: None, during: None, changed: Vec::new() });
        s.lines = line + 1;
        id
    }

    #[test]
    fn what_a_call_changed() {
        let mut s = ready();
        let var = |name: &str, addr| (Var { name: name.into(), addr, ty: 1, count: None, bytes: 64 }, "0".to_string());
        s.vars = alloc::vec![var("buf", 0x1000), var("n", 0x1040), var("same", 0x1080)];
        let id = ask(&mut s, 10, 2, None);
        s.calls.get_mut(&id).unwrap().said = ("getdents64".into(), alloc::vec![None, Some("buf".into()), None]);
        s.finish(10, Some(48));
        // (buf, passed: read as records, its first 48 bytes; n, changed; same, not.)
        s.changed(|v| if v.name == "same" { "0".into() } else { format!("{:#x}", v.addr) },
                  |call, k, v, len| (call == "getdents64" && k == 1).then(|| format!("{len} bytes of {}", v.name)));
        let Outcome::Call { changed, .. } = &s.outcomes[0] else { panic!() };
        assert_eq!(changed, &["buf = 48 bytes of buf", "n = 0x1040"]);
        assert_eq!(s.vars[1].1, "0x1040", "shown last: now");
    }

    #[test]
    fn memory_is_handed_out_aligned_until_full() {
        let mut s = ready();
        assert_eq!(s.alloc(10, 3), Ok(0x1000));
        assert_eq!(s.alloc(10, 8), Ok(0x1010));
        assert!(s.alloc(10, 0x100).is_err());
        assert!(s.alloc(11, 1).is_err());
    }

    #[test]
    fn a_call_finished_during_a_later_line() {
        let mut s = ready();
        let a = ask(&mut s, 10, 0, None);
        assert!(s.next_call(10).is_err(), "one call at a time");
        assert!(s.begin(10, 7) && !s.begin(10, 8) && s.recording());
        // (Another runner's call, the next line's, wakes it.)
        s.ready(20, 0x300, 0x400, 0x2000, 0x100, false);
        ask(&mut s, 20, 1, None);
        assert!(s.finish(10, Some(3)) && !s.finish(10, Some(3)));
        assert_eq!(s.outcomes[0], Outcome::Call { id: a, done: true, ret: Some(3), during: Some(1), changed: Vec::new() });
        assert_eq!(s.calls[&a].from, 7);
    }

    #[test]
    fn a_clone_child_is_its_result_name_before_the_clone_returns() {
        let mut s = ready();
        let clone = ask(&mut s, 10, 0, Some("child"));
        assert!(!s.ready(20, 0x300, 0x400, 0x1000, 0, false), "a call under way");
        assert_eq!(s.runners[&20].source, Some(clone));
        assert_eq!(s.names.get("child", 1), Some(&Sym::Value { v: 20, to: None }));
        assert_eq!(s.task_tid(Some("child"), 1), Ok(20));
        assert!(s.task_tid(Some("child"), 0).is_err(), "not before its line");
        assert_eq!(s.task_columns(1), ["child"]);
        // (Its heap is the parent's (vfork): kept while one has it.)
        s.forget(20);
        assert!(s.heaps.contains_key(&0x1000));
        s.forget(10);
        assert!(s.heaps.is_empty() && s.main.is_none());
    }
}
