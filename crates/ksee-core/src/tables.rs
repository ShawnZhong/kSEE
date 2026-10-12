//! The code and type tables (the ksee.code and ksee.types sections) the
//! trace plugin and the pack put in the kernel module.

use alloc::collections::BTreeMap;

use crate::arch;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// A source location; `file` indexes [`Tables::files`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Loc {
    pub file: u32,
    pub line: u32,
}

/// An instrumented function.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    /// Where it is defined.
    pub loc: Loc,
    /// Which of the source's parameters call records carry (a bit each);
    /// the others were optimized away.
    pub known: u32,
    /// Which take two words (a bit each: 64 bits wide); the others one.
    pub wide: u32,
    /// Only its call and returns are recorded, not its blocks (it loops,
    /// writes nothing the host undoes, calls nothing: see the plugin).
    pub sealed: bool,
}

/// A named variable kept in a wasm local, whose values the recording has
/// (Record::Var): variable `name` of function `function` (the one whose
/// records they are, or one inlined into it; `of`: its id), parameter
/// `param` of it (from 1; 0: a local). `arg`: if nonzero, it is the
/// call's parameter `arg` throughout (an inlined call's parameter passed
/// one of the call's own), with no records.
#[derive(Debug, Clone, PartialEq)]
pub struct Var {
    pub of: u32,
    /// (Shared: the same names, many times over.)
    pub function: &'static str,
    pub name: &'static str,
    pub param: u32,
    pub arg: u32,
}

/// A source line a block runs; `inlined`: if it is code inlined into the
/// block's function, the functions it is inlined from (an index into
/// [`Tables::inlines`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub loc: Loc,
    pub inlined: Option<u32>,
}

use crate::types::Types;

/// A basic block: the source lines it runs, in order, in stretches between
/// the calls it makes (a stretch runs at the block's record, the next at
/// each return into it). A Store's, Compare's or Var's `line` indexes
/// `steps`. (A view of the Tables' steps and cuts: Tables::block.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Block<'a> {
    pub steps: &'a [Step],
    /// Where each stretch begins in `steps` (the first at 0).
    cuts: &'a [u32],
}

/// Where block `key` (function, n)'s steps and cuts are in the Tables'.
#[derive(Debug, Clone, Copy)]
struct BlockAt {
    key: (u32, u32),
    steps: (u32, u32),
    cuts: (u32, u32),
}

impl<'a> Block<'a> {
    /// Where it begins: its first line (the function's signature, for its
    /// entry block).
    pub fn loc(&self) -> Loc {
        self.steps.first().map_or(Loc { file: u32::MAX, line: 0 }, |s| s.loc)
    }

    /// Stretch `seg` (none past the last).
    pub fn stretch(&self, seg: u32) -> &'a [Step] {
        let steps: &'a [Step] = self.steps;
        &steps[self.start(seg)..self.start(seg + 1)]
    }

    /// The last line it has run by stretch `seg` (seg's, or an earlier
    /// one's if seg has none), if any.
    pub fn last_by(&self, seg: u32) -> Option<Step> {
        self.steps[..self.start(seg + 1)].last().copied()
    }

    /// Where stretch `seg` begins in `steps`.
    pub fn start(&self, seg: u32) -> usize {
        self.cuts.get(seg as usize).map_or(self.steps.len(), |&a| a as usize)
    }

    /// The stretch step `index` is in (of stretches beginning at one
    /// index, the empty ones between two calls, the last), if it has one.
    pub fn stretch_of(&self, index: u32) -> Option<u32> {
        ((index as usize) < self.steps.len()).then(|| self.cuts.partition_point(|&a| a <= index) as u32 - 1)
    }
}

#[derive(Debug, Default)]
pub struct Tables {
    pub functions: BTreeMap<u32, Function>,
    /// The blocks, by (function, n): where each one's steps and cuts are
    /// in `steps` and `cuts`, every block's in one (a hundred thousand
    /// blocks: not an allocation each). See block.
    blocks: Vec<BlockAt>,
    steps: Vec<Step>,
    cuts: Vec<u32>,
    /// Names of inlined functions.
    pub inline_names: Vec<String>,
    /// Inline chains (see [`Step`]): innermost first, each an inlined
    /// function (in [`Tables::inline_names`]) and where it is inlined (a line of
    /// the next one out; the last: of the block's function).
    pub inlines: Vec<Vec<(u32, Loc)>>,
    /// Source file paths.
    pub files: Vec<String>,
    /// The kernel's types.
    pub types: Types,
    /// (function, k) -> its variable k, as its Var records have it.
    pub vars: BTreeMap<(u32, u32), Var>,
}

