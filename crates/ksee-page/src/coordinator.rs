//! Root-owned effects that coordinate URL state and the one active lesson run.

use dioxus::dioxus_core::Task;
use dioxus::prelude::*;

use crate::lesson;
use crate::state::Ui;

/// Install the application coordinators. Called unconditionally by the root.
pub fn use_coordinator(ui: Ui) {
    // URL reflection is intentionally independent of kernel execution.
    use_effect(move || {
        let lesson = (ui.lesson)();
        let program = (ui.program)();
        lesson::to_url(lesson, &program);
    });

    let mut running = use_signal(|| None::<Task>);
    use_effect(move || {
        // These are the complete reactive inputs to execution.
        let program = (ui.program)();
        let kernel = (ui.kernel)();
        let run = ui.begin_run();

        if let Some(task) = running.write().take() {
            task.cancel();
        }
        if matches!(kernel, Some(Ok(()))) {
            running.set(Some(spawn(lesson::run(ui, run, program))));
        }
    });
}
