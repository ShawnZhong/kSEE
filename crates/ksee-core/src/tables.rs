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
    /// Parse "path:line" or, with `column`, "path:line:column".
    fn loc(&mut self, s: &str, column: bool) -> Loc {
        let (rest, col) = match column {
            true => s.rsplit_once(':').unwrap_or((s, "0")),
            false => (s, "0"),
        };
        let (path, line) = rest.rsplit_once(':').unwrap_or((rest, "0"));
        let file = match self.ids.get(path) {
            Some(&id) => id,
            None => {
                self.paths.push(path.to_string());
                let id = self.paths.len() as u32 - 1;
                self.ids.insert(path.to_string(), id);
                id
            }
        };
        Loc { file, line: line.parse().unwrap_or(0), column: col.parse().unwrap_or(0) }
    }
}

impl Tables {
    /// Parse the `ksee.probes` and `ksee.types` sections. Lines are
    /// tab-separated and may repeat (one copy per compilation unit).
    pub fn parse(probes: &str, types: &str) -> Tables {
        let mut t = Tables::default();
        let mut files = Files::default();
        for line in probes.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            match cols.as_slice() {
                ["R", words] => t.ring_words = words.parse().unwrap_or(0),
                ["F", id, name, location, rest @ ..] => {
                    let Ok(id) = id.parse() else { continue };
                    let arg_types = match rest.first() {
                        Some(types) if !types.is_empty() => {
                            types.split(',').map(String::from).collect()
                        }
                        _ => Vec::new(),
                    };
                    let loc = files.loc(location, false);
                    t.functions.insert(id, Function { name: name.to_string(), loc, arg_types });
                }
                ["B", id, n, where_] => {
                    if let (Ok(id), Ok(n)) = (id.parse(), n.parse()) {
                        t.blocks.insert((id, n), files.loc(where_, true));
                    }
                }
                _ => {}
            }
        }
        t.files = files.paths;
        t.types = Types::parse(types);
        t
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
        let probes = "R\t64\nF\t7\tpick\t/s/p.c:2\ttask*,int\nB\t7\t0\t/s/q.h:2:39\nF\t7\tpick\t/s/p.c:2\ttask*,int\n";
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
