//! The Calls tab: the syscall the cursor is in (its outermost call), what
//! it called and what those called, with the code inlined into each where
//! it is, as a tree (View::call_tree): each function once per path, how
//! often it was called there. The frames the cursor is in are marked, the
//! innermost most; a function opens where it is first reached; a tree folds.

use std::collections::BTreeSet;

use dioxus::prelude::*;

use crate::machine;
use crate::state::{Derived, Ui};

#[component]
pub(crate) fn Calls() -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    // (The outermost call at the cursor, and the frames out to it, by name.)
    let root = use_memo(move || d.stack.read().iter().rev().find(|f| !f.inlined).map(|f| f.first));
    let path = use_memo(move || d.stack.read().iter().rev().map(|f| f.func.clone()).collect::<Vec<_>>());
    let tree = use_memo(move || {
        (ui.snapshot)();
        root().map_or_else(Vec::new, |first| machine().view(|v| v.call_tree(first)))
    });
    // (The nodes folded, by index.)
    let mut folded = use_signal(BTreeSet::<usize>::new);
    let tree = tree.read();
    if tree.is_empty() {
        return rsx! { p { class: "pad dim", "Select a run: what it called." } };
    }
    let path = path.read();
    // (Those not under a folded one; each marked if the cursor is in it.)
    let mut shown = Vec::new();
    let (mut under, mut at): (Option<u32>, Vec<&str>) = (None, Vec::new());
    for (k, n) in tree.iter().enumerate() {
        at.truncate(n.depth as usize);
        at.push(&n.name);
        let on = path.len() >= at.len() && path.iter().zip(&at).all(|(a, b)| a == b);
        let here = on && path.len() == at.len();
        if under.is_some_and(|depth| n.depth > depth) {
            continue;
        }
        under = folded.read().contains(&k).then_some(n.depth);
        let parent = tree.get(k + 1).is_some_and(|next| next.depth > n.depth);
        let class = if here { "call here" } else if on { "call on" } else { "call" };
        shown.push((k, n.clone(), parent, class));
    }
    rsx! {
        div { class: "calls mono",
            for (k, n, parent, class) in shown {
                div { class, style: "padding-left: {n.depth as f32 * 1.2 + 0.4}em",
                    if parent {
                        a { class: "fold dim", title: "fold, unfold",
                            onclick: move |_| { let mut f = folded.write(); if !f.remove(&k) { f.insert(k); } },
                            if folded.read().contains(&k) { "▸ " } else { "▾ " } }
                    } else {
                        span { class: "fold", "  " }
                    }
                    // (Its frame the one shown: the innermost, at its first record.)
                    a { title: "its first call here", onclick: move |_| ui.commit(Ui::moment_at(n.first, None, None)), "{n.name}" }
                    if n.inlined { span { class: "dim", " · inlined" } }
                    if n.count > 1 { span { class: "dim", " ×{n.count}" } }
                }
            }
        }
    }
}
