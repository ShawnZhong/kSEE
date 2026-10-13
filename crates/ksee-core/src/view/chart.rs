//! The timeline (View::timeline): the whole recording as a stack chart,
//! exactly: each call (and the code inlined into it) a bar over the steps
//! it is on, row by row, and which task ran when; and a window's moves (a
//! wheel). A window is fractional steps (its start, its span), so drawing
//! it is a transform of the steps, not a sampling of them.

use super::*;

/// The window (`start`, `span`, in steps) a wheel gesture makes, the
/// pointer at `x` of `width` px; kept in the recording by its user. A
/// gesture follows its dominant axis so trackpad cross-axis noise cannot
/// pan and zoom at once; `zoom` is the vertical gesture's span multiplier.
pub fn wheel_window((start, span): (f64, f64), x: f64, width: f64, dx: f64, dy: f64, shift: bool, zoom: f64) -> (f64, f64) {
    if shift || dx.abs() > dy.abs() {
        let delta = if shift { dy } else { dx };
        return (start + delta / width * span, span);
    }
    let at = (x / width).clamp(0.0, 1.0);
    let new = span * zoom;
    (start + at * (span - new), new)
}

/// The timeline: its bars, each row's spans of them, and the tasks' spans.
#[derive(Debug, Default)]
pub struct Timeline {
    pub bars: Vec<Bar>,
    /// Per row (0: the outermost): its spans, (first step, one past the
    /// last, bar), in order.
    pub rows: Vec<Vec<(usize, usize, u32)>>,
    /// Which task ran when: (first step, one past the last, task), in order.
    pub tasks: Vec<(usize, usize, u32)>,
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

impl Timeline {
    /// The steps the call from record `first` in `func` (or code inlined
    /// as it) is on: from its first to one past its last.
    pub fn extent(&self, first: usize, func: &str) -> Option<(usize, usize)> {
        let of = |b: u32| self.bars.get(b as usize).is_some_and(|x| x.first == first && x.name == func);
        self.rows.iter().flatten().filter(|s| of(s.2)).fold(None, |e, s| Some(e.map_or((s.0, s.1), |(a, b): (usize, usize)| (a.min(s.0), b.max(s.1)))))
    }

    /// Row `d`'s spans that overlap steps `a..b`.
    pub fn spans(&self, d: usize, a: f64, b: f64) -> &[(usize, usize, u32)] {
        self.rows.get(d).map_or(&[], |row| &row[within(row, a, b)])
    }

    /// Which of the tasks' spans overlap steps `a..b`: their indexes.
    pub fn task_range(&self, a: f64, b: f64) -> core::ops::Range<usize> {
        within(&self.tasks, a, b)
    }

    /// How many rows steps `a..b` stack.
    pub fn depth(&self, a: f64, b: f64) -> usize {
        (0..self.rows.len()).rev().find(|&d| !self.spans(d, a, b).is_empty()).map_or(0, |d| d + 1)
    }

