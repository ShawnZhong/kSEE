//! The page, in Rust (Dioxus), in the runtime's wasm module: it calls the
//! Machine directly. On the left, a lesson (lesson.rs): its paragraphs,
//! then its program's lines, each with what it did, made one after another
//! on a fresh kernel. Beside it, the recording: the timeline (chart.rs),
//! the source where the cursor is (source.rs), and the tabs: the Stack,
//! the tasks, the objects the call selected changed, the console, the
//! trace (stack.rs, panels.rs). The state is three things (state.rs): the
//! call selected, the cursor in it (a line it ran), and the frame chosen on
//! the Stack.

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use ksee_runtime::machine::Machine;
use wasm_bindgen::prelude::*;

mod chart;
mod editor;
mod layout;
mod lesson;
mod panels;
#[cfg(feature = "selftest")]
mod selftest;
mod source;
mod stack;
mod state;
mod text;
#[cfg(feature = "selftest")]
mod uitest;

pub use lesson::Example;
use state::Ui;

thread_local! {
    /// The Machine (one thread: the page's).
    static MACHINE: RefCell<Option<Rc<Machine>>> = const { RefCell::new(None) };
    static EXAMPLES: RefCell<Vec<Example>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn machine() -> Rc<Machine> {
    MACHINE.with_borrow(|m| m.clone().expect("mounted"))
}

pub(crate) fn examples() -> Vec<Example> {
    EXAMPLES.with_borrow(|e| e.clone())
}

/// Mount the page into the element `root`, with the booted `machine` (the
/// page owns it from here) and the lessons, `examples` (examples.json).
#[wasm_bindgen]
pub fn mount(root: &str, machine: Machine, examples: JsValue) -> Result<(), JsValue> {
    let examples: Vec<Example> = serde_wasm_bindgen::from_value(examples)?;
    MACHINE.with_borrow_mut(|m| *m = Some(Rc::new(machine)));
    EXAMPLES.with_borrow_mut(|e| *e = examples);
    // (The tests, for the driver; the kernel's take the Machine instead: uitest.rs.)
    #[cfg(feature = "selftest")]
    uitest::expose();
    #[cfg(feature = "selftest")]
    if web_sys::window().and_then(|w| w.location().hash().ok()).as_deref() == Some("#selftest") {
        return Ok(());
    }
    dioxus::LaunchBuilder::web().with_cfg(dioxus::web::Config::new().rootname(root)).launch(App);
    Ok(())
}

#[component]
fn App() -> Element {
    let ui = use_context_provider(|| {
        let ui = Ui::new();
        lesson::from_url(ui);
        ui
    });
    let derived = state::Derived::new(ui);
    use_context_provider(|| derived);
    let sizes = use_context_provider(|| Signal::new(layout::Sizes::kept()));
    use_context_provider(|| Signal::new(None::<layout::Drag>));
    rsx! {
        main { class: "page", style: "{sizes().style()}",
            aside { class: "side",
                lesson::Header {}
                article { class: "lesson", lesson::Lesson {} }
            }
            layout::Handle { split: layout::Split::Left }
            section { class: "work", tabindex: "0", onkeydown: move |e| stack::key(ui, &e),
                if ui.call.read().is_some() {
                    chart::Timeline {}
                    layout::Handle { split: layout::Split::Chart }
                    div { class: "below",
                        source::Source {}
                        layout::Handle { split: layout::Split::Right }
                        panels::Tabs {}
                    }
                } else {
                    p { class: "dim pad", "Pick a call on the left: its recording shows here." }
                }
            }
        }
        layout::Sheet {}
    }
}
