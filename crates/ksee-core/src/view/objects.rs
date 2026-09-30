//! What a span did to the kernel's data (View::objects): the objects it
//! changed, as a graph.

use super::*;
use super::stores::{Leaf, RegionKind};

/// An object a span of records (a call) changed, which is still there when
/// it returns (see View::objects): as the Stores name it (`what`, `kind`,
/// `ty`, `id`, `addr`, `size`), and the members the call wrote.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Node {
    /// What to call it: an object a pointer reaches, its type's own name
    /// ("file": which variable names it is only how a function does); the
    /// task, a global, their names ("prog(19)", "nr_files").
    pub label: String,
    /// As the code names it ("file", "prog(19)->files"), and the function
    /// that does, if one (Region::within).
    pub what: String,
    pub within: Option<String>,
    pub kind: RegionKind,
    pub ty: String,
    pub id: u32,
    pub addr: u32,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub size: u32,
    pub changes: Vec<Diff>,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub depth: u32,
    /// What happened to it (Group).
    pub group: Group,
    /// Another task than the span's, if one made its first write (one the
    /// span woke, or that ran while it waited: a journal's thread).
    pub by: Option<TaskRef>,
    /// The node it is shown under (of its group: its index), the first
    /// that points to it as the span ends, if any; and by which of its
    /// members ("f_inode"), unless its name says it ("prog(19)->files"
    /// under prog(19), by files: none).
    pub under: Option<usize>,
    pub via: Option<String>,
}

/// What happened to an object: it ends otherwise than it began (what the
/// call did), or as it began (every change undone by the end: a lock
/// taken and released, a count taken and dropped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize), serde(rename_all = "lowercase"))]
pub enum Group {
    Changed,
    Same,
}

/// A member a span of records changed: from `before` (as the span began)
/// to `after` (as it ends; the same: left as it was, a count taken and
/// dropped), a pointer into another node as it ends: `to`, its index; and
/// how, in `runs`: a run a function writing it in turn, in order.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Diff {
    pub member: String,
    pub before: String,
    pub after: String,
    pub to: Option<usize>,
    pub runs: Vec<Run>,
}

/// Writes of one function in a row to a member: from `before` to `after`,
/// `writes` of them, the first at record `first`, the last at `last`, by
/// the code at `at`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Run {
    pub before: String,
    pub after: String,
    pub writes: usize,
    pub first: usize,
    pub last: usize,
    pub at: Option<Line>,
}

