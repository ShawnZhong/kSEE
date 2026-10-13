//! Reading the kernel's wasm module (wasmparser): its custom sections (its
//! DWARF, the plugin's ksee.code), and what its linker put in memory and in
//! its function table, and its functions' names (the syscall table: see
//! pack/mod.rs).

use std::collections::BTreeMap;

use wasmparser::{BinaryReaderError, ConstExpr, DataKind, ElementItems, ElementKind, KnownCustom, Name, Operator, Parser, Payload};

/// What the pack reads of a module, in place.
#[derive(Default)]
pub struct Module<'a> {
    /// The custom sections, by name.
    pub custom: BTreeMap<&'a str, &'a [u8]>,
    /// The active data segments: (address, bytes).
    data: Vec<(u32, &'a [u8])>,
    /// The function table as the active element segments set it: table
    /// index -> function index.
    pub table: BTreeMap<u32, u32>,
    /// The functions' names (the name section's), by index.
    pub names: BTreeMap<u32, &'a str>,
}

impl<'a> Module<'a> {
    pub fn read(bytes: &'a [u8]) -> Result<Module<'a>, BinaryReaderError> {
        let mut m = Module::default();
        for payload in Parser::new(0).parse_all(bytes) {
            match payload? {
                Payload::DataSection(r) => for d in r {
                    let d = d?;
                    if let DataKind::Active { offset_expr, .. } = d.kind {
                        m.data.push((constant(offset_expr)?, d.data));
                    }
                },
                Payload::ElementSection(r) => for e in r {
                    let e = e?;
                    if let (ElementKind::Active { offset_expr, .. }, ElementItems::Functions(fs)) = (e.kind, e.items) {
                        let at = constant(offset_expr)?;
                        for (k, f) in fs.into_iter().enumerate() {
                            m.table.insert(at + k as u32, f?);
                        }
                    }
                },
                Payload::CustomSection(c) => {
                    if let KnownCustom::Name(r) = c.as_known() {
                        for n in r {
                            if let Name::Function(map) = n? {
                                for n in map {
                                    let n = n?;
                                    m.names.insert(n.index, n.name);
                                }
                            }
                        }
                    }
                    m.custom.insert(c.name(), c.data());
                }
                _ => {}
            }
        }
        Ok(m)
    }

    /// `len` bytes of memory at `addr` as the active data segments set it
    /// (zero where none does).
    pub fn memory(&self, addr: u32, len: u32) -> Vec<u8> {
        let mut out = vec![0u8; len as usize];
        for &(at, bytes) in &self.data {
            for (k, &byte) in bytes.iter().enumerate() {
                if let Some(i) = (at as usize + k).checked_sub(addr as usize).filter(|&i| i < out.len()) {
                    out[i] = byte;
                }
            }
        }
        out
    }
}

/// A segment's offset: a linker's, an i32.const.
fn constant(e: ConstExpr) -> Result<u32, BinaryReaderError> {
    Ok(match e.get_operators_reader().read()? {
        Operator::I32Const { value } => value as u32,
        _ => 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modules_memory_table_and_names() {
        // Table 1.. = functions 0, 1; memory 8.. = 1, 0, 0, 0; function 0 is "read".
        let m = wat::parse_str(r#"(module
            (func $read) (func $write)
            (table 4 funcref) (elem (i32.const 1) $read $write)
            (memory 1) (data (i32.const 8) "\01\00\00\00")
            (@custom "ksee.code" "F"))"#).unwrap();
        let m = Module::read(&m).unwrap();
        assert_eq!(m.memory(6, 6), [0, 0, 1, 0, 0, 0]);
        assert_eq!(m.table.clone().into_iter().collect::<Vec<_>>(), [(1, 0), (2, 1)]);
        assert_eq!(m.names.get(&0), Some(&"read"));
        assert_eq!(m.custom.get("ksee.code"), Some(&&b"F"[..]));
    }
}
