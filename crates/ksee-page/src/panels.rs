//! Beside the source, a tab each: the Stack (stack.rs); the tasks at the
//! moment shown; what the call selected did to the kernel's data (Objects);
//! the kernel's console.

use std::collections::BTreeSet;
use std::rc::Rc;

use dioxus::prelude::*;
use ksee_core::view::{Group, Node, RegionKind, TaskState};

use crate::machine;
use crate::state::{Derived, Shared, Tab, Ui};

#[component]
pub fn Tabs() -> Element {
    let ui = use_context::<Ui>();
    use_context_provider(|| Signal::new(BTreeSet::<String>::new()));
    let tab = (ui.tab)();
    let head = |t: Tab, label: &'static str, title: &'static str| rsx! {
        a { class: if tab == t { "tab on" } else { "tab" }, title, onclick: move |_| ui.set_tab(t), "{label}" }
    };
    rsx! {
        div { class: "panels",
            nav { class: "tabs",
                {head(Tab::Stack, "Stack", "the calls at the cursor, their values as they were then")}
                {head(Tab::Tasks, "Tasks", "every task at the cursor: what each is doing, its task_struct")}
                {head(Tab::Objects, "Objects", "the objects the call selected changed, and how they point to each other")}
                {head(Tab::Console, "Console", "the kernel's log and the programs' output")}
            }
            match tab {
                Tab::Stack => rsx! { div { class: "pane", crate::stack::Stack {} } },
                Tab::Objects => rsx! { div { class: "pane", Objects {} } },
                Tab::Tasks => rsx! { div { class: "pane", Tasks {} } },
                Tab::Console => rsx! { div { class: "pane", pre { class: "console", "{ui.console}" } } },
            }
        }
    }
}

/// "3 objects", "1 object".
fn count(n: usize) -> String {
    format!("{n} object{}", if n == 1 { "" } else { "s" })
}

/// Their labels, once each: "file, inode, folio ×23".
fn labels(nodes: &[&Node]) -> String {
    let mut out: Vec<(String, usize)> = Vec::new();
    for n in nodes {
        match out.iter_mut().find(|(l, _)| *l == n.label) {
            Some((_, k)) => *k += 1,
            None => out.push((n.label.clone(), 1)),
        }
    }
    out.into_iter().map(|(l, k)| if k > 1 { format!("{l} ×{k}") } else { l }).collect::<Vec<_>>().join(", ")
}

/// What the call selected did to the kernel's data: a line saying it, then
/// a box per object it changed (its own task's, then each other task's
/// that ran meanwhile, then those it left as they were), each under the
/// first that points to it; each member a function changed, a row each,
/// from what to what (a click: that write). The moment shown colors it:
/// the writes not made by then gray, the one it is in marked.
#[component]
fn Objects() -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    let nodes = use_memo(move || {
        (ui.snapshot)();
        let m = machine();
        Shared(Rc::new(d.syscall.cloned().map_or_else(Vec::new, |s| m.view(|v| v.objects(s.first, s.last.unwrap_or(m.records().saturating_sub(1)))))))
    })();
    let Some(s) = d.syscall.cloned() else { return rsx! { p { class: "pad dim", "Select a call: what it did to the kernel's data." } } };
    let title = format!("{} by {}", s.name, s.task.named());
    if nodes.is_empty() {
        return rsx! { p { class: "pad dim", "{title} changed nothing of the kernel's data" } };
    }
    let cur = d.line.cloned().map(|l| l.at);
    let mut depth = vec![0usize; nodes.len()];
    for k in 0..nodes.len() {
        depth[k] = nodes[k].under.map_or(0, |u| depth[u] + 1);
    }
    // (Each group's boxes, by index, in order.)
    let own: Vec<usize> = (0..nodes.len()).filter(|&k| nodes[k].group == Group::Changed && nodes[k].by.is_none()).collect();
    let same: Vec<usize> = (0..nodes.len()).filter(|&k| nodes[k].group == Group::Same).collect();
    // (Each other task's, by task, as they first come.)
    let mut others: Vec<(String, Vec<usize>)> = Vec::new();
    for k in (0..nodes.len()).filter(|&k| nodes[k].group == Group::Changed) {
        let Some(t) = &nodes[k].by else { continue };
        let t = t.named();
        match others.iter_mut().find(|(x, _)| *x == t) {
            Some((_, l)) => l.push(k),
            None => others.push((t, vec![k])),
        }
    }
    let names = |ks: &[usize]| labels(&ks.iter().map(|&k| &nodes[k]).collect::<Vec<_>>());
    let head = format!("{}{}{}",
        if own.is_empty() { format!("{title} itself changed nothing of the kernel's data") } else { format!("{title} changed {}: {}", count(own.len()), names(&own)) },
        if others.is_empty() { String::new() } else { format!("; while it ran, other tasks changed {} more", count(others.iter().map(|(_, l)| l.len()).sum())) },
        if same.is_empty() || own.is_empty() { String::new() } else { format!("; left {} as it was", count(same.len())) });
    let others: Vec<(String, String, Vec<usize>)> = others.into_iter().map(|(t, l)| (t, format!("{}: {}", count(l.len()), names(&l)), l)).collect();
    let same_labels = names(&same);
    rsx! {
        div { class: "objects",
            div { class: "ohead", "{head}" }
            for k in own { ObjectBox { nodes: nodes.clone(), k, depth: depth[k], cur } }
            for (t , said , list) in others {
                div { class: "ohead", title: "a task that ran while it did (one it woke, or waited for), and first wrote these", "By {t}, while it ran: {said}" }
                for k in list { ObjectBox { nodes: nodes.clone(), k, depth: depth[k], cur } }
            }
            if !same.is_empty() {
                div { class: "ohead dim", title: "changed, then changed back by the end (a lock taken and released, a count taken and dropped)",
                    "Left as it was, changed and changed back: {same_labels}" }
                for k in same { ObjectBox { nodes: nodes.clone(), k, depth: depth[k], cur } }
            }
        }
    }
}