impl View<'_> {
    /// What records `first..=last` (a call's) did to the kernel's data:
    /// the objects they changed, not on a stack (a call's locals, and what
    /// a pointer reaches there, are the Stack's; nor the machine's own
    /// variables: the call's plumbing), each member from what it was to
    /// what it is as the span ends; those that end changed first, then
    /// those left as they were (Node::group); as a tree, by their pointers
    /// then (Node::under), in the order shown: each tree where its first
    /// node is in the order of the calls that name them, the outer first
    /// (then the first written first), each node followed by those under it.
    pub fn objects(&self, first: usize, last: usize) -> Vec<Node> {
        let types = &self.tables.types;
        let named = self.stores(first, last);
        // (Each object's writes (its region's, and any other's of its
        // address and type), by member, in order; with the member's leaf.)
        let mut nodes: Vec<(Node, Vec<(String, Leaf, Vec<Wrote>)>)> = Vec::new();
        let mut leaves: BTreeMap<(u32, u32, u32), Vec<(String, Option<Leaf>)>> = BTreeMap::new();
        let own = self.rec.tasks.get(first).copied();
        for w in &named.writes {
            let r = &named.regions[w.region as usize];
            // (The kernel's data, not a stack's: a call's locals, and what a
            // pointer reaches on a stack, are the Stack's.)
            if !matches!(r.kind, RegionKind::Object | RegionKind::Task | RegionKind::Global) || r.size == 0 || r.life.is_some() {
                continue;
            }
            let group = Group::Changed;
            let k = match nodes.iter().position(|(n, _)| n.addr == r.addr && n.id == r.ty) {
                Some(k) => k,
                None => {
                    let task = self.rec.tasks[w.i];
                    let by = (Some(task) != own).then(|| self.task(task, w.i));
                    nodes.push((Node { label: match (r.kind, types.get(types.resolve(r.ty)).name.as_str()) {
                                           (RegionKind::Object, name) if !name.is_empty() => name.to_string(),
                                           _ => r.what.clone(),
                                       },
                                       what: r.what.clone(), within: r.within.clone(), kind: r.kind, ty: types.name(r.ty), id: r.ty, addr: r.addr, size: r.size, changes: Vec::new(),
                                       depth: r.depth, group, by, under: None, via: None }, Vec::new()));
                    nodes.len() - 1
                }
            };
            // (A store's members (one may copy several), once per place.)
            let Record::Store { addr, size, .. } = self.rec.records[w.i] else { continue };
            let wrote = leaves.entry((w.region, addr, size)).or_insert_with(|| self.members(r, w.i)).clone();
            let members = &mut nodes[k].1;
            for (member, leaf) in wrote {
                match members.iter_mut().find(|(m, _, _)| *m == member) {
                    Some((_, _, ws)) => ws.push(*w),
                    None => {
                        let Some(leaf) = leaf else { continue };
                        members.push((member, leaf, alloc::vec![*w]));
                    }
                }
            }
        }
        // (Each member's changes; one only ever written as it was (memory
        // zeroed that was zero), not; an object with none, not. A pointer:
        // where it points as the span ends.)
        let mem = self.rec.memory_at(self.now, last.min(last_record(self.rec)));
        let mut nodes: Vec<(Node, Vec<Option<u32>>)> = nodes.into_iter().filter_map(|(mut n, members)| {
            let mut points = Vec::new();
            for (member, leaf, writes) in members {
                let before = |i: usize| self.value(n.addr, leaf, i.saturating_sub(1));
                let after = |i: usize| self.value(n.addr, leaf, i);
                // (A run: one function's writes in a row; one that left it
                // as it was, not.)
                let func = |w: &Wrote| w.at.and_then(|s| self.tables.functions.get(&s.func)).map(|f| f.name.as_str());
                let mut runs: Vec<Run> = Vec::new();
                for run in writes.chunk_by(|a, b| func(a) == func(b)) {
                    let (a, z) = (run[0], run[run.len() - 1]);
                    runs.push(Run { before: before(a.i), after: after(z.i), writes: run.len(), first: a.i, last: z.i, at: z.at.map(|s| self.line_only(&s)) });
                }
                let (a, z) = (runs[0].before.clone(), runs.last().expect("a run").after.clone());
                runs.retain(|r| r.before != r.after);
                if runs.is_empty() {
                    continue;
                }
                let (ty, off, _) = leaf;
                points.push(matches!(types.get(types.resolve(ty)).kind, Kind::Pointer(_)).then(|| mem.read_u32(n.addr.wrapping_add(off))));
                n.changes.push(Diff { member, before: a, after: z, to: None, runs });
            }
            if n.changes.is_empty() {
                return None;
            }
            // (Lasting, but every member as it was: its changes undone.)
            if n.changes.iter().all(|d| d.before == d.after) {
                n.group = Group::Same;
            }
            // (The members it ends with changed first, then those as they were.)
            let mut both: Vec<(Diff, Option<u32>)> = n.changes.drain(..).zip(points).collect();
            both.sort_by_key(|(d, _)| d.before == d.after);
            let points;
            (n.changes, points) = both.into_iter().unzip();
            Some((n, points))
        }).collect();
        // (What changed, then what was left as it was; in each, the span's
        // task's, then each other's in the order they first wrote; in each,
        // in the order of the calls that name them, the outer first, as a
        // call tree reads; within one call, the first written first. The
        // task first; globals last.)
        let mut tasks: Vec<Option<u32>> = alloc::vec![None];
        for (n, _) in &nodes {
            let t = n.by.as_ref().map(|t| t.addr);
            if !tasks.contains(&t) {
                tasks.push(t);
            }
        }
        nodes.sort_by_key(|n| (n.0.group == Group::Same, tasks.iter().position(|&t| t == n.0.by.as_ref().map(|t| t.addr)), n.0.depth));
        let (mut nodes, points): (Vec<Node>, Vec<Vec<Option<u32>>>) = nodes.into_iter().unzip();
        // (One name for two (the slabs of two caches a callee named by one
        // parameter): the later numbered: "slab #2 in pfmemalloc_match".)
        let mut seen: BTreeMap<(String, Option<String>), u32> = BTreeMap::new();
        for n in &mut nodes {
            let k = seen.entry((n.what.clone(), n.within.clone())).or_default();
            *k += 1;
            if *k > 1 {
                n.what = format!("{} #{k}", n.what);
            }
        }
        let within = |p: u32| nodes.iter().position(|n| n.addr <= p && p < n.addr.saturating_add(n.size));
        // (Each node's pointers to the others of its group and task, as
        // memory is as the span ends: the tree each is shown as.)
        let links: Vec<Vec<(String, usize)>> = nodes.iter().enumerate().map(|(k, n)| {
            types.pointers(n.id).into_iter().filter_map(|(member, off)| Some((member, within(mem.read_u32(n.addr.wrapping_add(off)))
                .filter(|&to| to != k && nodes[to].group == n.group && nodes[to].by == n.by)?))).collect()
        }).collect();
        let tos: Vec<Vec<Option<usize>>> = points.iter().enumerate().map(|(k, p)| p.iter().map(|p| p.and_then(within).filter(|&to| to != k)).collect()).collect();
        // (The tree they are shown in, from the ones nothing points to (then
        // any left: a cycle's), each under the first, in order, that points
        // to it.)
        let mut under = alloc::vec![None; nodes.len()];
        let mut placed = alloc::vec![false; nodes.len()];
        let pointed: alloc::collections::BTreeSet<usize> = links.iter().flatten().map(|l| l.1).collect();
        let roots = (0..nodes.len()).filter(|k| !pointed.contains(k)).chain(0..nodes.len());
        for root in roots {
            if placed[root] {
                continue;
            }
            placed[root] = true;
            let mut queue = alloc::collections::VecDeque::from([root]);
            while let Some(k) = queue.pop_front() {
                for (member, to) in &links[k] {
                    if !placed[*to] {
                        (placed[*to], under[*to]) = (true, Some((k, member.clone())));
                        queue.push_back(*to);
                    }
                }
            }
        }
        // (In the order they are shown: a tree where its first node is in
        // the order above (the task's, which a wait queue's entry may point
        // to, first), its nodes in turn, each followed by those under it.)
        let root = |mut k: usize| {
            while let Some((p, _)) = &under[k] {
                k = *p;
            }
            k
        };
        let mut roots: Vec<usize> = Vec::new();
        for k in 0..nodes.len() {
            if !roots.contains(&root(k)) {
                roots.push(root(k));
            }
        }
        let mut order = Vec::with_capacity(nodes.len());
        let mut stack: Vec<usize> = roots.into_iter().rev().collect();
        while let Some(k) = stack.pop() {
            order.push(k);
            stack.extend((0..nodes.len()).rev().filter(|&c| under[c].as_ref().is_some_and(|(p, _)| *p == k)));
        }
        let mut new = alloc::vec![0; nodes.len()];
        for (at, &k) in order.iter().enumerate() {
            new[k] = at;
        }
        // (A pointer the call wrote: the node it points to.)
        let mut nodes: Vec<Option<Node>> = nodes.into_iter().map(Some).collect();
        order.into_iter().map(|k| {
            let mut n = nodes[k].take().expect("once");
            for (d, to) in n.changes.iter_mut().zip(&tos[k]) {
                d.to = to.map(|to| new[to]);
            }
            let by = under[k].take();
            n.under = by.as_ref().map(|&(p, _)| new[p]);
            n.via = by.map(|(_, member)| member).filter(|m| !n.what.ends_with(&format!("->{m}")));
            n
        }).collect()
    }
}
