//! The probe and type tables the trace plugin embeds in the kernel module.

use alloc::collections::{BTreeMap, BTreeSet};

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
    /// Only its call and returns are recorded, not its blocks (it loops,
    /// writes nothing the host undoes, calls nothing: see the plugin).
    pub sealed: bool,
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
    /// Where the syscall wrappers are defined (file, line): code inlined
    /// there is SYSCALL_DEFINE's shims, the entry's own (see is_wrapper).
    pub wrapper_lines: BTreeSet<(u32, u32)>,
    /// Source file paths.
    pub files: Vec<String>,
    /// The kernel's types.
    pub types: Types,
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
#[derive(Default)]
struct Unit(BTreeMap<u32, u32>);

impl Unit {
    /// Parse "k:line" or, with `column`, "k:line:column" (k: a P line's).
    fn loc(&self, s: &str, column: bool) -> Loc {
        let mut parts = s.split(':');
        let file = parts.next().and_then(|k| k.parse().ok()).and_then(|k| self.0.get(&k).copied()).unwrap_or(u32::MAX);
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
                        unit.0.insert(k, files.intern(path));
                    }
                }
                ["R", words] => t.ring_words = words.parse().unwrap_or(0),
                ["F", id, name, location, known, rest @ ..] => {
                    let Ok(id) = id.parse() else { continue };
                    let loc = unit.loc(location, false);
                    let sealed = rest.first() == Some(&"1");
                    if name.starts_with(arch::SYSCALL_WRAPPER) {
                        t.wrapper_lines.insert((loc.file, loc.line));
                    }
                    t.functions.insert(id, Function { name: name.to_string(), loc, known: known.parse().unwrap_or(0), sealed });
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
                            // The functions it is inlined from: name^FILE:line each.
                            let chain: Vec<(u32, Loc)> = inl.into_iter().flat_map(|c| c.split('@')).map(|level| {
                                let (name, site) = level.split_once('^').unwrap_or((level, ""));
                                let k = *names.entry(name.to_string()).or_insert_with(|| {
                                    t.names.push(name.to_string());
                                    t.names.len() as u32 - 1
                                });
                                (k, unit.loc(site, false))
                            }).collect();
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
    }

    /// The lines block `n` of `func` runs, in stretches between its calls
    /// (just its first line, if the plugin said no more).
    pub fn segments(&self, func: u32, n: u32) -> Vec<Vec<Step>> {
        match self.lines.get(&(func, n)) {
            Some(s) => s.clone(),
            None => alloc::vec![self.blocks.get(&(func, n)).map(|&loc| Step { loc, inlined: None }).into_iter().collect()],
        }
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

    /// Whether `func` is a syscall's table entry (arch::SYSCALL_WRAPPER):
    /// the syscall's entry frame, with what is inlined at its line.
    pub fn is_wrapper(&self, func: u32) -> bool {
        self.functions.get(&func).is_some_and(|f| f.name.starts_with(arch::SYSCALL_WRAPPER))
    }

    /// Whether code inlined at `site` is a wrapper's shim (inlined at the
    /// wrapper's own line: SYSCALL_DEFINE's).
    pub fn at_wrapper_line(&self, site: Loc) -> bool {
        self.wrapper_lines.contains(&(site.file, site.line))
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
                      F\t1\tf\t0:1\t0\nB\t1\t0\t0:1:1\nL\t1\t0\t2:3@get_current^1:9@spin_lock^0:2;0:4\n";
        let t = Tables::parse(probes, "W\tget_current\tarch/wasm/include/asm/current.h\t12\nW\tspin_lock\tinclude/linux/spinlock.h\t7\n");
        let steps = &t.lines[&(1, 0)][0];
        let chain = |s: &Step| s.inlined.map(|k| t.inlines[k as usize].iter().map(|&(n, _)| t.names[n as usize].clone()).collect::<Vec<_>>());
        assert_eq!((t.file(steps[0].loc), steps[0].loc.line, chain(&steps[0])), ("include/linux/spinlock.h", 9, Some(alloc::vec!["spin_lock".into()])));
        assert_eq!((steps[1].loc.line, steps[1].inlined), (4, None));
    }

    #[test]
    fn id_matches_the_plugin() {
        // From the kernel's ksee.probes section.
        assert_eq!(Tables::id("try_to_wake_up"), 2218239081);
    }
}
