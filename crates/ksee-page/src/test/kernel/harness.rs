use std::collections::BTreeMap;
use std::rc::Rc;

use ksee_core::view::{Frame, Object, SyscallSummary};
use ksee_host::machine::Machine;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use ksee_host::Finished;

use crate::lesson::Step;

pub(super) fn log(s: &str) {
    web_sys::console::log_1(&s.into());
}

/// A promise of `ms` milliseconds passed.
pub(super) async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |ok, _| {
        let set: js_sys::Function = js_sys::Reflect::get(&js_sys::global(), &"setTimeout".into()).unwrap().into();
        let _ = set.call2(&JsValue::NULL, &ok, &ms.into());
    });
    let _ = JsFuture::from(p).await;
}

/// A call a line made, as the kernel left it once quiet: its id, how it
/// finished (none: it blocked) and its result, its syscall summary.
pub(super) struct Made {
    pub(super) id: Option<u32>,
    pub(super) how: Option<String>,
    pub(super) ret: Option<i64>,
    pub(super) syscall: Option<SyscallSummary>,
    pub(super) var: Option<(u32, u32, Option<u32>)>,
}

pub(super) struct KernelHarness {
    pub(super) machine: Rc<Machine>,
    pub(super) failures: Vec<String>,
    /// The console's text so far.
    pub(super) console_output: String,
    /// The calls finished, by id.
    pub(super) finished: BTreeMap<u32, Finished>,
}

impl KernelHarness {
    pub(super) fn check(&mut self, what: impl AsRef<str>, ok: bool) -> bool {
        let what = what.as_ref();
        log(&format!("{}: {what}", if ok { "ok" } else { "FAIL" }));
        if !ok {
            self.failures.push(what.into());
        }
        ok
    }

    pub(super) fn console(&mut self) {
        self.console_output.push_str(&self.machine.console().replace('\r', ""));
    }

    /// The kernel run until it is quiet: the calls finished meanwhile kept.
    pub(super) async fn quiet(&mut self) {
        let finished = self.machine.quiet().await.unwrap_or_default();
        self.machine.snapshot();
        self.console();
        for f in finished {
            self.finished.insert(f.id, f);
        }
    }

    pub(super) fn syscall(&self, id: u32) -> Option<SyscallSummary> {
        let k = self.machine.syscall_of(id)?;
        self.machine.view(|v| v.syscall(k))
    }

    /// Line `text` of the program, made on `task`; a call: the kernel run until quiet.
    pub(super) async fn line_on(&mut self, task: Option<&str>, text: &str) -> Made {
        let made = match self.machine.make(task, text) {
            Ok(m) => m,
            Err(e) => {
                self.check(format!("{text}: {e}"), false);
                return Made { id: None, how: None, ret: None, syscall: None, var: None };
            }
        };
        let id = match made {
            ksee_host::machine::Made::Call(id) => id,
            ksee_host::machine::Made::Decl(v) => return Made { id: None, how: None, ret: None, syscall: None, var: Some((v.ty, v.addr, v.count)) },
        };
        self.quiet().await;
        let f = self.finished.get(&id);
        Made { id: Some(id), how: f.map(|f| f.how.to_string()), ret: f.map(|f| f.ret), syscall: self.syscall(id), var: None }
    }

    pub(super) async fn line(&mut self, text: &str) -> Made {
        self.line_on(None, text).await
    }

    /// Whether syscall `s` ran function `func` (on its task).
    pub(super) fn ran(&self, s: &Option<SyscallSummary>, func: &str) -> bool {
        s.as_ref().is_some_and(|s| s.ret.is_some() && self.machine.first_call(func, s.first, s.last, Some(s.task.addr)).is_some())
    }

    /// The call stack at record `i` (its `nth` line), its values as memory was after record `at`.
    pub(super) fn stack(&self, i: usize, nth: Option<usize>, at: usize) -> Vec<Frame> {
        self.machine.view(|v| v.call_stack(i, nth, 0, at))
    }

    pub(super) fn show(&self, v: (u32, u32, Option<u32>)) -> String {
        self.machine.show(v.0, v.1, v.2)
    }

    /// A lesson's program (or a try) on a fresh kernel: each step's call
    /// id (none: a declaration or assignment).
    pub(super) async fn program(&mut self, steps: &[Step]) -> Vec<Option<u32>> {
        let _ = self.machine.restart().await;
        self.console();
        let mut ids = Vec::new();
        for step in steps {
            ids.push(self.line_on(step.task(), step.text()).await.id);
        }
        ids
    }
}

/// A value's child named `name`, if any.
pub(super) fn child<'a>(o: &'a Object, name: &str) -> Option<&'a Object> {
    o.children.as_ref()?.iter().find(|c| c.name == name)
}

pub(super) fn arg<'a>(f: &'a Frame, name: &str) -> Option<&'a Object> {
    f.args.iter().find(|a| a.name == name)
}

pub(super) fn local<'a>(f: &'a Frame, name: &str) -> Option<&'a Object> {
    f.locals.iter().find(|a| a.name == name)
}

pub(super) fn value(o: Option<&Object>) -> String {
    o.and_then(|o| o.value.clone()).unwrap_or_default()
}
