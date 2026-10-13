//! The lessons, each on a fresh kernel: its calls finished, its links
//! found; its tries, each made.

use std::rc::Rc;

use ksee_core::program::{statement, Statement};
use ksee_core::text::{self, LessonTarget, Piece};
use ksee_host::machine::Machine;
use ksee_host::test::KernelHarness;

use crate::lesson::ProgramLine;

/// Each step as the harness makes it: its task and its text.
fn lines(steps: &[ProgramLine]) -> impl Iterator<Item = (Option<&str>, &str)> {
    steps.iter().map(|s| (s.task(), s.text()))
}

pub(super) async fn run(machine: Rc<Machine>) -> super::Check {
    let mut t = KernelHarness::new(machine);
    let m = t.machine.clone();
    let lessons = crate::lessons();
    // (The module's memory after each lesson, MiB: it never shrinks, so a
    // lesson's leftovers would add up.)
    let mut mib = Vec::new();
    for lesson in lessons {
        let ids = t.program(lines(&lesson.program)).await;
        let is_call = |step: &ProgramLine| matches!(statement(step.text()), Ok(Statement::Call { .. }));
        let blocked: Vec<String> = lesson.program.iter().zip(&ids).filter(|(step, id)| is_call(step) && !id.is_some_and(|id| t.finished(id).is_some()))
            .map(|(step, _)| step.text().into()).collect();
        let mut missing = Vec::new();
        for p in &lesson.text {
            for x in text::pieces(p) {
                let Piece::Link { label, to } = x else { continue };
                let found = match to {
                    Some(LessonTarget::Try { index }) => lesson.tries.get(index - 1).is_some(),
                    Some(LessonTarget::Tab(_)) => true,
                    Some(LessonTarget::Recording { call, function, .. }) => m.find(call.as_ref().map(|(c, n)| (c.as_str(), *n)), &function).is_some(),
                    None => false,
                };
                if !found {
                    missing.push(label);
                }
            }
        }
        for (k, try_) in lesson.tries.iter().enumerate() {
            let ran = t.program(lines(try_)).await;
            t.check(format!("lesson {}, try {}: {} lines made", lesson.name, k + 1, try_.len()), ran.len() == try_.len() && !try_.is_empty());
        }
        t.check(format!("lesson {}: {} lines, all finished, the explanation's links found (missing {missing:?}, blocked {blocked:?})", lesson.name, lesson.program.len()),
                missing.is_empty() && blocked.is_empty());
        mib.push(core::arch::wasm32::memory_size(0) * 64 / 1024);
    }
    t.done(format!("{} lessons; MiB of memory after each: {mib:?}", lessons.len()))
}
