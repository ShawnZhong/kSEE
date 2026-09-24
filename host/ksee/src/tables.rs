//! The probe and type tables the trace plugin embeds in the kernel module.

use std::collections::HashMap;

/// An instrumented function.
#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    /// `file:line` of the definition.
    pub location: String,
    /// Types of the first four parameters, e.g. `task_struct*`.
    pub arg_types: Vec<String>,
}

/// Byte offset and size of a struct field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Default)]
pub struct Tables {
    pub functions: HashMap<u32, Function>,
    /// (function, block) -> `line:column`.
    pub blocks: HashMap<(u32, u32), String>,
    /// (struct, field) -> layout.
    pub fields: HashMap<(String, String), Field>,
}

impl Tables {
    /// Parse the `ksee.probes` and `ksee.types` sections. Lines are
    /// tab-separated and may repeat (one copy per compilation unit).
    pub fn parse(probes: &str, types: &str) -> Tables {
        let mut t = Tables::default();
        for line in probes.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            match cols.as_slice() {
                ["F", id, name, location, rest @ ..] => {
                    let Ok(id) = id.parse() else { continue };
                    let arg_types = match rest.first() {
                        Some(types) if !types.is_empty() => {
                            types.split(',').map(String::from).collect()
                        }
                        _ => Vec::new(),
                    };
                    t.functions.insert(id, Function {
                        name: name.to_string(),
                        location: location.to_string(),
                        arg_types,
                    });
                }
                ["B", id, n, loc] => {
                    if let (Ok(id), Ok(n)) = (id.parse(), n.parse()) {
                        t.blocks.insert((id, n), loc.to_string());
                    }
                }
                _ => {}
            }
        }
        for line in types.lines() {
            if let [s, f, offset, size] = line.split('\t').collect::<Vec<_>>()[..] {
                if let (Ok(offset), Ok(size)) = (offset.parse(), size.parse()) {
                    t.fields.insert((s.into(), f.into()), Field { offset, size });
                }
            }
        }
        t
    }

    pub fn field(&self, s: &str, f: &str) -> Option<Field> {
        self.fields.get(&(s.to_string(), f.to_string())).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_probes_and_types() {
        let probes = "F\t7\tpick\tp.c:2\ttask*,int\nB\t7\t0\t2:39\nF\t7\tpick\tp.c:2\ttask*,int\n";
        let types = "task_struct\tpid\t736\t4\n";
        let t = Tables::parse(probes, types);
        assert_eq!(t.functions[&7].arg_types, ["task*", "int"]);
        assert_eq!(t.blocks[&(7, 0)], "2:39");
        assert_eq!(t.field("task_struct", "pid"), Some(Field { offset: 736, size: 4 }));
    }
}
