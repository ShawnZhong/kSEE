//! How long what the page asks of a recording takes, on the last one made:
//! said, not checked (the suite's report: so a change that slows one shows).

use super::harness::KernelHarness;

/// The page's clock (ms).
fn now() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map_or(0.0, |p| p.now())
}

/// Milliseconds `f` takes, the best of `n` runs.
fn ms(n: usize, mut f: impl FnMut()) -> f64 {
    (0..n).map(|_| {
        let t = now();
        f();
        now() - t
    }).fold(f64::INFINITY, f64::min)
}

/// A boot, and a call on the fresh kernel.
pub(super) async fn boot(t: &mut KernelHarness) -> String {
    let m = t.machine.clone();
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
    let (records, steps, calls) = m.view(|v| (v.rec.len(), v.rec.steps.len(), v.rec.runs.len()));
    let mix = m.view(|v| {
        use ksee_core::trace::Record as R;
        let mut c = [0usize; 7];
        for r in &v.rec.records {
            c[match r { R::Block { n: 0, .. } => 0, R::Block { .. } => 1, R::Return { .. } => 2, R::Store { .. } => 3, R::Compare { .. } => 4, R::Var { .. } => 5, R::Task { .. } => 6 }] += 1;
        }
        format!("calls {} blocks {} returns {} stores {} compares {} vars {} tasks {}", c[0], c[1], c[2], c[3], c[4], c[5], c[6])
    });

    // (The syscall with the most records: the page's heaviest view of one.)
    let Some((k, s)) = m.view(|v| (0..calls).filter_map(|k| v.run(k).map(|s| (k, s))).filter(|(_, s)| s.nr.is_some()).max_by_key(|(_, s)| s.last - s.first)) else { return String::new() };
    let mid = (s.first + s.last) / 2;
    let file = m.view(|v| v.call_stack(mid, None, 0, mid).first().and_then(|f| f.file.clone())).unwrap_or_default();
    let chart = ms(3, || { m.view(|v| v.chart(0, steps.saturating_sub(1), 1200)); });
    let runs = ms(3, || { m.view(|v| (0..v.rec.runs.len()).filter_map(|k| v.run(k)).count()); });
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
    format!("{mix}; {records} records, {steps} steps, {calls} runs; the largest, {} records: snapshot {snapshot:.1} ms, \
             chart {chart:.1}, runs {runs:.1}, its lines {lines:.1}, a stack {stack:.1}, its objects {objects:.1}, \
             its stores named {named:.1}, a file's notes {notes:.1} and coverage {coverage:.1} ({file}); at vma_iter_prealloc, a stack {deep:?}", s.last - s.first + 1)
}
