//! Target-neutral rendering for the objects changed by a recorded call.

use std::collections::HashMap;
use std::rc::Rc;

use dioxus::prelude::*;
use ksee_core::view::{Group, Node, RegionKind};

#[derive(Clone, PartialEq)]
pub(crate) struct ObjectDefinition {
    pub file: String,
    pub line: u32,
}

/// "3 objects", "1 object".
fn count(n: usize) -> String {
    format!("{n} object{}", if n == 1 { "" } else { "s" })
}

/// Their labels, once each: "file, inode, folio ×23".
fn labels(nodes: &[&Node]) -> String {
    let mut indexes: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<(String, usize)> = Vec::new();
    for node in nodes {
        match indexes.get(&node.label).copied() {
            Some(index) => out[index].1 += 1,
            None => {
                indexes.insert(node.label.clone(), out.len());
                out.push((node.label.clone(), 1));
            }
        }
    }
    out.into_iter()
        .map(|(label, count)| {
            if count > 1 {
                format!("{label} ×{count}")
            } else {
                label
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The objects changed by a call, grouped by the task that changed them and
/// by whether their changes survived to the end of the call.
#[component]
pub(crate) fn ObjectsView(
    nodes: Rc<Vec<Node>>,
    title: String,
    cursor: Option<usize>,
    definitions: Rc<Vec<Option<ObjectDefinition>>>,
    on_record: EventHandler<usize>,
    on_definition: EventHandler<ObjectDefinition>,
    on_object: EventHandler<usize>,
) -> Element {
    if nodes.is_empty() {
        return rsx! { p { class: "pad dim", "{title} changed nothing of the kernel's data" } };
    }

    let mut depth = vec![0usize; nodes.len()];
    for k in 0..nodes.len() {
        depth[k] = nodes[k].under.map_or(0, |parent| depth[parent] + 1);
    }
    let own: Vec<usize> = (0..nodes.len())
        .filter(|&k| nodes[k].group == Group::Changed && nodes[k].by.is_none())
        .collect();
    let same: Vec<usize> = (0..nodes.len())
        .filter(|&k| nodes[k].group == Group::Same)
        .collect();
    let mut other_indexes: HashMap<String, usize> = HashMap::new();
    let mut others: Vec<(String, Vec<usize>)> = Vec::new();
    for k in (0..nodes.len()).filter(|&k| nodes[k].group == Group::Changed) {
        let Some(task) = &nodes[k].by else { continue };
        let task = task.named();
        match other_indexes.get(&task).copied() {
            Some(index) => others[index].1.push(k),
            None => {
                other_indexes.insert(task.clone(), others.len());
                others.push((task, vec![k]));
            }
        }
    }
    let names = |indexes: &[usize]| labels(&indexes.iter().map(|&k| &nodes[k]).collect::<Vec<_>>());
    let head = format!(
        "{}{}{}",
        if own.is_empty() {
            format!("{title} itself changed nothing of the kernel's data")
        } else {
            format!("{title} changed {}: {}", count(own.len()), names(&own))
        },
        if others.is_empty() {
            String::new()
        } else {
            format!(
                "; while it ran, other tasks changed {} more",
                count(others.iter().map(|(_, list)| list.len()).sum())
            )
        },
        if same.is_empty() || own.is_empty() {
            String::new()
        } else {
            format!("; left {} as it was", count(same.len()))
        },
    );
    let others: Vec<(String, String, Vec<usize>)> = others
        .into_iter()
        .map(|(task, list)| {
            (
                task,
                format!("{}: {}", count(list.len()), names(&list)),
                list,
            )
        })
        .collect();
    let same_labels = names(&same);

    rsx! {
        div { class: "objects",
            div { class: "ohead", "{head}" }
            for k in own {
                ObjectBox { nodes: nodes.clone(), definitions: definitions.clone(), k, depth: depth[k], cursor, on_record, on_definition, on_object }
            }
            for (task, said, list) in others {
                div { class: "ohead", title: "a task that ran while it did (one it woke, or waited for), and first wrote these", "By {task}, while it ran: {said}" }
                for k in list {
                    ObjectBox { nodes: nodes.clone(), definitions: definitions.clone(), k, depth: depth[k], cursor, on_record, on_definition, on_object }
                }
            }
            if !same.is_empty() {
                div { class: "ohead dim", title: "changed, then changed back by the end (a lock taken and released, a count taken and dropped)",
                    "Left as it was, changed and changed back: {same_labels}" }
                for k in same {
                    ObjectBox { nodes: nodes.clone(), definitions: definitions.clone(), k, depth: depth[k], cursor, on_record, on_definition, on_object }
                }
            }
        }
    }
}

#[component]
fn ObjectBox(
    nodes: Rc<Vec<Node>>,
    definitions: Rc<Vec<Option<ObjectDefinition>>>,
    k: usize,
    depth: usize,
    cursor: Option<usize>,
    on_record: EventHandler<usize>,
    on_definition: EventHandler<ObjectDefinition>,
    on_object: EventHandler<usize>,
) -> Element {
    let node = &nodes[k];
    let what = match (node.kind, &node.within) {
        (RegionKind::Object, Some(function)) => format!("{} in {function}", node.what),
        (RegionKind::Object, None) => node.what.clone(),
        _ => node.ty.clone(),
    };
    let via = node
        .via
        .as_ref()
        .map_or(String::new(), |via| format!("  ← {via}"));
    let definition = definitions.get(k).cloned().flatten();
    let style = if depth > 0 {
        format!("margin-left: {}px", depth.min(6) * 18)
    } else {
        String::new()
    };
    let mut rows = Vec::new();
    for change in &node.changes {
        for (q, run) in change.runs.iter().enumerate() {
            let class = if cursor.is_some_and(|i| run.first > i) {
                "later"
            } else if cursor.is_some_and(|i| run.first <= i && i <= run.last) {
                "on"
            } else if change.before == change.after {
                "same"
            } else {
                ""
            };
            let to = (q + 1 == change.runs.len()).then_some(change.to).flatten();
            let title = format!(
                "{}{}: click to go there",
                if run.writes == 1 {
                    "written once".to_string()
                } else {
                    format!("written {} times", run.writes)
                },
                run.at.as_ref().map_or(String::new(), |line| format!(
                    " in {} ({}:{})",
                    line.func, line.file, line.line
                )),
            );
            rows.push((
                class,
                title,
                if q == 0 {
                    if change.member.is_empty() {
                        "(itself)".to_string()
                    } else {
                        change.member.clone()
                    }
                } else {
                    String::new()
                },
                run.before.clone(),
                run.after.clone(),
                to.map(|target| (target, nodes[target].label.clone())),
                run.at
                    .as_ref()
                    .map_or(String::new(), |line| line.func.clone()),
                run.last,
            ));
        }
    }

    rsx! {
        div { class: if depth > 0 { "obj under" } else { "obj" }, id: "obj{k}", style,
            div { class: "title",
                b { "{node.label}" }
                span { class: "dim", "  {what}  at {node.addr:#x}{via}" }
                if let Some(definition) = definition {
                    a { title: "where it is declared", onclick: move |_| on_definition.call(definition.clone()),
                        "  {definition.file}:{definition.line}" }
                }
            }
            table { class: "objs",
                for (class, title, member, before, after, to, function, last) in rows {
                    tr { class, title, onclick: move |_| on_record.call(last),
                        td { class: "fname", "{member}" }
                        td { class: "fval",
                            span { class: "dim", "{before} → " }
                            match to {
                                Some((target, label)) => rsx! {
                                    a { onclick: move |event| { event.stop_propagation(); on_object.call(target); }, "{label}" }
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
