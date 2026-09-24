//! `ksee btf`: the kernel's types from DWARF, deduplicated across
//! compilation units (like pahole's BTF), as a ksee.types section (see
//! ksee_core::types), in a copy of the module without DWARF.

use std::collections::HashMap;
use std::fmt::Write;

use gimli::{AttributeValue, DwAt, DwTag, EndianSlice, LittleEndian, UnitOffset, constants as dw};
use ksee_core::module;

type R<'a> = EndianSlice<'a, LittleEndian>;

/// The type table as ksee.types text.
pub fn types(image: &[u8]) -> Result<String, gimli::Error> {
    let sections: HashMap<String, Vec<u8>> = module::custom_sections(image)
        .filter(|(name, _)| name.starts_with(".debug_"))
        .map(|(name, data)| (name.to_string(), data.to_vec()))
        .collect();
    let dwarf = gimli::Dwarf::load(|id| -> Result<R, gimli::Error> {
        let data = sections.get(id.name()).map_or(&[][..], Vec::as_slice);
        Ok(EndianSlice::new(data, LittleEndian))
    })?;
    let mut b = Builder::default();
    let mut units = dwarf.units();
    while let Some(header) = units.next()? {
        let unit = dwarf.unit(header)?;
        let mut cu = Cu { dwarf: &dwarf, unit: &unit, memo: HashMap::new(), files: HashMap::new() };
        // Every type in the unit, so that unused ones are listed too.
        let mut entries = unit.entries();
        let mut offsets = Vec::new();
        while let Some(e) = entries.next_dfs()? {
            if matches!(e.tag(), dw::DW_TAG_structure_type | dw::DW_TAG_union_type
                        | dw::DW_TAG_enumeration_type | dw::DW_TAG_typedef) {
                offsets.push(e.offset());
            }
        }
        for off in offsets {
            b.canon(&mut cu, off)?;
        }
    }
    Ok(b.text())
}

struct Cu<'a, 'b> {
    dwarf: &'a gimli::Dwarf<R<'b>>,
    unit: &'a gimli::Unit<R<'b>>,
    /// Unit offset -> type id.
    memo: HashMap<UnitOffset, u32>,
    files: HashMap<u64, String>,
}

#[derive(Default)]
struct Builder {
    /// Rows of the type table, by id (0 is void).
    rows: Vec<String>,
    extra: Vec<String>, // M and V lines
    ids: HashMap<String, u32>,
    /// (tag, name) -> the id of the first complete definition, or a
    /// placeholder for it reserved by a declaration.
    by_name: HashMap<(DwTag, String), (u32, bool)>,
}

impl Builder {
    fn new_id(&mut self) -> u32 {
        if self.rows.is_empty() {
            self.rows.push(String::new()); // 0: void
        }
        self.rows.push(String::new());
        self.rows.len() as u32 - 1
    }

    fn intern(&mut self, key: String, row: impl FnOnce(u32) -> String) -> u32 {
        if let Some(&id) = self.ids.get(&key) {
            return id;
        }
        let id = self.new_id();
        self.rows[id as usize] = row(id);
        self.ids.insert(key, id);
        id
    }

