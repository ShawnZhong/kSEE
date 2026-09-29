//! What a span did to the kernel's data (View::objects): the objects it
//! changed, as a graph.

use super::*;
use super::stores::Leaf;

/// An object a span of records (a call) changed, which is still there when
/// it returns (see View::objects): as the Stores name it (`what`, `kind`,
/// `ty`, `id`, `addr`, `size`), and the members the call wrote.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Node {
    pub what: String,
    pub kind: &'static str,
    pub ty: String,
    pub id: u32,
    pub addr: u32,
    pub size: u32,
    pub changes: Vec<Diff>,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub depth: u32,
    /// What happened to it: "changed" (it ends otherwise than it began:
    /// what the call did), or "same" (every change undone by the end: a
    /// lock taken and released, a count taken and dropped).
    pub group: &'static str,
    /// The node it is shown under (of its group), and by which of its
    /// members: the first that points to it as the span ends ("f_inode"),
    /// if any.
    pub under: Option<(usize, String)>,
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
    /// those left as they were (Node::group); as a graph, by their pointers
    /// then. In the order of the calls that name them, the outer first
    /// (then the first written first).
    pub fn objects(&self, first: usize, last: usize) -> Vec<Node> {
        let types = &self.tables.types;
        let named = self.stores(first, last);
        // (Each object's writes (its region's, and any other's of its
        // address and type), by member, in order; with the member's leaf.)
        let mut nodes: Vec<(Node, Vec<(String, Leaf, Vec<Wrote>)>)> = Vec::new();
        let mut leaves: BTreeMap<(u32, u32, u32), Vec<(String, Option<Leaf>)>> = BTreeMap::new();
        for w in &named.writes {
            let r = &named.regions[w.region as usize];
            // (The kernel's data, not a stack's: a call's locals, and what a
            // pointer reaches on a stack, are the Stack's.)
            if !matches!(r.kind, "object" | "task" | "global") || r.size == 0 || r.life.is_some() {
                continue;
            }
            let group = "changed";
            let k = match nodes.iter().position(|(n, _)| n.addr == r.addr && n.id == r.ty) {
                Some(k) => k,
                None => {
                    nodes.push((Node { what: r.what.clone(), kind: r.kind, ty: types.name(r.ty), id: r.ty, addr: r.addr, size: r.size, changes: Vec::new(),
                                       depth: r.depth, group, under: None }, Vec::new()));
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
                n.group = "same";
            }
            // (The members it ends with changed first, then those as they were.)
            let mut both: Vec<(Diff, Option<u32>)> = n.changes.drain(..).zip(points).collect();
            both.sort_by_key(|(d, _)| d.before == d.after);
            let points;
            (n.changes, points) = both.into_iter().unzip();
            Some((n, points))
        }).collect();
        // (What changed, then what was left as it was; in each, in the order of the calls that name them, the outer first,
        // as a call tree reads; within one call, the first written first.
        // The task first; globals last.)
        nodes.sort_by_key(|n| (n.0.group == "same", n.0.depth));
        let (mut nodes, points): (Vec<Node>, Vec<Vec<Option<u32>>>) = nodes.into_iter().unzip();
        // (One name for two (the slabs of two caches a callee named by one
        // parameter): the later numbered: "slab in pfmemalloc_match #2".)
        let mut seen: BTreeMap<String, u32> = BTreeMap::new();
        for n in &mut nodes {
            let k = seen.entry(n.what.clone()).or_default();
            *k += 1;
            if *k > 1 {
                n.what = format!("{} #{k}", n.what);
            }
        }
        let within = |p: u32| nodes.iter().position(|n| n.addr <= p && p < n.addr.saturating_add(n.size));
        // (Each node's pointers to the others of its group, as memory is as
        // the span ends: the tree each group is shown as.)
        let links: Vec<Vec<(String, usize)>> = nodes.iter().enumerate().map(|(k, n)| {
            types.pointers(n.id, 3).into_iter().filter_map(|(member, off)| Some((member, within(mem.read_u32(n.addr.wrapping_add(off)))
                .filter(|&to| to != k && nodes[to].group == n.group)?))).collect()
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
        // (A pointer the call wrote: the node it points to.)
        for ((n, to), u) in nodes.iter_mut().zip(tos).zip(under) {
            for (d, to) in n.changes.iter_mut().zip(to) {
                d.to = to;
            }
            n.under = u;
        }
        nodes
    }
}
