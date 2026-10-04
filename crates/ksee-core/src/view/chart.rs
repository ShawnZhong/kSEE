//! The timeline (View::chart): the stack chart's columns and bars over a
//! window of the recording's steps, and the window's moves (a wheel, a
//! drag).

use super::*;

/// The chart window made by a wheel gesture. Its ends are inclusive, while
/// the returned span is the distance between them. A gesture follows its
/// dominant axis so trackpad cross-axis noise cannot pan and zoom at once.
/// `width` is positive; `zoom` is the vertical gesture's span multiplier.
pub fn wheel_window((a, b): (usize, usize), x: f64, width: f64, dx: f64, dy: f64, shift: bool, zoom: f64) -> (f64, usize) {
    let span = (b - a + 1) as f64;
    if shift || dx.abs() > dy.abs() {
        let delta = if shift { dy } else { dx };
        return (a as f64 + delta / width * span, b - a);
    }

    let at = (x / width).clamp(0.0, 1.0);
    let new = span * zoom;
    (a as f64 + at * (span - new), (new as usize).saturating_sub(1))
}

/// A drag of `dx` px over a chart `width` px wide, its window `window`, kept
/// in the recording's `steps`: the window's start and span then, and the
/// offset (px) the drawn chart moves by meanwhile.
pub fn clamped_pan((a, b): (usize, usize), dx: f64, width: f64, steps: usize) -> (f64, usize, f64) {
    let span = b.saturating_sub(a);
    let visible = span.saturating_add(1).min(steps.max(1));
    let max_start = steps.saturating_sub(visible) as f64;
    let start = (a as f64 - dx / width.max(1.0) * visible as f64).clamp(0.0, max_start);
    (start, span, (a as f64 - start) / visible as f64 * width.max(1.0))
}

/// A stack chart (View::chart): its columns, and the bars they stack.
#[derive(Debug, Default)]
pub struct Chart {
    pub bars: Vec<Bar>,
    pub cols: Vec<Column>,
}

impl Chart {
    /// Each row's runs: a bar over the columns it is on, in a row, as the
    /// page draws it: (depth, first column, one past the last, bar).
    pub fn bar_runs(&self) -> Vec<(u32, usize, usize, u32)> {
        let n = self.cols.len();
        let depth = self.cols.iter().map(|c| c.frames.len()).max().unwrap_or(0);
        let mut runs = Vec::new();
        for dd in 0..depth {
            let mut k = 0;
            while k < n {
                let Some(&bar) = self.cols.get(k).and_then(|c| c.frames.get(dd)) else { k += 1; continue };
                let mut e = k + 1;
                while self.cols.get(e).and_then(|c| c.frames.get(dd)) == Some(&bar) {
                    e += 1;
                }
                runs.push((dd as u32, k, e, bar));
                k = e;
            }
        }
        runs
    }

    /// Which task ran when: a run of columns each, as the page draws it
    /// under the chart: (first column, one past the last, task).
    pub fn task_runs(&self) -> Vec<(usize, usize, u32)> {
        let mut runs: Vec<(usize, usize, u32)> = Vec::new();
        for (k, c) in self.cols.iter().enumerate() {
            match runs.last_mut() {
                Some(r) if r.2 == c.task => r.1 = k + 1,
                _ => runs.push((k, k + 1, c.task)),
            }
        }
        runs
    }
}

/// A column: its step and record, the task running there, and its stack
/// then, as bars (indexes in Chart::bars), outermost first.
#[derive(Debug)]
pub struct Column {
    pub step: usize,
    pub i: usize,
    pub task: u32,
    pub frames: Vec<u32>,
}

/// A bar: a call from record `first`, or code inlined into it; its
/// function's name (the inlined one's), and the call's file.
#[derive(Debug)]
pub struct Bar {
    pub first: usize,
    pub name: String,
    pub file: Option<String>,
    pub inlined: bool,
}

impl View<'_> {
    /// A stack chart of steps `first..=last` in `n` columns, each a
    /// moment spread evenly over the steps (Recording::steps): the task
    /// there and its call stack then, outermost first, each call and the
    /// code inlined into it a bar (in `bars`, once each).
    pub fn chart(&self, first: usize, last: usize, n: usize) -> Chart {
        let (t, rec) = (self.tables, self.rec);
        let mut chart = Chart::default();
        let last = last.min(rec.steps.len().saturating_sub(1));
        if rec.steps.is_empty() || n == 0 || first > last {
            return chart;
        }
        let mut known: BTreeMap<(usize, Option<u32>), u32> = BTreeMap::new();
        // (The k-th column: step first + k * span / n.)
        let span = last - first + 1;
        for step in (0..n).map(|k| first + k * span / n) {
            let (i, nth, cut) = rec.steps[step];
            let mut frames = Vec::new();
            let stack = rec.calls.stack_at(t, i, Some(nth as usize));
            for (d, c) in stack.iter().enumerate() {
                // (The innermost call without the code inlined its line has not entered yet, as the Stack has it.)
                let mut nest = self.nest(c);
                if d + 1 == stack.len() {
                    nest.drain(..(cut as usize).min(nest.len() - 1));
                }
                for (k, at) in nest.into_iter().rev() {
                    let bar = *known.entry((c.first, k)).or_insert_with(|| {
                        let place = self.place(c.func);
                        let name = k.map_or_else(|| place.as_ref().map_or("?".into(), |p| p.name.clone()), |k| self.inline_name(k));
                        // (Its file: code inlined, its own function's, where its line is.)
                        let file = match k {
                            Some(_) => at.map(|l| t.file(l).to_string()),
                            None => place.map(|p| p.file),
                        };
                        chart.bars.push(Bar { first: c.first, name, file, inlined: k.is_some() });
                        chart.bars.len() as u32 - 1
                    });
                    frames.push(bar);
                }
            }
            chart.cols.push(Column { step, i, task: rec.tasks.get(i).copied().unwrap_or(0), frames });
        }
        chart
    }
}