    /// The stack at step `s`: its bars, outermost first.
    pub fn at(&self, s: usize) -> Vec<u32> {
        self.rows.iter().map_while(|row| {
            let k = row.partition_point(|x| x.1 <= s);
            row.get(k).filter(|x| x.0 <= s).map(|x| x.2)
        }).collect()
    }
}

/// Which of `spans` (in order) overlap steps `a..b`: their indexes.
fn within(spans: &[(usize, usize, u32)], a: f64, b: f64) -> core::ops::Range<usize> {
    let i = spans.partition_point(|s| s.1 as f64 <= a);
    i..spans.partition_point(|s| (s.0 as f64) < b).max(i)
}

impl View<'_> {
    /// The timeline: at each step (Recording::steps), the task there and
    /// its call stack then, outermost first, each call and the code
    /// inlined into it a bar (in `bars`, once each), a span of a row for as
    /// long as it stays.
    pub fn timeline(&self) -> Timeline {
        let (t, rec) = (self.tables, self.rec);
        let mut tl = Timeline::default();
        let mut known: BTreeMap<(usize, Option<u32>), u32> = BTreeMap::new();
        // (A call's bars, the code inlined into it at its line, outermost
        // first; the innermost without what it has not entered yet, `cut`.)
        let mut bars = |c: &stack::StackCall, cut: usize, out: &mut Vec<u32>| {
            for (k, at) in self.nest(c, cut).into_iter().rev() {
                out.push(*known.entry((c.first, k)).or_insert_with(|| {
                    let f = t.function(c.func);
                    let name = k.map_or_else(|| f.map_or("?", |f| f.name), |k| t.inline_name(k)).to_string();
                    // (Its file: code inlined, its own function's, where its line is.)
                    let file = match k {
                        Some(_) => at.map(|l| t.file(l).to_string()),
                        None => f.map(|f| t.file(f.loc).to_string()),
                    };
                    tl.bars.push(Bar { first: c.first, name, file, inlined: k.is_some() });
                    tl.bars.len() as u32 - 1
                }));
            }
        };
        // (Each call's callers' bars: its caller's, and its caller's own at
        // the line calling it; found once a call.)
        let mut callers: Vec<Option<alloc::rc::Rc<Vec<u32>>>> = alloc::vec![None; rec.calls.calls.len()];
        let mut callers_of = |k: u32, bars: &mut dyn FnMut(&stack::StackCall, usize, &mut Vec<u32>)| -> alloc::rc::Rc<Vec<u32>> {
            // (The calls out from k whose callers are not found yet, then each, outermost first.)
            let mut chain = Vec::new();
            let mut at = k;
            while callers[at as usize].is_none() {
                match rec.calls.caller(t, at) {
                    Some((parent, call)) => { chain.push((at, call)); at = parent; }
                    None => { callers[at as usize] = Some(Default::default()); }
                }
            }
            for (k, call) in chain.into_iter().rev() {
                let parent = rec.calls.calls[k as usize].parent.unwrap_or(k);
                let mut own = (*callers[parent as usize].clone().unwrap_or_default()).clone();
                bars(&call, 0, &mut own);
                callers[k as usize] = Some(alloc::rc::Rc::new(own));
            }
            callers[k as usize].clone().unwrap_or_default()
        };
        // (And the innermost's, the same while its call, its inlined code and
        // what it has not entered yet are.)
        let (mut inside, mut own) = (None, Vec::new());
        let mut frames = Vec::new();
        // (Each row's bar now, and since which step: closed, a span, as it changes.)
        let mut open: Vec<(usize, u32)> = Vec::new();
        let close = |rows: &mut Vec<Vec<(usize, usize, u32)>>, d: usize, (start, bar): (usize, u32), end: usize| {
            if rows.len() <= d {
                rows.resize_with(d + 1, Vec::new);
            }
            rows[d].push((start, end, bar));
        };
        for (step, &(i, nth, cut)) in rec.steps.iter().enumerate() {
            frames.clear();
            if let Some((k, inner)) = rec.calls.innermost(t, i, Some(nth as usize)) {
                frames.extend_from_slice(&callers_of(k, &mut bars));
                let key = (inner.first, inner.step.and_then(|s| s.inlined()), cut);
                if inside != Some(key) {
                    own.clear();
                    bars(&inner, cut as usize, &mut own);
                    inside = Some(key);
                }
                frames.extend_from_slice(&own);
            }
            let same = open.iter().zip(&frames).take_while(|(o, f)| o.1 == **f).count();
            for (d, o) in open.drain(same..).enumerate().rev() {
                close(&mut tl.rows, same + d, o, step);
            }
            open.extend(frames[same..].iter().map(|&b| (step, b)));
            extend(&mut tl.tasks, step, rec.tasks.get(i).copied().unwrap_or(0));
        }
        for (d, o) in open.into_iter().enumerate() {
            close(&mut tl.rows, d, o, rec.steps.len());
        }
        tl
    }
}

/// `spans` with `what` at `step`: the last span longer, if it is `what`'s
/// and ends there; else a new one.
fn extend(spans: &mut Vec<(usize, usize, u32)>, step: usize, what: u32) {
    match spans.last_mut() {
        Some(s) if s.1 == step && s.2 == what => s.1 = step + 1,
        _ => spans.push((step, step + 1, what)),
    }
}
