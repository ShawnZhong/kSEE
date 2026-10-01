//! The call selected, as a stack chart: its records in columns (moments,
//! evenly over its steps), each column its task's stack then, the
//! outermost at the bottom; a bar a call (or code inlined into one) over
//! the columns it is on. A click: the cursor there, on that frame.

use dioxus::prelude::*;

use crate::state::Ui;

/// Columns, and a row's height (px).
pub const COLUMNS: usize = 300;
const ROW: usize = 17;

/// Where file `file` is in the kernel (arch::AREAS): its color's class.
pub fn area(file: Option<&str>) -> &'static str {
    let file = file.unwrap_or("");
    ksee_core::arch::AREAS.iter().find(|(p, _)| file.starts_with(p)).map_or("other", |(_, n)| n)
}

#[component]
pub fn Chart() -> Element {
    let ui = use_context::<Ui>();
    let chart = use_context::<crate::state::Derived>().chart.cloned();
    let lines = ui.lines();
    let n = chart.cols.len().max(1);
    let depth = chart.cols.iter().map(|c| c.frames.len()).max().unwrap_or(0);
    // (Each row's runs: a bar over the columns it is on, in a row.)
    let mut runs = Vec::new();
    for d in 0..depth {
        let mut k = 0;
        while k < n {
            let Some(&b) = chart.cols.get(k).and_then(|c| c.frames.get(d)) else { k += 1; continue };
            let mut e = k + 1;
            while chart.cols.get(e).and_then(|c| c.frames.get(d)) == Some(&b) {
                e += 1;
            }
            runs.push((d, k, e, b as usize));
            k = e;
        }
    }
    let at = lines.get((ui.cursor)()).map(|l| l.i);
    let now = at.and_then(|i| chart.cols.iter().rposition(|c| c.i <= i));
    let pct = |k: usize| k as f64 * 100.0 / n as f64;
    rsx! {
        div { class: "chart", style: "height: {depth * ROW}px",
            for (d , a , e , b) in runs {
                {
                    let bar = &chart.bars[b];
                    let (i, deep) = (chart.cols[a].i, chart.cols[a].frames.len());
                    let lines = lines.clone();
                    rsx! {
                        div { class: "bar {area(bar.file.as_deref())}",
                            style: "left: {pct(a)}%; width: {pct(e - a)}%; bottom: {d * ROW}px",
                            title: "{bar.label}",
                            onclick: move |_| {
                                let mut ui = ui;
                                ui.go(lines.iter().rposition(|l| l.i <= i).unwrap_or(0));
                                ui.frame.set(Some(deep - 1 - d));
                            },
                            "{bar.label}"
                        }
                    }
                }
            }
            if let Some(k) = now { div { class: "now", style: "left: {pct(k)}%" } }
        }
    }
}
