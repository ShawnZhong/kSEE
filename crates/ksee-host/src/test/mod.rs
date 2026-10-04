//! The kernel's tests: the program makes syscalls, recorded, as the page's
//! lessons do, and the checks are of what the recordings show, through the
//! views the page uses. In the module the page ships (`ksee test` opens it
//! at `#test=kernel` in headless Chrome); the page's own suites use the
//! harness.

mod harness;
mod lifecycle;
mod metadata;
mod perf;
mod program;
mod trace;

use std::rc::Rc;

use crate::machine::Machine;

pub use harness::KernelHarness;

/// Run the tests on the booted Machine: its timings, or the checks that
/// failed (each check is logged, "ok: …" or "FAIL: …").
pub async fn kernel(machine: Rc<Machine>) -> Result<String, String> {
    let mut t = KernelHarness::new(machine);
    let m = t.machine.clone();
    let main = m.main().unwrap_or(0);
    t.check(format!("the program ready: a runner ({main}), not /init"), main > 1 && m.runners().contains(&main));
    m.snapshot();
    t.check("nothing recorded at first", m.records() == 0);
    trace::run(&mut t, main).await;
    program::run(&mut t, main).await;
    metadata::run(&mut t).await;
    let views = perf::views(&mut t);
    lifecycle::run(&mut t).await;
    let timings = format!("timings: {}; {views}", perf::boot(&mut t).await);
    t.done(timings)
}
