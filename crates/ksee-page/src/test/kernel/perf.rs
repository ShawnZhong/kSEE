//! How long what the page asks of a recording takes, on the last one made:
//! said, not checked (the suite's report: so a change that slows one shows).

use super::harness::KernelHarness;

/// Milliseconds `f` takes, the best of `n` runs.
fn ms(n: usize, mut f: impl FnMut()) -> f64 {
    let now = || web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now());
    (0..n).map(|_| {
        let t = now();
        f();
        now() - t
    }).fold(f64::INFINITY, f64::min)
}

/// A boot, and a call on the fresh kernel.
pub(super) async fn boot(t: &mut KernelHarness) -> String {
    let m = t.machine.clone();
    let now = || web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now());
    let t0 = now();
    let _ = m.restart().await;
    let boot = now() - t0;
    let t0 = now();
    t.line("getpid()").await;
    format!("a boot {boot:.0} ms, a call {:.1} ms", now() - t0)
}

/// The views of the last recording, its largest syscall's.
pub(super) fn views(t: &mut KernelHarness) -> String {
    let m = t.machine.clone();
    let snapshot = ms(3, || m.snapshot());
    let (records, steps, calls) = m.view(|v| (v.rec.len(), v.rec.steps.len(), v.rec.syscalls.len()));
    // (The syscall with the most records: the page's heaviest view.)
    let Some((k, s)) = m.view(|v| (0..calls).filter_map(|k| v.syscall(k).map(|s| (k, s))).max_by_key(|(_, s)| s.last - s.first)) else { return String::new() };
    let mid = (s.first + s.last) / 2;
    let file = m.view(|v| v.call_stack(mid, None, 0, mid).first().and_then(|f| f.file.clone())).unwrap_or_default();
    let chart = ms(3, || { m.view(|v| v.chart(0, steps.saturating_sub(1), 1200)); });
    let syscalls = ms(3, || { m.view(|v| v.syscalls()); });
    let lines = ms(3, || { m.view(|v| v.lines(s.first, s.last, s.task.addr)); });
    let stack = ms(3, || { m.view(|v| v.call_stack(mid, None, 0, mid)); });
    let objects = ms(3, || { m.view(|v| v.objects(s.first, s.last)); });
    // (The first time, a span's stores named: what the page does as a call
    // is first shown.)
    m.snapshot();
    let named = ms(1, || { m.view(|v| v.notes(s.first, s.last, &file, mid)); });
    let notes = ms(3, || { m.view(|v| v.notes(s.first, s.last, &file, mid)); });
    let coverage = ms(3, || { m.view(|v| v.coverage(&file, Some(k))); });
    // (A deep stack of structs: an mmap's, where it preallocates its maple
    // tree nodes; the first step on it, the stack then.)
    let deep = m.first_call("vma_iter_prealloc", 0, records.saturating_sub(1), None).map(|i| {
        let n = m.view(|v| v.call_stack(i, None, 0, i).len());
        (n, ms(3, || { m.view(|v| v.call_stack(i, None, 0, i)); }))
    });
    format!("{records} records, {steps} steps, {calls} syscalls; the largest, {} records: snapshot {snapshot:.1} ms, \
             chart {chart:.1}, syscalls {syscalls:.1}, its lines {lines:.1}, a stack {stack:.1}, its objects {objects:.1}, \
             its stores named {named:.1}, a file's notes {notes:.1} and coverage {coverage:.1} ({file}); at vma_iter_prealloc, a stack {deep:?}", s.last - s.first + 1)
}
