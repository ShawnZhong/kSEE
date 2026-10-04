//! Browser tests compiled into the page with feature `browser-tests`: the kernel's
//! integration checks and the page's DOM interaction suites.

use wasm_bindgen::prelude::*;

mod dom;
mod kernel;
mod ui;

pub(super) type Check = Result<String, String>;

/// The test the URL names (`#...test=NAME`, set by chrome.rs), started: its
/// outcome POSTed to the server's /result, `ok` or `fail` then its log (a
/// panic's too: the page stops there).
pub fn start() -> Option<String> {
    let hash = web_sys::window()?.location().hash().ok()?;
    let name = hash.trim_start_matches('#').split('&').find_map(|kv| kv.strip_prefix("test="))?.to_string();
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

/// Test `name` (kernel, lesson, history, tasks, responsive; the page each needs is
/// opened by chrome.rs): its report, or an error saying what failed.
async fn run_test(name: String) -> Result<String, JsValue> {
    let r = match name.as_str() {
        "kernel" => run_kernel().await,
        "lesson" => ui::lesson().await,
        "history" => ui::history().await,
        "tasks" => ui::tasks().await,
        "responsive" => ui::responsive().await,
        _ => Err(format!("no test {name}")),
    };
    r.map_err(|e| JsValue::from_str(&e))
}

async fn run_kernel() -> Check {
    let machine = crate::boot().await?;
    kernel::run(machine).await
}
