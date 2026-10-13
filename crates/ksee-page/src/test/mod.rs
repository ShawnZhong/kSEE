//! The page's tests, in the module it ships: `ksee test` opens the page at
//! `#test=NAME` in headless Chrome. The kernel's suite is ksee-host's; the
//! lessons', and the page's DOM interaction suites, are here.

use wasm_bindgen::prelude::*;

mod dom;
mod lessons;
mod ui;

pub(super) type Check = Result<String, String>;

/// The test the URL names (`#...test=NAME`, set by chrome.rs), started: its
/// outcome said on the console (see report; a panic's too: the page stops
/// there).
pub fn start() -> Option<String> {
    let name = crate::nav::hash("test")?;
    ON.set(true);
    std::panic::set_hook(Box::new(|p| {
        web_sys::console::error_1(&p.to_string().into());
        report(false, &format!("panicked: {p}"));
    }));
    let run = name.clone();
    wasm_bindgen_futures::spawn_local(async move {
        match run_test(run).await {
            Ok(log) => report(true, &log),
            Err(e) => report(false, &e.as_string().unwrap_or_default()),
        }
    });
    Some(name)
}

/// The outcome, on the console, one line (chrome.rs reads it on the
/// shell's stderr): `ksee test: ok` or `fail`, then its log as a JSON string.
fn report(ok: bool, log: &str) {
    let log = js_sys::JSON::stringify(&log.into()).ok().and_then(|s| s.as_string()).unwrap_or_default();
    web_sys::console::log_1(&format!("ksee test: {} {log}", if ok { "ok" } else { "fail" }).into());
}

thread_local! {
    /// Whether a test runs: the timeline then says where it drew its bars.
    static ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn on() -> bool {
    ON.get()
}

/// The tests that need no page: the Machine only.
pub const MACHINE_ONLY: &[&str] = &["kernel", "lessons"];

/// Test `name` (see chrome.rs; the page each needs is opened there): its
/// report, or an error saying what failed.
async fn run_test(name: String) -> Result<String, JsValue> {
    let r: Check = async {
        match name.as_str() {
            "kernel" => ksee_host::test::kernel(crate::boot().await?).await,
            "lessons" => lessons::run(crate::boot().await?).await,
            "read" => ui::read().await,
            "url" => ui::url().await,
            "tasks" => ui::tasks().await,
            "disk" => ui::disk().await,
            "responsive" => ui::responsive().await,
            "tour" => ui::tour().await,
            _ => Err(format!("no test {name}")),
        }
    }.await;
    r.map_err(|e| JsValue::from_str(&e))
}