/// Object `k`'s box: what it is, as the code names it, where; each member
/// changed, a row a function that changed it in turn (a pointer to another
/// box, at the end: a link to it).
#[component]
fn ObjectBox(nodes: Shared<Vec<Node>>, k: usize, depth: usize, cur: Option<usize>) -> Element {
    let ui = use_context::<Ui>();
    let d = use_context::<Derived>();
    let n = &nodes[k];
    let what = match (n.kind, &n.within) {
        (RegionKind::Object, Some(f)) => format!("{} in {f}", n.what),
        (RegionKind::Object, None) => n.what.clone(),
        _ => n.ty.clone(),
    };
    let via = n.via.as_ref().map_or(String::new(), |v| format!("  ← {v}"));
    // (A global: where it is declared.)
    let decl = (n.kind == RegionKind::Global).then(|| machine().view(|v| v.definition(&n.what))).flatten();
    let style = if depth > 0 { format!("margin-left: {}px", depth.min(6) * 18) } else { String::new() };
    let mut rows = Vec::new();
    for c in &n.changes {
        for (q, r) in c.runs.iter().enumerate() {
            let class = if cur.is_some_and(|i| r.first > i) { "later" } else if cur.is_some_and(|i| r.first <= i && i <= r.last) { "on" } else if c.before == c.after { "same" } else { "" };
            let to = (q + 1 == c.runs.len()).then_some(c.to).flatten();
            let title = format!("{}{}: click to go there", if r.writes == 1 { "written once".to_string() } else { format!("written {} times", r.writes) },
                r.at.as_ref().map_or(String::new(), |l| format!(" in {} ({}:{})", l.func, l.file, l.line)));
            rows.push((class, title, if q == 0 { if c.member.is_empty() { "(itself)".to_string() } else { c.member.clone() } } else { String::new() },
                r.before.clone(), r.after.clone(), to.map(|t| (t, nodes[t].label.clone())), r.at.as_ref().map_or(String::new(), |l| l.func.clone()), r.last));
        }
    }
    rsx! {
        div { class: if depth > 0 { "obj under" } else { "obj" }, id: "obj{k}", style,
            div { class: "title",
                b { "{n.label}" }
                span { class: "dim", "  {what}  at {n.addr:#x}{via}" }
                if let Some(d) = decl {
                    a { title: "where it is declared", onclick: move |_| ui.show_definition(Some((d.file.clone(), d.line))), "  {d.file}:{d.line}" }
                }
            }
            table { class: "objs",
                for (class , title , member , before , after , to , func , last) in rows {
                    tr { class, title, onclick: move |_| ui.go_record(d, last, None),
                        td { class: "fname", "{member}" }
                        td { class: "fval",
                            span { class: "dim", "{before} → " }
                            match to {
                                Some((t, label)) => rsx! { a { onclick: move |e| { e.stop_propagation(); show(t) }, "{label}" } },
                                None => rsx! { "{after}" },
                            }
                        }
                        td { class: "ftype", "{func}" }
                    }
                }
            }
        }
    }
}

