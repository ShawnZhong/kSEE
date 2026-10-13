//! The Memory tab: the objects the run selected changed (View::objects),
//! each a table of its fields, under the task that changed it.

use std::rc::Rc;

use dioxus::prelude::*;
use ksee_core::view::{Node, RegionKind};

use crate::machine;
use crate::state::{Derived, Shared, Ui};

/// The run's objects, and where each global among them is declared.
type Graph = Shared<(Vec<Node>, Vec<Option<(String, u32)>>)>;

/// "3 objects", "1 object".
fn count(n: usize) -> String {
    format!("{n} object{}", if n == 1 { "" } else { "s" })
}

/// The call you read (the Memory and Calls tabs'): the frame chosen (inlined
/// code: the call it is in; the bottom frame: the whole syscall), its first
/// record and name.
pub(crate) fn read_call(d: Derived) -> Memo<Option<(usize, String)>> {
    use_memo(move || {
        let stack = d.stack.read();
        stack.get(*d.frame.read()..).and_then(|up| up.iter().find(|f| !f.inlined)).map(|f| (f.first, f.func.clone()))
    })
}

/// `items` by their keys, in the order each key first comes.
fn grouped<T>(items: impl IntoIterator<Item = (String, T)>) -> Vec<(String, Vec<T>)> {
    let mut out: Vec<(String, Vec<T>)> = Vec::new();
    for (key, item) in items {
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(item),
            None => out.push((key, vec![item])),
        }
    }
    out
}

/// The objects the call you read changed (with what it called), by the
/// task that changed them and by whether their changes survived to its end.
#[component]
pub(crate) fn Objects() -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    // (What it shows: what the call you read changed, with what it called.)
    let scope = read_call(d);
    let graph = use_memo(move || {
        (ui.snapshot)();
        let m = machine();
        let nodes = scope.cloned().map_or_else(Vec::new, |(first, _)| m.view(|v| { let (a, b) = v.call_span(first); v.objects(a, b) }));
        let declared = nodes.iter().map(|n| {
            (n.kind == RegionKind::Global).then(|| m.view(|v| v.definition(&n.what))).flatten().map(|p| (p.file, p.line))
        }).collect();
        Shared(Rc::new((nodes, declared)))
    });
    let Some((_, title)) = scope.cloned() else {
        return rsx! { p { class: "pad dim", "Select a run: what it did to the kernel's data." } };
    };
    let graph = graph.cloned();
    let nodes = &(*graph).0;
    if nodes.is_empty() {
        return rsx! { p { class: "pad dim", "{title} changed nothing of the kernel's data" } };
    }

    let mut depth = vec![0usize; nodes.len()];
    for k in 0..nodes.len() {
        depth[k] = nodes[k].under.map_or(0, |parent| depth[parent] + 1);
    }
    let own: Vec<usize> = (0..nodes.len()).filter(|&k| !nodes[k].same() && nodes[k].by.is_none()).collect();
    let same: Vec<usize> = (0..nodes.len()).filter(|&k| nodes[k].same()).collect();
    let others = grouped((0..nodes.len()).filter(|&k| !nodes[k].same())
        .filter_map(|k| Some((nodes[k].by.as_ref()?.named(), k))));
    let mut head = if own.is_empty() {
        format!("{title} itself changed nothing of the kernel's data")
    } else {
        format!("{title} changed {}", count(own.len()))
    };
    if !others.is_empty() {
        head += &format!("; while it ran, other tasks changed {} more", count(others.iter().map(|(_, list)| list.len()).sum()));
    }
    if !same.is_empty() && !own.is_empty() {
        head += &format!("; left {} as it was", count(same.len()));
    }
    // (Its own; then each other task's, under a head; then those left as
    // they were, under one.)
    let mut sections: Vec<(Option<(&str, &str, String)>, Vec<usize>)> = vec![(None, own)];
    for (task, list) in others {
        let said = format!("By {task}, while it ran: {}", count(list.len()));
        sections.push((Some(("listhead", "a task that ran while it did (one it woke, or waited for), and first wrote these", said)), list));
    }
    if !same.is_empty() {
        let said = format!("Left as it was, changed and changed back: {}", count(same.len()));
        sections.push((Some(("listhead dim", "changed, then changed back by the end (a lock taken and released, a count taken and dropped)", said)), same));
    }

    rsx! {
        div { class: "listing",
            div { class: "listhead", "{head}" }
            for (h , list) in sections {
                if let Some((class, title, said)) = h {
                    div { class, title, "{said}" }
                }
                for k in list {
                    ObjectBox { graph: graph.clone(), k, depth: depth[k] }
                }
            }
        }
    }
}

/// Object `k`: its fields' changes, a row each (a click: the cursor to its
/// last write); a pointer's target, a link to its box.
#[component]
fn ObjectBox(graph: Graph, k: usize, depth: usize) -> Element {
    let mut ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    let cursor = d.line.cloned().map(|l| l.at);
    let (nodes, declared) = &*graph;
    let node = &nodes[k];
    let what = match (node.kind, &node.within) {
        (RegionKind::Object, Some(function)) => format!("{} in {function}", node.what),
        (RegionKind::Object, None) => node.what.clone(),
        _ => node.type_name.clone(),
    };
    let via = node.via.as_ref().map_or(String::new(), |via| format!("  ← {via}"));
    let declared = declared[k].clone();
    let style = if depth > 0 { format!("margin-left: {}px", depth.min(6) * 18) } else { String::new() };
    let mut rows = Vec::new();
    for change in &node.changes {
        for (q, run) in change.streaks.iter().enumerate() {
            let class = match cursor {
                Some(i) if run.first > i => "later",
                Some(i) if i <= run.last => "on",
                _ if change.before == change.after => "same",
                _ => "",
            };
            let times = if run.writes == 1 { "written once".to_string() } else { format!("written {} times", run.writes) };
            let place = run.at.as_ref().map_or(String::new(), |l| format!(" in {} ({}:{})", l.func, l.file, l.line));
            let member = match q {
                0 if change.member.is_empty() => "(itself)".to_string(),
                0 => change.member.clone(),
                _ => String::new(),
            };
            let to = (q + 1 == change.streaks.len()).then_some(change.to).flatten().map(|t| (t, nodes[t].label.clone()));
            let function = run.at.as_ref().map_or(String::new(), |l| l.func.to_string());
            rows.push((class, format!("{times}{place}: click to go there"), member, run.before.clone(), run.after.clone(), to, function, run.last));
        }
    }

    rsx! {
        div { class: if depth > 0 { "obj under" } else { "obj" }, id: "obj{k}", style,
            div { class: "title",
                b { "{node.label}" }
                span { class: "dim", "  {what}  at {node.addr:#x}{via}" }
                if let Some((file, line)) = declared {
                    a { title: "where it is declared", onclick: move |_| ui.seen.set(Some((file.clone(), line))),
                        "  {file}:{line}" }
                }
            }
            table { class: "fields",
                for (class, title, member, before, after, to, function, last) in rows {
                    tr { class, title, onclick: move |_| ui.go_record(last),
                        td { class: "fname", "{member}" }
                        td { class: "fval",
                            span { class: "dim", "{before} → " }
                            match to {
                                Some((target, label)) => rsx! {
                                    a { onclick: move |event| {
                                            event.stop_propagation();
                                            if let Some(e) = crate::chart::element(&format!("obj{target}")) { e.scroll_into_view(); }
                                        }, "{label}" }
                                },
                                None => rsx! { "{after}" },
                            }
                        }
                        td { class: "ftype", "{function}" }
                    }
                }
            }
        }
    }
}