    fn text(&self) -> String {
        let mut out = String::new();
        for row in self.rows.iter().skip(1) {
            out.push_str(row);
            out.push('\n');
        }
        for line in &self.extra {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    fn canon(&mut self, cu: &mut Cu, off: UnitOffset) -> Result<u32, gimli::Error> {
        if let Some(&id) = cu.memo.get(&off) {
            return Ok(id);
        }
        let e = cu.unit.entry(off)?;
        let name = attr_string(cu, &e, dw::DW_AT_name)?;
        let size = attr_u64(&e, dw::DW_AT_byte_size).unwrap_or(0) as u32;
        let target = |b: &mut Builder, cu: &mut Cu| -> Result<u32, gimli::Error> {
            match e.attr_value(dw::DW_AT_type) {
                Some(AttributeValue::UnitRef(t)) => b.canon(cu, t),
                _ => Ok(0),
            }
        };
        let id = match e.tag() {
            dw::DW_TAG_base_type => {
                let enc = match e.attr_value(dw::DW_AT_encoding) {
                    Some(AttributeValue::Encoding(enc)) => Some(enc),
                    _ => None,
                };
                let enc = match enc {
                    Some(dw::DW_ATE_signed) => "s",
                    Some(dw::DW_ATE_float) => "f",
                    Some(dw::DW_ATE_boolean) => "b",
                    Some(dw::DW_ATE_signed_char | dw::DW_ATE_unsigned_char) => "c",
                    _ => "u",
                };
                self.intern(format!("base {name} {size} {enc}"), |id| format!("T\t{id}\tbase\t{name}\t{size}\t{enc}"))
            }
            dw::DW_TAG_const_type | dw::DW_TAG_volatile_type | dw::DW_TAG_restrict_type
            | dw::DW_TAG_atomic_type => target(self, cu)?,
            dw::DW_TAG_pointer_type => {
                // Memoize before recursing: pointers close cycles.
                let t = target(self, cu)?;
                self.intern(format!("ptr {t}"), |id| format!("T\t{id}\tptr\t\t4\t{t}"))
            }
            dw::DW_TAG_typedef => {
                let t = target(self, cu)?;
                let size = self.size(t);
                self.intern(format!("typedef {name} {t}"), |id| format!("T\t{id}\ttypedef\t{name}\t{size}\t{t}"))
            }
            dw::DW_TAG_subroutine_type => self.intern("func".into(), |id| format!("T\t{id}\tfunc\t\t0")),
            dw::DW_TAG_array_type => {
                let elem = target(self, cu)?;
                let mut dims = Vec::new();
                let mut tree = cu.unit.entries_tree(Some(off))?;
                let mut children = tree.root()?.children();
                while let Some(child) = children.next()? {
                    let c = child.entry();
                    if c.tag() == dw::DW_TAG_subrange_type {
                        let n = attr_u64(c, dw::DW_AT_count)
                            .or_else(|| attr_u64(c, dw::DW_AT_upper_bound).map(|u| u + 1))
                            .unwrap_or(0);
                        dims.push(n as u32);
                    }
                }
                let mut id = elem;
                for &n in dims.iter().rev() {
                    let esize = self.size(id);
                    let size = n * esize;
                    id = self.intern(format!("array {id} {n}"), |aid| format!("T\t{aid}\tarray\t\t{size}\t{id}\t{n}"));
                }
                id
            }
            tag @ (dw::DW_TAG_structure_type | dw::DW_TAG_union_type | dw::DW_TAG_enumeration_type) => {
                let kind = match tag {
                    dw::DW_TAG_structure_type => "struct",
                    dw::DW_TAG_union_type => "union",
                    _ => "enum",
                };
                let decl = matches!(e.attr_value(dw::DW_AT_declaration), Some(AttributeValue::Flag(true)));
                let named = (tag, name.clone());
                if decl {
                    // Stands for the definition, wherever it is.
                    if let Some(&(id, _)) = self.by_name.get(&named) {
                        id
                    } else {
                        let id = self.new_id();
                        self.rows[id as usize] = format!("T\t{id}\t{kind}\t{name}\t0");
                        self.by_name.insert(named, (id, false));
                        id
                    }
                } else {
                    let file = decl_file(cu, &e)?;
                    let line = attr_u64(&e, dw::DW_AT_decl_line).unwrap_or(0);
                    let key = format!("{kind} {name} {size} {file}:{line}");
                    if let Some(&id) = self.ids.get(&key) {
                        cu.memo.insert(off, id);
                        return Ok(id);
                    }
                    let id = match self.by_name.get(&named) {
                        Some(&(id, false)) if !name.is_empty() => id, // fill the placeholder
                        _ => self.new_id(),
                    };
                    if !name.is_empty() {
                        self.by_name.entry(named.clone()).or_insert((id, true)).1 = true;
                        if let Some(v) = self.by_name.get_mut(&named) {
                            if v.0 == id {
                                v.1 = true;
                            }
                        }
                    }
                    self.ids.insert(key, id);
                    self.rows[id as usize] = format!("T\t{id}\t{kind}\t{name}\t{size}");
                    cu.memo.insert(off, id);
                    self.members(cu, off, id, tag)?;
                    id
                }
            }
            _ => 0,
        };
        cu.memo.insert(off, id);
        Ok(id)
    }

    fn members(&mut self, cu: &mut Cu, off: UnitOffset, id: u32, tag: DwTag) -> Result<(), gimli::Error> {
        let mut rows = Vec::new();
        {
            let mut tree = cu.unit.entries_tree(Some(off))?;
            let mut children = tree.root()?.children();
            while let Some(child) = children.next()? {
                let c = child.entry();
                let name = attr_string(cu, c, dw::DW_AT_name)?;
                match c.tag() {
                    dw::DW_TAG_member => {
                        let bit_offset = attr_u64(c, dw::DW_AT_data_bit_offset)
                            .or_else(|| attr_u64(c, dw::DW_AT_data_member_location).map(|b| b * 8))
                            .unwrap_or(0);
                        let bit_size = attr_u64(c, dw::DW_AT_bit_size).unwrap_or(0);
                        let ty = match c.attr_value(dw::DW_AT_type) {
                            Some(AttributeValue::UnitRef(t)) => Some(t),
                            _ => None,
                        };
                        rows.push((false, name, bit_offset, bit_size, ty, 0));
                    }
                    dw::DW_TAG_enumerator if tag == dw::DW_TAG_enumeration_type => {
                        let v = match c.attr_value(dw::DW_AT_const_value) {
                            Some(AttributeValue::Sdata(v)) => v,
                            Some(AttributeValue::Udata(v)) => v as i64,
                            Some(v) => v.sdata_value().unwrap_or(0),
                            None => 0,
                        };
                        rows.push((true, name, 0, 0, None, v));
                    }
                    _ => {}
                }
            }
        }
        for (is_enum, name, bit_offset, bit_size, ty, value) in rows {
            if is_enum {
                self.extra.push(format!("V\t{id}\t{name}\t{value}"));
            } else {
                let t = match ty {
                    Some(t) => self.canon(cu, t)?,
                    None => 0,
                };
                self.extra.push(format!("M\t{id}\t{name}\t{bit_offset}\t{bit_size}\t{t}"));
            }
        }
        Ok(())
    }

    /// The size of type `id` (from its row).
    fn size(&self, id: u32) -> u32 {
        self.rows.get(id as usize).and_then(|r| r.split('\t').nth(4)).and_then(|s| s.parse().ok()).unwrap_or(0)
    }
}

fn attr_u64(e: &gimli::DebuggingInformationEntry<R>, at: DwAt) -> Option<u64> {
    e.attr_value(at).and_then(|v| v.udata_value())
}

fn attr_string(cu: &Cu, e: &gimli::DebuggingInformationEntry<R>, at: DwAt) -> Result<String, gimli::Error> {
    Ok(match e.attr_value(at) {
        Some(v) => cu.dwarf.attr_string(cu.unit, v)?.to_string_lossy().into_owned(),
        None => String::new(),
    })
}

fn decl_file(cu: &mut Cu, e: &gimli::DebuggingInformationEntry<R>) -> Result<String, gimli::Error> {
    let Some(AttributeValue::FileIndex(i)) = e.attr_value(dw::DW_AT_decl_file) else {
        return Ok(String::new());
    };
    if let Some(f) = cu.files.get(&i) {
        return Ok(f.clone());
    }
    let mut name = String::new();
    if let Some(lp) = &cu.unit.line_program {
        let header = lp.header();
        if let Some(file) = header.file(i) {
            if let Some(dir) = file.directory(header) {
                let _ = write!(name, "{}/", cu.dwarf.attr_string(cu.unit, dir)?.to_string_lossy());
            }
            let _ = write!(name, "{}", cu.dwarf.attr_string(cu.unit, file.path_name())?.to_string_lossy());
        }
    }
    cu.files.insert(i, name.clone());
    Ok(name)
}

/// `image` without DWARF and with `types` as its ksee.types section.
pub fn rewrite(image: &[u8], types: &str) -> Vec<u8> {
    let mut out = image[..8].to_vec();
    for (id, name, raw) in module::sections(image) {
        let drop = id == 0 && (name.starts_with(".debug_") || name == "ksee.types");
        if !drop {
            out.extend_from_slice(raw);
        }
    }
    let mut payload = Vec::new();
    leb(&mut payload, "ksee.types".len() as u32);
    payload.extend_from_slice(b"ksee.types");
    payload.extend_from_slice(types.as_bytes());
    out.push(0);
    leb(&mut out, payload.len() as u32);
    out.extend_from_slice(&payload);
    out
}

fn leb(out: &mut Vec<u8>, mut v: u32) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        out.push(if v == 0 { b } else { b | 0x80 });
        if v == 0 {
            break;
        }
    }
}