/// Interns file paths while parsing.
#[derive(Default)]
struct Files {
    ids: BTreeMap<String, u32>,
    paths: Vec<String>,
}

impl Files {
    fn intern(&mut self, path: &str) -> u32 {
        match self.ids.get(path) {
            Some(&id) => id,
            None => {
                self.paths.push(path.to_string());
                let id = self.paths.len() as u32 - 1;
                self.ids.insert(path.to_string(), id);
                id
            }
        }
    }
}

/// A compilation unit's own numbering: its files (P lines) and its inline
/// sites (I lines: the function inlined, and where).
#[derive(Default)]
struct Unit {
    files: BTreeMap<u32, u32>,
    sites: BTreeMap<u32, (u32, Loc)>,
}

impl Unit {
    /// Parse "k:line" (k: a P line's).
    fn loc(&self, s: &str) -> Loc {
        let (k, line) = s.split_once(':').unwrap_or((s, ""));
        let file = k.parse().ok().and_then(|k| self.files.get(&k).copied()).unwrap_or(u32::MAX);
        Loc { file, line: line.parse().unwrap_or(0) }
    }
}

impl Tables {
    /// Parse the `ksee.code` and `ksee.types` sections. Lines are
    /// tab-separated and may repeat (one copy per compilation unit).
    pub fn parse(code: &str, types: &str) -> Tables {
        Tables::with_types(code, Types::parse(types))
    }

