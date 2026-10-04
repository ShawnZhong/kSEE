//! The page, in Rust (Dioxus), in the runtime's wasm module: it calls the
//! Machine directly. On the left, a lesson (lesson.rs): its paragraphs,
//! then its program's lines, each with what it did, made one after another
//! on a fresh kernel. Beside it, the recording: the timeline (chart.rs),
//! the source where the cursor is (source.rs), and the tabs: the Stack,
//! the tasks, the objects the call selected changed, and the console
//! (stack.rs, panels.rs). The state is three things (state.rs): the
//! call selected, the cursor in it (a line it ran), and the frame chosen on
//! the Stack.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dioxus::dioxus_core::Task;
use dioxus::prelude::*;
use js_sys::Promise;
use ksee_host::machine::Machine;
use wasm_bindgen::convert::TryFromJsValue;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

mod chart;
mod editor;
mod layout;
mod lesson;
mod objects_view;
mod panels;
mod source;
mod stack;
mod state;
#[cfg(feature = "browser-tests")]
mod test;

use lesson::Example;
use state::Ui;

thread_local! {
    /// The Machine's boot (a promise: `mount`'s), and the Machine (one
    /// thread: the page's), once booted (App's).
    static BOOT: RefCell<Option<Promise>> = const { RefCell::new(None) };
    static MACHINE: RefCell<Option<Rc<Machine>>> = const { RefCell::new(None) };
    /// The lessons (`mount`'s, the page's life long).
    static EXAMPLES: Cell<&'static [Example]> = const { Cell::new(&[]) };
}

/// The Machine, once booted; or why it could not be.
pub(crate) async fn boot() -> Result<Rc<Machine>, String> {
    let promise = BOOT.with_borrow(|p| p.clone()).ok_or("not mounted")?;
    let booted = JsFuture::from(promise).await.map_err(|e| {
        e.as_string()
            .or_else(|| js_sys::Error::from(e).message().as_string())
            .unwrap_or_default()
    })?;
    Ok(Rc::new(
        Machine::try_from_js_value(booted).map_err(|_| "not a Machine".to_string())?,
    ))
}

/// The booted Machine (called only once the kernel is up).
pub(crate) fn machine() -> Rc<Machine> {
    MACHINE.with_borrow(|m| m.clone()).expect("booted")
}

pub(crate) fn examples() -> &'static [Example] {
    EXAMPLES.get()
}

/// Mount the page into the element `root`, with the lessons, `examples`
/// (examples.json), and `machine`, the kernel booting (a promise of the
/// Machine: the page owns it once booted; the lesson shows meanwhile).
#[wasm_bindgen]
pub fn mount(root: &str, examples: JsValue, machine: Promise) -> Result<(), JsValue> {
    // (A panic said on the console, not only "unreachable".)
    std::panic::set_hook(Box::new(|p| {
        web_sys::console::error_1(&p.to_string().into())
    }));
    let examples: Vec<Example> = serde_wasm_bindgen::from_value(examples)?;
    BOOT.with_borrow_mut(|b| *b = Some(machine));
    EXAMPLES.set(Box::leak(examples.into_boxed_slice()));
    // (The tests, for the native Chromium driver; the kernel test takes the Machine directly.)
    #[cfg(feature = "browser-tests")]
    if let Some(name) = test::start() {
        if name == "kernel" {
            return Ok(());
        }
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
        lesson::from_url(ui);
        ui
    });
    // (The kernel booting: ready, or why not.)
    use_hook(move || {
        spawn(async move {
            let mut kernel = ui.kernel;
            kernel.set(Some(match boot().await {
                Ok(machine) => {
                    MACHINE.with_borrow_mut(|m| *m = Some(machine));
                    Ok(())
                }
                Err(error) => Err(error),
            }));
        })
    });
    // (The URL, as the lesson and its program are.)
    use_effect(move || lesson::to_url((ui.lesson)(), &(ui.program)()));
    // (The program made again, on a fresh kernel, as it or the kernel
    // changes: the run before cancelled.)
    let mut running = use_signal(|| None::<Task>);
    use_effect(move || {
        let program = (ui.program)();
        let kernel = (ui.kernel)();
        ui.begin_run();
        if let Some(task) = running.write().take() {
            task.cancel();
        }
        if matches!(kernel, Some(Ok(()))) {
            running.set(Some(spawn(lesson::run(ui, program))));
        }
    });
    let derived = state::Derived::new(ui);
    use_context_provider(|| derived);
    let sizes = use_context_provider(|| Signal::new(layout::Sizes::kept()));
    use_context_provider(|| Signal::new(None::<layout::Drag>));
    // (The arrow keys, anywhere on the page: one listener on the window.)
    stack::listen(ui);
    rsx! {
        lesson::Header {}
        main { class: "page", style: "{sizes().style()}",
            aside { class: "side",
                article { class: "lesson", lesson::Lesson {} }
            }
            layout::Handle { split: layout::Split::Left }
            section { class: "work",
                if ui.at.read().is_some() {
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
                            Some(Ok(())) => "Pick a call on the left: its recording shows here.".to_string(),
                        }
                    }
                }
            }
        }
        layout::Sheet {}
    }
}
