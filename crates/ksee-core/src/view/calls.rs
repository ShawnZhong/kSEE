//! A call's tree (the Calls tab): what it called, and what those called,
//! with the code inlined into each where it is, each function once per path
//! (however often it was called), in the order first reached.

use super::*;

/// A function in a call's tree, `depth` deep (the call itself: 0): a call
/// (`count` of them along this path of callers), or code inlined into the
/// one it is under; first reached at record `first`.
#[derive(Debug, Clone, PartialEq)]
pub struct CallNode {
    pub name: String,
    pub depth: u32,
    pub inlined: bool,
    pub count: u32,
    pub first: usize,
}

/// A node being made: its name and whether inlined, count, first record,
/// children in order.
struct Made {
    key: (String, bool),
    count: u32,
    first: usize,
    children: Vec<usize>,
}

/// Node `at`'s child `key`, made if new (first reached at record `i`).
fn child(nodes: &mut Vec<Made>, at: usize, key: (String, bool), i: usize) -> usize {
    if let Some(&c) = nodes[at].children.iter().find(|&&c| nodes[c].key == key) {
        return c;
    }
    nodes.push(Made { key, count: 0, first: i, children: Vec::new() });
    let c = nodes.len() - 1;
    nodes[at].children.push(c);
    c
}

impl View<'_> {
    /// The tree of the call from record `first` (a block 0), in order: each
    /// node, then what is under it.
    pub fn call_tree(&self, first: usize) -> Vec<CallNode> {
        let (t, calls) = (self.tables, &self.rec.calls);
        let Some(root) = calls.call_index(first) else { return Vec::new() };
        let (a, b) = self.call_span(first);
        let mut nodes = alloc::vec![Made { key: (self.name(calls.calls[root as usize].func), false), count: 1, first, children: Vec::new() }];
        // (Each call in the tree: its node.)
        let mut node_of: BTreeMap<u32, usize> = BTreeMap::from([(root, 0)]);
        // (Code inlined, as a chain says it: the functions, the outermost first.)
        let inlined = |chain: Option<u32>| {
            let mut names: Vec<String> = t.chain(chain).map(|s| t.inline_name(s.name).to_string()).collect();
            names.reverse();
            names
        };
        for i in a..=b {
            let Record::Block { func, n } = self.rec.records[i] else { continue };
            let Some(k) = calls.index_at(i) else { continue };
            // (A call: under its caller, in the code inlined into it it is
            // called from; not one of another task's.)
            if n == 0 && k != root
                && let Some((parent, at)) = calls.caller(t, k)
                && let Some(&p) = node_of.get(&parent) {
                let node = inlined(at.step.and_then(|s| s.inlined())).into_iter().fold(p, |at, name| child(&mut nodes, at, (name, true), i));
                let node = child(&mut nodes, node, (self.name(func), false), i);
                nodes[node].count += 1;
                node_of.insert(k, node);
            }
            // (The code inlined into it the block runs.)
            let Some(&node) = node_of.get(&k) else { continue };
            let mut last = None;
            for s in t.block(func, n).map_or(&[][..], |b| b.steps) {
                if s.inlined() != last {
                    last = s.inlined();
                    inlined(last).into_iter().fold(node, |at, name| child(&mut nodes, at, (name, true), i));
                }
            }
        }
        let (mut out, mut todo) = (Vec::new(), alloc::vec![(0, 0)]);
        while let Some((k, depth)) = todo.pop() {
            let n = &nodes[k];
            out.push(CallNode { name: n.key.0.clone(), depth, inlined: n.key.1, count: n.count, first: n.first });
            todo.extend(n.children.iter().rev().map(|&c| (c, depth + 1)));
        }
        out
    }
}
