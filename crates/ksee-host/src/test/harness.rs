use std::rc::Rc;

use ksee_core::view::{Frame, Object, RunSummary};
use crate::machine::Machine;

use crate::Outcome;

pub fn log(s: &str) {
    web_sys::console::log_1(&s.into());
}

pub use crate::js::sleep;

/// Wait until `f` holds, checking every `ms` ms, `n` times at most; then
/// whether it does.
pub async fn until(mut f: impl FnMut() -> bool, ms: i32, n: usize) -> bool {
    for _ in 0..n {
        if f() {
            return true;
        }
        sleep(ms).await;
    }
    f()
}

/// A call a line made, as the kernel left it once quiet: its id, whether it
/// finished (not: it blocked) and its result, its syscall summary.
#[derive(Default)]
pub struct LineResult {
    pub id: Option<u32>,
    pub done: bool,
    pub ret: Option<i64>,
    pub syscall: Option<RunSummary>,
    pub var: Option<(u32, u32, Option<u32>)>,
}

/// The suites' state: the Machine, the checks failed, the console's
/// text (the page's lessons suite uses it too).
pub struct KernelHarness {
    pub machine: Rc<Machine>,
    pub failures: Vec<String>,
    /// The console's text so far.
    pub console_output: String,
}

impl KernelHarness {
    pub fn check(&mut self, what: impl AsRef<str>, ok: bool) -> bool {
        let what = what.as_ref();
        log(&format!("{}: {what}", if ok { "ok" } else { "FAIL" }));
        if !ok {
            self.failures.push(what.into());
        }
        ok
    }

    pub fn console(&mut self) {
        self.console_output.push_str(&self.machine.console().replace('\r', ""));
    }

    /// The kernel run until it is quiet.
    pub async fn quiet(&mut self) {
        let _ = self.machine.quiet().await;
        self.machine.snapshot();
        self.console();
    }

    /// Call `id`'s result, once it finished (none: it did not return).
    pub fn finished(&self, id: u32) -> Option<Option<i64>> {
        self.machine.outcomes().into_iter().find_map(|o| match o {
            Outcome::Call { id: i, done: true, ret, .. } if i == id => Some(ret),
            _ => None,
        })
    }

    pub fn syscall(&self, id: u32) -> Option<RunSummary> {
        let k = self.machine.run_of_call_id(id)?;
        self.machine.view(|v| v.run(k))
    }

    /// Line `text` of the program, made on `task`; a call: the kernel run until quiet.
    pub async fn line_on(&mut self, task: Option<&str>, text: &str) -> LineResult {
        let made = match self.machine.make(task, text) {
            Ok(m) => m,
            Err(e) => {
                self.check(format!("{text}: {e}"), false);
                return LineResult::default();
            }
        };
        let id = match made {
            crate::machine::Made::Call(id) => id,
            crate::machine::Made::Decl(v) => return LineResult { var: Some((v.ty, v.addr, v.count)), ..Default::default() },
        };
        self.quiet().await;
        let f = self.finished(id);
        LineResult { id: Some(id), done: f.is_some(), ret: f.flatten(), syscall: self.syscall(id), ..Default::default() }
    }

    pub async fn line(&mut self, text: &str) -> LineResult {
        self.line_on(None, text).await
    }

    /// Whether syscall `s` ran function `func` (on its task).
    pub fn ran(&self, s: &Option<RunSummary>, func: &str) -> bool {
        s.as_ref().is_some_and(|s| s.ret.is_some() && self.machine.first_call(func, s.first, s.last, Some(s.task.addr)).is_some())
    }

    /// The call stack at record `i` (its `nth` line), its values as memory was after record `at`.
    pub fn stack(&self, i: usize, nth: Option<usize>, at: usize) -> Vec<Frame> {
        self.machine.view(|v| v.call_stack(i, nth, 0, at))
    }

    pub fn show(&self, v: (u32, u32, Option<u32>)) -> String {
        self.machine.show(v.0, v.1, v.2)
    }

    /// A program (its lines, each on its task) on a fresh kernel: each
    /// line's call id (none: a declaration or assignment).
    pub async fn program<'a>(&mut self, steps: impl IntoIterator<Item = (Option<&'a str>, &'a str)>) -> Vec<Option<u32>> {
        let _ = self.machine.restart().await;
        self.console();
        let mut ids = Vec::new();
        for (task, text) in steps {
            ids.push(self.line_on(task, text).await.id);
        }
        ids
    }

    /// A harness on `machine`, booted.
    pub fn new(machine: Rc<Machine>) -> Self {
        KernelHarness { machine, failures: Vec::new(), console_output: String::new() }
    }

    /// The outcome: `report`, or the checks failed (the console logged).
    pub fn done(self, report: String) -> Result<String, String> {
        if self.failures.is_empty() {
            Ok(report)
        } else {
            log(&self.console_output);
            Err(self.failures.join("\n"))
        }
    }
}

/// A value's child named `name`, if any.
pub fn child<'a>(o: &'a Object, name: &str) -> Option<&'a Object> {
    o.children.as_ref()?.iter().find(|c| c.name == name)
}

pub fn arg<'a>(f: &'a Frame, name: &str) -> Option<&'a Object> {
    f.args.iter().find(|a| a.name == name)
}

pub fn local<'a>(f: &'a Frame, name: &str) -> Option<&'a Object> {
    f.locals.iter().find(|a| a.name == name)
}

pub fn value(o: Option<&Object>) -> String {
    o.and_then(|o| o.value.clone()).unwrap_or_default()
}
