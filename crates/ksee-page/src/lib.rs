//! The page, in Rust (Dioxus), in the runtime's wasm module: it calls the
//! Machine directly. On the left, a lesson (lesson.rs): its paragraphs,
//! then its program's lines, each with what it did, made one after another
//! on a fresh kernel. Beside it, the recording: the timeline (chart.rs),
//! the source where the cursor is (source.rs), and the tabs: the Stack,
//! the tasks, the objects the call selected changed, and the console
//! (stack.rs, panels.rs). The state is three things (state.rs): the
//! call selected, the cursor in it (a line it ran), and the frame chosen on
//! the Stack.

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use dioxus::prelude::*;
#[cfg(target_arch = "wasm32")]
use js_sys::Promise;
#[cfg(target_arch = "wasm32")]
use ksee_host::machine::Machine;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::convert::TryFromJsValue;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;

#[cfg(target_arch = "wasm32")]
mod chart;
#[cfg(target_arch = "wasm32")]
mod coordinator;
#[cfg(target_arch = "wasm32")]
mod editor;
#[cfg(target_arch = "wasm32")]
mod host;
#[cfg(target_arch = "wasm32")]
mod layout;
#[cfg(target_arch = "wasm32")]
mod lesson;
mod lesson_view;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod native_tests;
#[cfg(any(target_arch = "wasm32", test))]
mod objects_view;
#[cfg(target_arch = "wasm32")]
mod panels;
#[cfg(target_arch = "wasm32")]
mod source;
#[cfg(target_arch = "wasm32")]
mod stack;
#[cfg(target_arch = "wasm32")]
mod state;
#[cfg(all(target_arch = "wasm32", feature = "browser-tests"))]
mod test;

/// Clamp a horizontal timeline drag to the recording and return its committed
/// start/span plus the pixel offset that can actually be previewed.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn clamped_pan(
    window: (usize, usize),
    dx: f64,
    width: f64,
    steps: usize,
) -> (f64, usize, f64) {
    let (a, b) = window;
    let span = b.saturating_sub(a);
    let visible = span.saturating_add(1).min(steps.max(1));
    let max_start = steps.saturating_sub(visible) as f64;
    let requested = a as f64 - dx / width.max(1.0) * visible as f64;
    let start = requested.clamp(0.0, max_start);
    let offset = (a as f64 - start) / visible as f64 * width.max(1.0);
    (start, span, offset)
}

#[cfg(target_arch = "wasm32")]
pub(crate) use host::machine;
#[cfg(target_arch = "wasm32")]
use lesson::Example;
#[cfg(target_arch = "wasm32")]
use state::Ui;

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// The Machine (one thread: the page's), once booted; until then, its
    /// boot (a promise: `mount`'s).
    static BOOT: RefCell<Option<Promise>> = const { RefCell::new(None) };
    static EXAMPLES: RefCell<Vec<Example>> = const { RefCell::new(Vec::new()) };
}

/// The Machine, once booted; or why it could not be.
#[cfg(target_arch = "wasm32")]
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

#[cfg(target_arch = "wasm32")]
pub(crate) fn examples() -> Vec<Example> {
    EXAMPLES.with_borrow(|e| e.clone())
}

/// Mount the page into the element `root`, with the lessons, `examples`
/// (examples.json), and `machine`, the kernel booting (a promise of the
/// Machine: the page owns it once booted; the lesson shows meanwhile).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn mount(root: &str, examples: JsValue, machine: Promise) -> Result<(), JsValue> {
    // (A panic said on the console, not only "unreachable".)
    std::panic::set_hook(Box::new(|p| {
        web_sys::console::error_1(&p.to_string().into())
    }));
    let examples: Vec<Example> = serde_wasm_bindgen::from_value(examples)?;
    BOOT.with_borrow_mut(|b| *b = Some(machine));
    EXAMPLES.with_borrow_mut(|e| *e = examples);
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

#[cfg(target_arch = "wasm32")]
#[component]
fn App() -> Element {
    let ui = use_context_provider(|| {
        let ui = Ui::new();
        lesson::from_url(ui);
        ui
    });
    let host = use_context_provider(host::Host::new);
    // (The kernel booting: ready, or why not.)
    use_hook(move || {
        spawn(async move {
            let mut kernel = ui.kernel;
            kernel.set(Some(match boot().await {
                Ok(machine) => {
                    host.install(machine);
                    Ok(())
                }
                Err(error) => Err(error),
            }));
        })
    });
    coordinator::use_coordinator(ui);
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
