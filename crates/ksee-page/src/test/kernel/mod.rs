//! The kernel's tests, in the page's module (feature `browser-tests`: `ksee
//! test` builds it and runs them in Chromium): the program makes syscalls, recorded, as
//! the page's lessons do, and the checks are of what the recordings show,
//! through the views the page uses; then every lesson and try, on a fresh
//! kernel each, its calls finished and its links found.


mod harness;
mod lessons;
mod lifecycle;
mod metadata;
mod perf;
mod program;
mod trace;

use std::collections::BTreeMap;
use std::rc::Rc;

use ksee_host::machine::Machine;

use harness::{log, KernelHarness};

/// Run the tests on the booted Machine, with the lessons (both as `mount`
/// was given them: the page not mounted, see lib.rs): whether all passed
/// (each check is logged, "ok: …" or "FAIL: …").
pub(super) async fn run(machine: Rc<Machine>) -> Result<String, String> {
    let examples = crate::examples();
    let mut t = KernelHarness { machine, failures: Vec::new(), console_output: String::new(), finished: BTreeMap::new() };
    let m = t.machine.clone();
    let main = m.main().unwrap_or(0);
    t.check("timeline colors", crate::chart::call_color(Some("fs/x.c"), false) == "hsl(210 55% 74%)"
            && crate::chart::call_color(Some("fs/x.c"), true) == "hsl(210 65% 50%)"
            && crate::chart::call_color(Some("arch/x.c"), false) == "hsl(0 0% 79%)"
            && crate::chart::call_color(None, false) == "hsl(0 0% 79%)");
    let horizontal = crate::chart::wheel_view((100, 199), 500.0, 1000.0, 80.0, 4.0, false);
    let vertical = crate::chart::wheel_view((100, 199), 500.0, 1000.0, 4.0, 80.0, false);
    t.check("the timeline wheel follows one axis", (horizontal.0 - 108.0).abs() < f64::EPSILON && horizontal.1 == 99
            && vertical.0 < 100.0 && vertical.1 > 99);
    t.check(format!("the program ready: a runner ({main}), not /init"), main > 1 && m.runners().contains(&main));
    m.snapshot();
    t.check("nothing recorded at first", m.records() == 0);
    trace::run(&mut t, main).await;
    program::run(&mut t, main).await;
    metadata::run(&mut t).await;
    let views = perf::views(&mut t);
    lessons::run(&mut t, &examples).await;
    lifecycle::run(&mut t, &examples).await;
    let timings = format!("timings: {}; {views}", perf::boot(&mut t).await);
    if t.failures.is_empty() {
        Ok(timings)
    } else {
        log(&t.console_output);
        Err(t.failures.join("\n"))
    }
}
