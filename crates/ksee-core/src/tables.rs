//! The probe and type tables the trace plugin embeds in the kernel module.

use alloc::collections::BTreeMap;
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
    /// Types of the first four parameters, e.g. `task_struct*`.
    pub arg_types: Vec<String>,
}

/// A source line a block runs; `inlined`: the function (an index into
/// [`Tables::names`]) whose code it is, inlined into the block's.
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
    /// Size of the memory ring (the stores' old values) in words.
    pub mem_words: u32,
    pub functions: BTreeMap<u32, Function>,
    /// (function, block) -> where the block's first instruction is.
    pub blocks: BTreeMap<(u32, u32), Loc>,
    /// (function, block) -> the lines it runs, in stretches between the
    /// calls it makes (only for blocks with more than their first line).
    pub lines: BTreeMap<(u32, u32), Vec<Vec<Step>>>,
    /// Names of inlined functions (see [`Step`]).
    pub names: Vec<String>,
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
                ["W", words] => t.mem_words = words.parse().unwrap_or(0),
                ["F", id, name, location, rest @ ..] => {
                    let Ok(id) = id.parse() else { continue };
                    let arg_types = match rest.first() {
                        Some(types) if !types.is_empty() => {
                            types.split(',').map(String::from).collect()
                        }
                        _ => Vec::new(),
                    };
                    let loc = unit.loc(location, false);
                    t.functions.insert(id, Function { name: name.to_string(), loc, arg_types });
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
                            let inlined = inl.map(|f| match names.get(f) {
                                Some(&k) => k,
                                None => {
                                    let k = t.names.len() as u32;
                                    t.names.push(f.to_string());
                                    names.insert(f.to_string(), k);
                                    k
                                }
                            });
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
        t
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
        let probes = "U\nP\t0\t/s/p.c\nP\t1\t/s/q.h\nR\t64\nF\t7\tpick\t0:2\ttask*,int\nB\t7\t0\t1:2:39\nU\nP\t0\t/s/p.c\nF\t7\tpick\t0:2\ttask*,int\n";
        let t = Tables::parse(probes, "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ttask_struct\t740\nM\t2\tpid\t5888\t0\t1\n");
        assert_eq!(t.ring_words, 64);
        let f = &t.functions[&7];
        assert_eq!(f.arg_types, ["task*", "int"]);
        assert_eq!((t.file(f.loc), f.loc.line), ("/s/p.c", 2));
        let b = t.blocks[&(7, 0)];
        assert_eq!((t.file(b), b.line, b.column), ("/s/q.h", 2, 39));
        assert_eq!(t.field("task_struct", "pid"), Some(Field { offset: 736, size: 4 }));
    }

    #[test]
    fn id_matches_the_plugin() {
        // From the kernel's ksee.probes section.
        assert_eq!(Tables::id("try_to_wake_up"), 2218239081);
    }
}
