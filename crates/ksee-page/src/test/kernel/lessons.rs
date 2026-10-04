use ksee_core::program::{statement, Line as Stmt};
use ksee_core::text::{self, LessonTarget, Piece};

use crate::lesson::Step;

use super::harness::KernelHarness;

pub(super) async fn run(t: &mut KernelHarness, examples: &[crate::lesson::Example]) {
    let m = t.machine.clone();
    // The lessons: each on a fresh kernel, its calls finished, its links found; its tries, each made.
    for ex in examples {
        let ids = t.program(&ex.steps).await;
        let is_call = |step: &Step| matches!(statement(step.text()), Ok(Stmt::Call { .. }));
        let blocked: Vec<String> = ex.steps.iter().zip(&ids).filter(|(step, id)| is_call(step) && !id.is_some_and(|id| t.finished.contains_key(&id)))
            .map(|(step, _)| step.text().into()).collect();
        let calls: Vec<Option<u32>> = ex.steps.iter().zip(&ids).filter(|(step, _)| is_call(step)).map(|(_, id)| *id).collect();
        let mut missing = Vec::new();
        for p in &ex.text {
            for x in text::pieces(p) {
                let Piece::Link { target, to, .. } = x else { continue };
                let found = match to {
                    Some(LessonTarget::Try { index }) => ex.tries.get(index - 1).is_some(),
                    Some(LessonTarget::Recording { call, function, any_task, .. }) => {
                        let s = calls.get(call - 1).copied().flatten().and_then(|id| t.syscall(id));
                        s.as_ref().is_some_and(|s| function.as_ref().is_none_or(|f|
                            m.first_call(f, s.first, s.last, (!any_task).then_some(s.task.addr)).is_some()
                        ))
                    }
                    None => false,
                };
                if !found {
                    missing.push(target);
                }
            }
        }
        for (k, try_) in ex.tries.iter().enumerate() {
            let ran = t.program(try_).await;
            t.check(format!("example {}, try {}: {} lines made", ex.name, k + 1, try_.len()), ran.len() == try_.len() && !try_.is_empty());
        }
        t.check(format!("example {}: {} lines, all finished, the explanation's links found (missing {missing:?}, blocked {blocked:?})", ex.name, ex.steps.len()),
                missing.is_empty() && blocked.is_empty());
    }
}