    /// Parse the `ksee.code` section, the types parsed already (the host:
    /// one section's text at a time, the kernel's being megabytes).
    pub fn with_types(code: &str, types: Types) -> Tables {
        let mut t = Tables { types, ..Default::default() };
        let mut files = Files::default();
        let mut unit = Unit::default();
        let mut names: BTreeMap<String, u32> = BTreeMap::new();
        let mut chains: BTreeMap<Vec<(u32, Loc)>, u32> = BTreeMap::new();
        let mut seen: alloc::collections::BTreeSet<(u32, u32)> = Default::default();
        // (Each name once: variables repeat their functions' and their own.)
        let mut pool = crate::types::Names::default();
        let mut intern = |s: &str| pool.name(s);
        for line in code.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            match cols.as_slice() {
                ["U"] => unit = Unit::default(),
                ["P", k, path] => {
                    if let Ok(k) = k.parse() {
                        unit.files.insert(k, files.intern(path));
                    }
                }
                ["I", k, name, site] => {
                    if let Ok(k) = k.parse() {
                        let name = *names.entry(name.to_string()).or_insert_with(|| {
                            t.inline_names.push(name.to_string());
                            t.inline_names.len() as u32 - 1
                        });
                        unit.sites.insert(k, (name, unit.loc(site)));
                    }
                }
                ["V", id, k, of, function, name, param, arg] => {
                    if let (Ok(id), Ok(k), Ok(of)) = (id.parse(), k.parse(), of.parse()) && !name.starts_with(crate::arch::MADE_UP) {
                        let (param, arg) = (param.parse().unwrap_or(0), arg.parse().unwrap_or(0));
                        t.vars.insert((id, k), Var { of, function: intern(function), name: intern(name), param, arg });
                    }
                }
                ["F", id, name, location, known, rest @ ..] => {
                    let Ok(id) = id.parse() else { continue };
                    let loc = unit.loc(location);
                    let sealed = rest.first() == Some(&"1");
                    let wide = rest.get(1).and_then(|w| w.parse().ok()).unwrap_or(0);
                    t.functions.insert(id, Function { name: name.to_string(), loc, known: known.parse().unwrap_or(0), wide, sealed });
                }
                ["L", id, n, items] => {
                    if let (Ok(id), Ok(n)) = (id.parse(), n.parse()) {
                        let (mut steps, mut cuts) = (Vec::new(), alloc::vec![0]);
                        let mut file = ""; // ":line": the item before's
                        for item in items.split(';') {
                            if item == "|" {
                                cuts.push(steps.len() as u32);
                                continue;
                            }
                            let (at, inl) = item.split_once('@').map_or((item, None), |(a, f)| (a, Some(f)));
                            // The functions it is inlined from: an I line's site each.
                            let chain: Vec<(u32, Loc)> = inl.into_iter().flat_map(|c| c.split('@'))
                                .filter_map(|k| unit.sites.get(&k.parse::<u32>().ok()?).copied()).collect();
                            let full;
                            let at = match at.strip_prefix(':') {
                                Some(line) => {
                                    full = alloc::format!("{file}:{line}");
                                    full.as_str()
                                }
                                None => {
                                    file = at.split_once(':').map_or("", |(f, _)| f);
                                    at
                                }
                            };
                            let inlined = (!chain.is_empty()).then(|| *chains.entry(chain.clone()).or_insert_with(|| {
                                t.inlines.push(chain);
                                t.inlines.len() as u32 - 1
                            }));
                            steps.push(Step { loc: unit.loc(at), inlined });
                        }
                        // (Once: a unit's copy of a block another unit gave.)
                        if seen.insert((id, n)) {
                            let at = |v: usize, n: usize| (v as u32, (v + n) as u32);
                            t.blocks.push(BlockAt { key: (id, n), steps: at(t.steps.len(), steps.len()), cuts: at(t.cuts.len(), cuts.len()) });
                            t.steps.extend(steps);
                            t.cuts.extend(cuts);
                        }
                    }
                }
                _ => {}
            }
        }
        t.blocks.sort_by_key(|b| b.key);
        t.steps.shrink_to_fit();
        t.cuts.shrink_to_fit();
        t.files = files.paths;
        t.fold_primitives();
        t
    }

    /// The machine's primitives (arch::PRIMITIVES) inlined into kernel code
    /// are the machine, not calls: a step in one becomes a step at the line
    /// it is inlined at, in the function that inlined it, so that nothing
    /// shows a frame, a node or a line of get_current.
    fn fold_primitives(&mut self) {
        let prim: Vec<bool> = self.inline_names.iter()
            .map(|n| self.types.declared.get(n).is_some_and(|(f, _)| arch::PRIMITIVES.iter().any(|p| f.starts_with(p)))).collect();
        if !prim.iter().any(|&p| p) {
            return;
        }
        let mut chains: BTreeMap<Vec<(u32, Loc)>, u32> = self.inlines.iter().enumerate().map(|(i, c)| (c.clone(), i as u32)).collect();
        let mut folded: BTreeMap<u32, (Loc, Option<u32>)> = BTreeMap::new();
        for s in self.steps.iter_mut() {
            let Some(c) = s.inlined else { continue };
            if let Some(&(loc, inlined)) = folded.get(&c) {
                (s.loc, s.inlined) = (loc, inlined);
                continue;
            }
            let chain = &self.inlines[c as usize];
            let n = chain.iter().take_while(|&&(k, _)| prim[k as usize]).count();
            if n == 0 {
                continue;
            }
            let loc = chain[n - 1].1;
            let rest = chain[n..].to_vec();
            let inlined = (!rest.is_empty()).then(|| *chains.entry(rest.clone()).or_insert_with(|| {
                self.inlines.push(rest);
                self.inlines.len() as u32 - 1
            }));
            folded.insert(c, (loc, inlined));
            (s.loc, s.inlined) = (loc, inlined);
        }
    }

    /// Block `n` of `func`.
    pub fn block(&self, func: u32, n: u32) -> Option<Block<'_>> {
        let k = self.blocks.binary_search_by_key(&(func, n), |b| b.key).ok()?;
        Some(self.view(&self.blocks[k]))
    }

    fn view(&self, b: &BlockAt) -> Block<'_> {
        Block { steps: &self.steps[b.steps.0 as usize..b.steps.1 as usize], cuts: &self.cuts[b.cuts.0 as usize..b.cuts.1 as usize] }
    }

    /// Every line block `n` of `func` runs: its lines' own, and the lines
    /// the code of each is inlined at (`x = helper();`, helper inlined, runs
    /// no code of its own line's: the helper's lines run in its place; the
    /// debug info's inline sites say where). What coverage counts, and what
    /// "run to a line" looks for.
    pub fn block_lines(&self, func: u32, n: u32) -> impl Iterator<Item = Loc> + '_ {
        self.block(func, n).into_iter().flat_map(|b| b.steps)
            .flat_map(move |s| core::iter::once(s.loc).chain(self.chain(s.inlined).iter().map(|&(_, site)| site)))
    }

    /// Inline chain `k` (see inlines), or none.
    pub fn chain(&self, k: Option<u32>) -> &[(u32, Loc)] {
        k.and_then(|k| self.inlines.get(k as usize)).map_or(&[], Vec::as_slice)
    }

    /// Stretch `seg` of block `n` of `func`'s lines (none past the last).
    pub fn stretch(&self, func: u32, n: u32, seg: u32) -> &[Step] {
        self.block(func, n).map_or(&[], |b| b.stretch(seg))
    }

    /// Function `func`'s argument values, from a call's words (trace::Args):
    /// a word each, two (low, high) for a wide one.
    pub fn values(&self, func: u32, words: &[u32]) -> Vec<u64> {
        let wide = self.functions.get(&func).map_or(0, |f| f.wide);
        let (mut out, mut w) = (Vec::new(), words.iter().copied());
        while let Some(lo) = w.next() {
            let hi = if wide >> out.len() & 1 == 1 { w.next().unwrap_or(0) } else { 0 };
            out.push(lo as u64 | (hi as u64) << 32);
        }
        out
    }

    /// Where the blocks of `func` begin, in order.
    pub fn blocks_of(&self, func: u32) -> impl Iterator<Item = Loc> + '_ {
        let from = self.blocks.partition_point(|b| b.key.0 < func);
        self.blocks[from..].iter().take_while(move |b| b.key.0 == func).map(|b| self.view(b).loc())
    }

    pub fn file(&self, loc: Loc) -> &str {
        self.files.get(loc.file as usize).map_or("?", String::as_str)
    }

    /// The id of file `path` (Loc::file; the paths are each once).
    pub(crate) fn file_id(&self, path: &str) -> Option<u32> {
        self.files.iter().position(|f| f == path).map(|k| k as u32)
    }

    /// The id of the (first) instrumented function called `name`.
    pub fn find(&self, name: &str) -> Option<u32> {
        self.functions.iter().find(|(_, f)| f.name == name).map(|(&id, _)| id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_code_and_types() {
        let code = "U\nP\t0\t/s/p.c\nP\t1\t/s/q.h\nF\t7\tpick\t0:2\t3\t0\nL\t7\t0\t1:2\nU\nP\t0\t/s/p.c\nF\t7\tpick\t0:2\t3\t0\nF\t8\tcrc\t0:9\t1\t1\n";
        let t = Tables::parse(code, "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ttask_struct\t740\nM\t2\tpid\t5888\t0\t1\n");
        let f = &t.functions[&7];
        assert_eq!((f.known, f.sealed, t.functions[&8].sealed), (3, false, true));
        assert_eq!((t.file(f.loc), f.loc.line), ("/s/p.c", 2));
        let b = t.block(7, 0).unwrap().loc();
        assert_eq!((t.file(b), b.line), ("/s/q.h", 2));
        assert_eq!(t.types.field("task_struct", "pid"), Some((736, 4)));
    }

    #[test]
    fn the_machines_primitives_fold_into_their_lines() {
        // f's block 0 runs line 3 of get_current (arch/wasm's, inlined into
        // spin_lock at its line 9, inlined into f at line 2), then line 4 of
        // f: the get_current step is spin_lock's line 9.
        let code = "U\nP\t0\tf.c\nP\t1\tinclude/linux/spinlock.h\nP\t2\tarch/wasm/include/asm/current.h\n\
                      I\t0\tget_current\t1:9\nI\t1\tspin_lock\t0:2\n\
                      F\t1\tf\t0:1\t0\nL\t1\t0\t2:3@0@1;0:4\n";
        let t = Tables::parse(code, "W\tget_current\tarch/wasm/include/asm/current.h\t12\nW\tspin_lock\tinclude/linux/spinlock.h\t7\n");
        let steps = t.block(1, 0).unwrap().steps;
        let chain = |s: &Step| s.inlined.map(|k| t.inlines[k as usize].iter().map(|&(n, _)| t.inline_names[n as usize].clone()).collect::<Vec<_>>());
        assert_eq!((t.file(steps[0].loc), steps[0].loc.line, chain(&steps[0])), ("include/linux/spinlock.h", 9, Some(alloc::vec!["spin_lock".into()])));
        assert_eq!((steps[1].loc.line, steps[1].inlined), (4, None));
        // The block's own location was get_current's: now its first line's.
        let b = t.block(1, 0).unwrap().loc();
        assert_eq!((t.file(b), b.line), ("include/linux/spinlock.h", 9));
    }

    #[test]
    fn wide_arguments_take_two_words() {
        // f(int a, loff_t b, int c): b is wide (bit 1).
        let t = Tables::parse("F\t7\tf\t0:1\t7\t0\t2\n", "");
        assert_eq!(t.values(7, &[1, 2, 3, 4]), [1, 3 << 32 | 2, 4]);
    }
}
