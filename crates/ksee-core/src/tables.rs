//! The probe and type tables the trace plugin embeds in the kernel module.

use alloc::collections::BTreeMap;

use crate::arch;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// A source location; `file` indexes [`Tables::files`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Loc {
    pub file: u32,
    pub line: u32,
    pub column: u32,
}

/// An instrumented function.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    /// Where it is defined (column 0).
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
/// `param` of it (from 1; 0: a local).
#[derive(Debug, Clone, PartialEq)]
pub struct Var {
    pub of: u32,
    pub function: String,
    pub name: String,
    pub param: u32,
}

/// A source line a block runs; `inlined`: if it is code inlined into the
/// block's function, the functions it is inlined from (an index into
/// [`Tables::inlines`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub loc: Loc,
    pub inlined: Option<u32>,
}

/// Byte offset and size of a struct field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    pub offset: u32,
    pub size: u32,
}

use crate::types::Types;

#[derive(Debug, Default)]
pub struct Tables {
    /// Size of the trace ring in words.
    pub ring_words: u32,
    pub functions: BTreeMap<u32, Function>,
    /// (function, block) -> where the block's first instruction is.
    pub blocks: BTreeMap<(u32, u32), Loc>,
    /// (function, block) -> the lines it runs, in stretches between the
    /// calls it makes (only for blocks with more than their first line).
    pub lines: BTreeMap<(u32, u32), Vec<Vec<Step>>>,
    /// Names of inlined functions.
    pub names: Vec<String>,
    /// Inline chains (see [`Step`]): innermost first, each an inlined
    /// function (in [`Tables::names`]) and where it is inlined (a line of
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

/// A unit's files: its numbers (P lines) -> file ids.
/// A compilation unit's own numbering: its files (P lines) and its inline
/// sites (I lines: the function inlined, and where).
#[derive(Default)]
struct Unit {
    files: BTreeMap<u32, u32>,
    sites: BTreeMap<u32, (u32, Loc)>,
}

impl Unit {
    /// Parse "k:line" or, with `column`, "k:line:column" (k: a P line's).
    fn loc(&self, s: &str, column: bool) -> Loc {
        let mut parts = s.split(':');
        let file = parts.next().and_then(|k| k.parse().ok()).and_then(|k| self.files.get(&k).copied()).unwrap_or(u32::MAX);
        let line = parts.next().and_then(|l| l.parse().ok()).unwrap_or(0);
        let column = if column { parts.next().and_then(|c| c.parse().ok()).unwrap_or(0) } else { 0 };
        Loc { file, line, column }
    }
}

impl Tables {
    /// Parse the `ksee.probes` and `ksee.types` sections. Lines are
    /// tab-separated and may repeat (one copy per compilation unit).
    pub fn parse(probes: &str, types: &str) -> Tables {
        let mut t = Tables::default();
        let mut files = Files::default();
        let mut unit = Unit::default();
        let mut names: BTreeMap<String, u32> = BTreeMap::new();
        let mut chains: BTreeMap<Vec<(u32, Loc)>, u32> = BTreeMap::new();
        for line in probes.lines() {
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
                            t.names.push(name.to_string());
                            t.names.len() as u32 - 1
                        });
                        unit.sites.insert(k, (name, unit.loc(site, false)));
                    }
                }
                ["V", id, k, of, function, name, param, _wide] => {
                    if let (Ok(id), Ok(k), Ok(of)) = (id.parse(), k.parse(), of.parse()) {
                        let param = param.parse().unwrap_or(0);
                        t.vars.insert((id, k), Var { of, function: function.to_string(), name: name.to_string(), param });
                    }
                }
                ["R", words] => t.ring_words = words.parse().unwrap_or(0),
                ["F", id, name, location, known, rest @ ..] => {
                    let Ok(id) = id.parse() else { continue };
                    let loc = unit.loc(location, false);
                    let sealed = rest.first() == Some(&"1");
                    let wide = rest.get(1).and_then(|w| w.parse().ok()).unwrap_or(0);
                    t.functions.insert(id, Function { name: name.to_string(), loc, known: known.parse().unwrap_or(0), wide, sealed });
                }
                ["B", id, n, where_] => {
                    if let (Ok(id), Ok(n)) = (id.parse(), n.parse()) {
                        t.blocks.insert((id, n), unit.loc(where_, true));
                    }
                }
                ["L", id, n, items] => {
                    if let (Ok(id), Ok(n)) = (id.parse(), n.parse()) {
                        let mut segs = alloc::vec![Vec::new()];
                        let mut file = ""; // ":line": the item before's
                        for item in items.split(';') {
                            if item == "|" {
                                segs.push(Vec::new());
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
                            segs.last_mut().unwrap().push(Step { loc: unit.loc(at, false), inlined });
                        }
                        t.lines.insert((id, n), segs);
                    }
                }
                _ => {}
            }
        }
        t.files = files.paths;
        t.types = Types::parse(types);
        t.fold_primitives();
        t
    }

    /// The machine's primitives (arch::PRIMITIVES) inlined into kernel code
    /// are the machine, not calls: a step in one becomes a step at the line
    /// it is inlined at, in the function that inlined it, so that nothing
    /// shows a frame, a node or a line of get_current.
    fn fold_primitives(&mut self) {
        let prim: Vec<bool> = self.names.iter()
            .map(|n| self.types.declared.get(n).is_some_and(|(f, _)| arch::PRIMITIVES.iter().any(|p| f.starts_with(p)))).collect();
        if !prim.iter().any(|&p| p) {
            return;
        }
        let mut chains: BTreeMap<Vec<(u32, Loc)>, u32> = self.inlines.iter().enumerate().map(|(i, c)| (c.clone(), i as u32)).collect();
        let mut folded: BTreeMap<u32, (Loc, Option<u32>)> = BTreeMap::new();
        for segs in self.lines.values_mut() {
            for s in segs.iter_mut().flatten() {
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
        // (A block whose first instruction is a primitive's: its first line's.)
        let primitive_file = |t: &Tables, loc: Loc| arch::PRIMITIVES.iter().any(|p| t.file(loc).starts_with(p));
        let firsts: Vec<((u32, u32), Loc)> = self.blocks.iter().filter(|&(_, &l)| primitive_file(self, l))
            .filter_map(|(&k, _)| self.lines.get(&k).and_then(|segs| segs.first()).and_then(|seg| seg.first()).map(|s| (k, s.loc))).collect();
        for (k, loc) in firsts {
            self.blocks.insert(k, loc);
        }
    }

    /// The lines block `n` of `func` runs, in stretches between its calls
    /// (just its first line, if the plugin said no more).
    pub fn segments(&self, func: u32, n: u32) -> Vec<Vec<Step>> {
        match self.lines.get(&(func, n)) {
            Some(s) => s.clone(),
            None => alloc::vec![self.blocks.get(&(func, n)).map(|&loc| Step { loc, inlined: None }).into_iter().collect()],
        }
    }

    /// Every line block `n` of `func` runs: its lines' own, and the lines
    /// the code of each is inlined at (`x = helper();`, helper inlined, runs
    /// no code of its own line's: the helper's lines run in its place; the
    /// debug info's inline sites say where). What coverage counts, and what
    /// "run to a line" looks for.
    pub fn runs(&self, func: u32, n: u32) -> impl Iterator<Item = Loc> + '_ {
        let own = match self.lines.get(&(func, n)) {
            Some(segs) => segs.iter().flatten().copied().collect(),
            None => self.blocks.get(&(func, n)).map(|&loc| Step { loc, inlined: None }).into_iter().collect::<Vec<_>>(),
        };
        own.into_iter().flat_map(move |s| {
            let sites = s.inlined.and_then(|k| self.inlines.get(k as usize)).map_or(&[][..], Vec::as_slice);
            core::iter::once(s.loc).chain(sites.iter().map(|&(_, site)| site))
        })
    }

    /// Stretch `seg` of those lines (none past the last).
    pub fn stretch(&self, func: u32, n: u32, seg: u32) -> alloc::borrow::Cow<'_, [Step]> {
        use alloc::borrow::Cow;
        match self.lines.get(&(func, n)) {
            Some(s) => Cow::Borrowed(s.get(seg as usize).map_or(&[][..], Vec::as_slice)),
            None if seg == 0 => Cow::Owned(self.blocks.get(&(func, n)).map(|&loc| Step { loc, inlined: None }).into_iter().collect()),
            None => Cow::Borrowed(&[]),
        }
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

    /// The blocks of `func`, in order.
    pub fn blocks_of(&self, func: u32) -> impl Iterator<Item = (u32, Loc)> + '_ {
        self.blocks.range((func, 0)..=(func, u32::MAX)).map(|(&(_, n), &l)| (n, l))
    }

    pub fn file(&self, loc: Loc) -> &str {
        self.files.get(loc.file as usize).map_or("?", String::as_str)
    }

    pub fn field(&self, s: &str, f: &str) -> Option<Field> {
        let (offset, size) = self.types.field(s, f)?;
        Some(Field { offset, size })
    }

    /// The id of the (first) instrumented function called `name`.
    pub fn find(&self, name: &str) -> Option<u32> {
        self.functions.iter().find(|(_, f)| f.name == name).map(|(&id, _)| id)
    }

    /// The plugin's FNV-1a hash (function ids hash the name, prefixed with
    /// the source file for functions local to a file).
    pub fn id(name: &str) -> u32 {
        name.bytes().fold(2166136261u32, |h, c| (h ^ c as u32).wrapping_mul(16777619))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_probes_and_types() {
        let probes = "U\nP\t0\t/s/p.c\nP\t1\t/s/q.h\nR\t64\nF\t7\tpick\t0:2\t3\t0\nB\t7\t0\t1:2:39\nU\nP\t0\t/s/p.c\nF\t7\tpick\t0:2\t3\t0\nF\t8\tcrc\t0:9\t1\t1\n";
        let t = Tables::parse(probes, "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ttask_struct\t740\nM\t2\tpid\t5888\t0\t1\n");
        assert_eq!(t.ring_words, 64);
        let f = &t.functions[&7];
        assert_eq!((f.known, f.sealed, t.functions[&8].sealed), (3, false, true));
        assert_eq!((t.file(f.loc), f.loc.line), ("/s/p.c", 2));
        let b = t.blocks[&(7, 0)];
        assert_eq!((t.file(b), b.line, b.column), ("/s/q.h", 2, 39));
        assert_eq!(t.field("task_struct", "pid"), Some(Field { offset: 736, size: 4 }));
    }

    #[test]
    fn the_machines_primitives_fold_into_their_lines() {
        // f's block 0 runs line 3 of get_current (arch/wasm's, inlined into
        // spin_lock at its line 9, inlined into f at line 2), then line 4 of
        // f: the get_current step is spin_lock's line 9.
        let probes = "U\nP\t0\tf.c\nP\t1\tinclude/linux/spinlock.h\nP\t2\tarch/wasm/include/asm/current.h\n\
                      I\t0\tget_current\t1:9\nI\t1\tspin_lock\t0:2\n\
                      F\t1\tf\t0:1\t0\nB\t1\t0\t2:3:1\nL\t1\t0\t2:3@0@1;0:4\n";
        let t = Tables::parse(probes, "W\tget_current\tarch/wasm/include/asm/current.h\t12\nW\tspin_lock\tinclude/linux/spinlock.h\t7\n");
        let steps = &t.lines[&(1, 0)][0];
        let chain = |s: &Step| s.inlined.map(|k| t.inlines[k as usize].iter().map(|&(n, _)| t.names[n as usize].clone()).collect::<Vec<_>>());
        assert_eq!((t.file(steps[0].loc), steps[0].loc.line, chain(&steps[0])), ("include/linux/spinlock.h", 9, Some(alloc::vec!["spin_lock".into()])));
        assert_eq!((steps[1].loc.line, steps[1].inlined), (4, None));
        // The block's own location was get_current's: now its first line's.
        let b = t.blocks[&(1, 0)];
        assert_eq!((t.file(b), b.line), ("include/linux/spinlock.h", 9));
    }

    #[test]
    fn wide_arguments_take_two_words() {
        // f(int a, loff_t b, int c): b is wide (bit 1).
        let t = Tables::parse("F\t7\tf\t0:1\t7\t0\t2\n", "");
        assert_eq!(t.values(7, &[1, 2, 3, 4]), [1, 3 << 32 | 2, 4]);
    }

    #[test]
    fn id_matches_the_plugin() {
        // From the kernel's ksee.probes section.
        assert_eq!(Tables::id("try_to_wake_up"), 2218239081);
    }
}
