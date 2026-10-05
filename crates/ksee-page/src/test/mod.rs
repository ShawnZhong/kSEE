//! The page's tests, in the module it ships: `ksee test` opens the page at
//! `#test=NAME` in headless Chrome. The kernel's suite is ksee-host's; the
//! lessons', and the page's DOM interaction suites, are here.

use wasm_bindgen::prelude::*;

mod dom;
mod lessons;
mod ui;

pub(super) type Check = Result<String, String>;

/// The test the URL names (`#...test=NAME`, set by chrome.rs), started: its
/// outcome POSTed to the server's /result, `ok` or `fail` then its log (a
/// panic's too: the page stops there).
pub fn start() -> Option<String> {
    let hash = web_sys::window()?.location().hash().ok()?;
    let name = hash.trim_start_matches('#').split('&').find_map(|kv| kv.strip_prefix("test="))?.to_string();
    ON.set(true);
    std::panic::set_hook(Box::new(|p| {
        web_sys::console::error_1(&p.to_string().into());
        post(format!("fail\npanicked: {p}"));
    }));
    let run = name.clone();
    wasm_bindgen_futures::spawn_local(async move {
        post(match run_test(run).await {
            Ok(report) => format!("ok\n{report}"),
            Err(e) => format!("fail\n{}", e.as_string().unwrap_or_default()),
        });
    });
    Some(name)
}

/// The outcome, to the server (sent as it is called: a panic follows it).
fn post(body: String) {
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_body(&body.into());
    let _ = web_sys::window().unwrap().fetch_with_str_and_init("/result", &init);
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
            "history" => ui::history().await,
            "tasks" => ui::tasks().await,
            "disk" => ui::disk().await,
            "responsive" => ui::responsive().await,
            _ => Err(format!("no test {name}")),
        }
    }.await;
    r.map_err(|e| JsValue::from_str(&e))
}
