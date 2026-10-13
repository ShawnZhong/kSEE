//! The page, in Rust (Dioxus), in the runtime's wasm module: it calls the
//! Machine directly. On the left, a lesson (lesson.rs): its paragraphs,
//! then its program's lines, each with what it did, made one after another
//! on a fresh kernel. Beside it, the recording: the timeline (chart.rs),
//! the source where the cursor is (source.rs), and the tabs: the Stack,
//! the tasks, what the run selected wrote in the kernel's memory, and the console
//! (stack.rs, panels.rs); the header, the URL (nav.rs). The state
//! (state.rs): the lesson and its program, the moment shown (the call
//! selected, the cursor in it, the frame chosen), and the views of them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dioxus::dioxus_core::Task;
use dioxus::prelude::*;
use js_sys::Promise;
use ksee_host::machine::Machine;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

mod chart;
mod editor;
mod layout;
mod lesson;
mod nav;
mod objects;
mod panels;
mod source;
mod stack;
mod state;
mod test;

use lesson::Lesson;
use state::Ui;

thread_local! {
    /// The kernel's files (a promise: `mount`'s), and the Machine (one
    /// thread: the page's), once booted (App's).
    static BOOT: RefCell<Option<Promise>> = const { RefCell::new(None) };
    static MACHINE: RefCell<Option<Rc<Machine>>> = const { RefCell::new(None) };
    /// The lessons (`mount`'s, the page's life long).
    static LESSONS: Cell<&'static [Lesson]> = const { Cell::new(&[]) };
}

/// The Machine, booted from the kernel's files; or why it could not be.
pub(crate) async fn boot() -> Result<Rc<Machine>, String> {
    let promise = BOOT.with_borrow(|p| p.clone()).ok_or("not mounted")?;
    let said = |e: JsValue| e.as_string().or_else(|| js_sys::Error::from(e).message().as_string()).unwrap_or_default();
    let files: js_sys::Array = JsFuture::from(promise).await.map_err(said)?.unchecked_into();
    let file = |k| files.get(k).unchecked_into::<js_sys::Uint8Array>();
    Ok(Rc::new(Machine::start(file(0), file(1), file(2), file(3)).await.map_err(said)?))
}

/// The page's document.
pub(crate) fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

/// The booted Machine (called only once the kernel is up).
pub(crate) fn machine() -> Rc<Machine> {
    MACHINE.with_borrow(|m| m.clone()).expect("booted")
}

pub(crate) fn lessons() -> &'static [Lesson] {
    LESSONS.get()
}

/// Mount the page into the element `root`, with the lessons, `lessons`
/// (lessons.json), and `machine`, the kernel's files arriving (a promise of
/// its image, tables, initramfs and disk: the page boots it; the lesson
/// shows meanwhile).
#[wasm_bindgen]
pub fn mount(root: &str, lessons: &str, machine: Promise) -> Result<(), JsValue> {
    // (A panic said on the console, not only "unreachable".)
    std::panic::set_hook(Box::new(|p| {
        web_sys::console::error_1(&p.to_string().into())
    }));
    let lessons: Vec<Lesson> = serde_json::from_str(lessons).map_err(|e| JsValue::from(format!("lessons.json: {e}")))?;
    BOOT.with_borrow_mut(|b| *b = Some(machine));
    LESSONS.set(Box::leak(lessons.into_boxed_slice()));
    // (The lesson's text as the page came with it, for what reads it without
    // running it: this page draws it now.)
    if let Some(e) = crate::document().and_then(|d| d.get_element_by_id("static")) {
        e.remove();
    }
    // (A test, if the URL names one: those of the Machine only, no page.)
    if test::start().is_some_and(|name| test::MACHINE_ONLY.contains(&name.as_str())) {
        return Ok(());
    }
    dioxus::LaunchBuilder::web()
        .with_cfg(dioxus::web::Config::new().rootname(root))
        .launch(App);
    Ok(())
}

#[component]
fn App() -> Element {
    let ui = use_context_provider(|| {
        let ui = Ui::new();
        nav::from_url(ui);
        ui
    });
    // (The kernel booting: ready, or why not.)
    use_hook(move || {
        spawn(async move {
            let mut kernel = ui.kernel;
            kernel.set(Some(boot().await.map(|machine| MACHINE.with_borrow_mut(|m| *m = Some(machine)))));
        })
    });
    // (The URL, as the lesson and its program are.)
    use_effect(move || nav::to_url((ui.lesson)(), &(ui.program)()));
    // (The program made again, on a fresh kernel, as it or the kernel
    // changes: the run before cancelled.)
    let mut running = use_signal(|| None::<Task>);
    use_effect(move || {
        let program = (ui.program)();
        let up = ui.up();
        ui.begin_program();
        if let Some(task) = running.write().take() {
            task.cancel();
        }
        if up {
            running.set(Some(spawn(lesson::make_program(ui, program))));
        }
    });
    let derived = state::Derived::new(ui);
    use_context_provider(|| derived);
    let sizes = use_context_provider(|| Signal::new(layout::Sizes::kept()));
    use_context_provider(|| Signal::new(None::<layout::Drag>));
    // (The values opened: the Stack's, the Tasks', a source name's tip's.)
    use_context_provider(|| Signal::new(std::collections::BTreeSet::<String>::new()));
    // (The arrow keys, anywhere on the page: one listener on the window.)
    stack::listen(ui);
    rsx! {
        nav::Header {}
        main { class: "page", style: "{sizes().style()}",
            aside { class: "side",
                lesson::LessonPane {}
                if (ui.menu)() { nav::Contents {} }
            }
            layout::Handle { split: layout::Split::Left }
            section { class: "work",
                if ui.cursor.read().is_some() {
                    chart::Timeline {}
                    layout::Handle { split: layout::Split::Chart }
                    div { class: "below",
                        source::Source {}
                        layout::Handle { split: layout::Split::Right }
                        panels::Tabs {}
                    }
                } else {
                    p { class: "dim pad",
                        match (ui.kernel)() {
                            None => "Starting the kernel…".to_string(),
                            Some(Err(e)) => format!("The kernel could not start: {e}"),
                            // (A run under way: the line it is at, the first not done.)
                            Some(Ok(())) => {
                                let (program, outcomes) = ((ui.program)(), ui.outcomes.read().clone());
                                let at = outcomes.iter().position(|o| matches!(o, ksee_host::Outcome::Call { done: false, .. })).unwrap_or(outcomes.len());
                                match program.get(at) {
                                    // (Not after a line that failed: the run stopped there.)
                                    Some(step) if outcomes.len() < program.len() && !outcomes.iter().any(|o| matches!(o, ksee_host::Outcome::Failed(_))) =>
                                        format!("Running line {}: {}…", at + 1, step.text()),
                                    _ => "Pick a call on the left: its recording shows here.".to_string(),
                                }
                            }
                        }
                    }
                }
            }
        }
        layout::Sheet {}
    }
}