/// Box `k` in view.
fn show(k: usize) {
    if let Some(e) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id(&format!("obj{k}"))) {
        e.scroll_into_view();
    }
}

/// What a task's state says, hovered.
fn state(s: TaskState) -> (&'static str, &'static str) {
    match s {
        TaskState::Running => ("running", "running at the cursor: on the CPU"),
        TaskState::Runnable => ("runnable", "on the run queue: waiting for the CPU"),
        TaskState::Sleeping => ("sleeping", "off the run queue: waiting for something (its syscall or call says what)"),
        TaskState::Exited => ("exited", "exited by the cursor: the kernel's task list no longer has it"),
        TaskState::Later => ("later", "made only after the cursor"),
    }
}

/// The tasks at the moment shown, as a tree (a thread under its process's
/// leader, a process under its parent): the one running marked, and open
/// to begin with; each what it is doing and where; open, its task_struct
/// as it was then.
#[component]
fn Tasks() -> Element {
    let ui = use_context::<Ui>();
    (ui.snapshot)();
    let d = use_context::<Derived>();
    let mut open = use_context::<Signal<BTreeSet<String>>>();
    let at = d.line.cloned().map_or(0, |l| l.at);
    let m = machine();
    let list = m.view(|v| v.tasks(at));
    // (Each under the task it sits under, if the list has it; siblings by pid.)
    let known: Vec<u32> = list.iter().map(|t| t.task.addr).collect();
    let up = |t: &ksee_core::view::TaskAt| t.up.filter(|u| known.contains(u));
    let mut order = Vec::new();
    // (Depth first: a task, then those under it.)
    fn visit(list: &[ksee_core::view::TaskAt], up: &dyn Fn(&ksee_core::view::TaskAt) -> Option<u32>, at: Option<u32>, depth: usize, out: &mut Vec<(usize, usize)>) {
        let mut kids: Vec<usize> = (0..list.len()).filter(|&k| up(&list[k]) == at).collect();
        kids.sort_by_key(|&k| list[k].task.pid.unwrap_or(0));
        for k in kids {
            out.push((k, depth));
            visit(list, up, Some(list[k].task.addr), depth + 1, out);
        }
    }
    visit(&list, &up, None, 0, &mut order);
    rsx! {
        table { class: "objs",
            for (k , depth) in order {
                {
                    let t = &list[k];
                    let running = t.state == TaskState::Running;
                    // (Open: the running one unless folded, another if opened.)
                    let p = format!("task/{}", t.task.addr);
                    let is_open = running != open.read().contains(&p);
                    let (said, title) = state(t.state);
                    let place = if t.last.is_some() { t.syscall.clone().or(t.func.clone()).map_or(String::new(), |s| format!(" in {s}")) } else { String::new() };
                    let class = format!("frame{}{}", if running { " on" } else { "" }, if matches!(t.state, TaskState::Later | TaskState::Exited) { " later" } else { "" });
                    let fields = if is_open { m.at_moment(at, |v, mem| v.task_fields(mem, t.task.addr)).fields } else { Vec::new() };
                    let name = t.task.named();
                    let q = p.clone();
                    rsx! {
                        tr { class: "{class} opens", title,
                            onclick: move |_| { let mut s = open.write(); if !s.remove(&q) { s.insert(q.clone()); } },
                            td { colspan: "3", style: "padding-left: {8 + depth * 16}px",
                                a { class: "mono", title: if is_open { "fold it" } else { "its task_struct, as it was at the cursor" },
                                    span { class: "tw", if is_open { "▾ " } else { "▸ " } }
                                    "{name}" }
                                span { class: "dim", "  {said}{place}" }
                            }
                        }
                        for o in fields {
                            crate::stack::Value { o: o.clone(), path: format!("{p}.{}", o.name), depth: 1, at }
                        }
                    }
                }
            }
        }
    }
}
