//! The kernel's types, read from DWARF (types deduplicated across
//! compilation units, as pahole does for BTF): the type table as
//! ksee.types text (see ksee_core::types).

use std::collections::HashMap;
use std::fmt::Write;

use gimli::{AttributeValue, DwAt, DwTag, EndianSlice, LittleEndian, UnitOffset, constants as dw};

type R<'a> = EndianSlice<'a, LittleEndian>;

/// The type table as ksee.types text: types, globals, every function's
/// parameters (`P name params`, as "name:type": the call stack's
/// arguments). (The syscalls' names, `S nr name`, come after: see
/// syscalls.)
pub fn types(image: &[u8]) -> Result<String, gimli::Error> {
    let sections: HashMap<String, Vec<u8>> = ksee_core::wasm::custom_sections(&image, 0, image.len() as u32).into_iter()
        .filter(|(name, _, _)| name.starts_with(".debug_"))
        .map(|(name, at, n)| (name, image[at as usize..(at + n) as usize].to_vec()))
        .collect();
    let dwarf = gimli::Dwarf::load(|id| -> Result<R, gimli::Error> {
        let data = sections.get(id.name()).map_or(&[][..], Vec::as_slice);
        Ok(EndianSlice::new(data, LittleEndian))
    })?;
    let mut b = Builder::default();
    let mut units = dwarf.units();
    while let Some(header) = units.next()? {
        let unit = dwarf.unit(header)?;
        // (A function's id, as the plugin has it: its name's hash, or, local
        // to its file, "file:name"'s, the file the unit's own.)
        let file = unit.name.map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let id = |name: &str, external: bool| if external { super::id(name) } else { super::id(&format!("{file}:{name}")) };
        let mut cu = Cu { dwarf: &dwarf, unit: &unit, memo: HashMap::new(), files: HashMap::new() };
        // Every type in the unit, so that unused ones are listed too, and
        // every variable with a fixed address.
        let mut entries = unit.entries();
        let mut offsets = Vec::new();
        let mut vars = Vec::new();
        let mut funcs = Vec::new();
        let mut abstract_funcs = Vec::new();
        while let Some(e) = entries.next_dfs()? {
            match e.tag() {
                dw::DW_TAG_structure_type | dw::DW_TAG_union_type | dw::DW_TAG_enumeration_type
                | dw::DW_TAG_typedef => offsets.push(e.offset()),
                dw::DW_TAG_subprogram => {
                    let name = attr_string(&cu, e, dw::DW_AT_name)?;
                    if !name.is_empty() && e.attr(dw::DW_AT_declaration).is_none() {
                        // (Where it is: for the inline ones too, which have no probe.)
                        let (file, line) = (decl_file(&mut cu, e)?, attr_u64(e, dw::DW_AT_decl_line).unwrap_or(0));
                        b.locate(&name, &file, line);
                        // (A function inlined somewhere is abstract here, with
                        // no locations: its out-of-line copy, if any, has
                        // them, and comes first.)
                        let fid = id(&name, e.attr(dw::DW_AT_external).is_some());
                        if e.attr(dw::DW_AT_inline).is_none() {
                            funcs.push((name, fid, e.offset()));
                        } else {
                            abstract_funcs.push((name, fid, e.offset()));
                        }
                    } else if name.is_empty() && e.attr(dw::DW_AT_low_pc).is_some()
                        && let Some(AttributeValue::UnitRef(origin)) = e.attr_value(dw::DW_AT_abstract_origin) {
                        let origin = unit.entry(origin)?;
                        let name = attr_string(&cu, &origin, dw::DW_AT_name)?;
                        if !name.is_empty() {
                            let fid = id(&name, origin.attr(dw::DW_AT_external).is_some());
                            funcs.push((name, fid, e.offset()));
                        }
                    }
                }
                dw::DW_TAG_variable => {
                    if let (Some(addr), Some(AttributeValue::UnitRef(ty))) =
                        (address(&cu, e)?, e.attr_value(dw::DW_AT_type))
                    {
                        let at = (decl_file(&mut cu, e)?, attr_u64(e, dw::DW_AT_decl_line).unwrap_or(0));
                        vars.push((attr_string(&cu, e, dw::DW_AT_name)?, addr, ty, at));
                    }
                }
                _ => {}
            }
        }
        for off in offsets {
            b.canon(&mut cu, off)?;
        }
        for (name, fid, off) in funcs.into_iter().chain(abstract_funcs) {
            if !b.functions.insert(fid) {
                continue;
            }
            let mut sig = Vec::new();
            for (pname, ty) in params(&cu, off)? {
                let t = match ty {
                    Some(t) => b.canon(&mut cu, t)?,
                    None => 0,
                };
                sig.push(format!("{pname}:{t}"));
            }
            b.extra.push(format!("P\t{fid}\t{name}\t{}", sig.join(",")));
            let mut vars = Vec::new();
            for (vname, ty, at) in locals(&cu, off)? {
                // (One not in memory: no offset. Its values are in the
                // recording: see the plugin's V lines.)
                if let Some(t) = ty {
                    vars.push(format!("{vname}:{}:{}", b.canon(&mut cu, t)?, at.map_or(String::new(), |a| a.to_string())));
                }
            }
            if !vars.is_empty() {
                b.extra.push(format!("Q\t{fid}\t{}", vars.join(",")));
            }
        }
        for (name, addr, ty, (file, line)) in vars {
            if !name.is_empty() && b.globals.insert(name.clone()) {
                let t = b.canon(&mut cu, ty)?;
                b.extra.push(format!("G\t{name}\t{addr}\t{t}"));
                b.locate(&name, &file, line);
            }
        }
    }
    Ok(b.text())
}
/// The local variables of the subprogram at `off`, in its lexical blocks
/// too (and those of the calls inlined into it that are in memory, in its
/// frame): (name, type, its offset from the frame base if it is in memory:
/// a location `DW_OP_WASM_location 0 <the frame base's local>,
/// DW_OP_plus_uconst off`, in any range of its list).
fn locals(cu: &Cu, off: UnitOffset) -> Result<Vec<(String, Option<UnitOffset>, Option<u32>)>, gimli::Error> {
    type Out = Vec<(String, Option<UnitOffset>, Option<u32>)>;
    // (The frame base: DW_AT_frame_base is DW_OP_WASM_location 0 <local>.)
    let mut tree = cu.unit.entries_tree(Some(off))?;
    let root = tree.root()?;
    let fb = match root.entry().attr_value(dw::DW_AT_frame_base) {
        Some(AttributeValue::Exprloc(e)) => match e.operations(cu.unit.encoding()).next()? {
            Some(gimli::Operation::WasmLocal { index }) => Some(index),
            _ => None,
        },
        _ => None,
    };
    // (`DW_OP_fbreg N`; or `DW_OP_WASM_location 0 fb` with DW_OP_plus_uconsts
    // (added up), possibly in pieces, one per member: the object's start
    // is each located piece's address less its offset in the object, the
    // same for all of them.)
    let frame_offset = |ops: gimli::Expression<R>| -> Result<Option<u32>, gimli::Error> {
        use gimli::Operation as Op;
        let mut ops = ops.operations(cu.unit.encoding());
        let (mut pos, mut cur, mut base): (u32, Option<u32>, Option<u32>) = (0, None, None);
        loop {
            match ops.next()? {
                Some(Op::FrameOffset { offset }) if pos == 0 && cur.is_none() => return Ok(u32::try_from(offset).ok()),
                Some(Op::WasmLocal { index }) if Some(index) == fb => cur = Some(0),
                Some(Op::PlusConstant { value }) => cur = cur.map(|c| c + value as u32),
                Some(Op::Piece { size_in_bits, .. }) => {
                    if let Some(c) = cur {
                        let Some(b) = c.checked_sub(pos) else { return Ok(None) };
                        if base.is_some_and(|x| x != b) {
                            return Ok(None);
                        }
                        base = Some(b);
                    }
                    pos += size_in_bits as u32 / 8;
                    cur = None;
                }
                None => return Ok(if pos == 0 { cur } else { base }),
                // (A value in a wasm local, a constant, ...: not memory.)
                _ => cur = None,
            }
        }
    };
    fn walk(cu: &Cu, node: gimli::EntriesTreeNode<R>, out: &mut Out, inlined: bool,
            frame_offset: &dyn Fn(gimli::Expression<R>) -> Result<Option<u32>, gimli::Error>) -> Result<(), gimli::Error> {
        let mut children = node.children();
        while let Some(child) = children.next()? {
            let c = child.entry();
            match c.tag() {
                dw::DW_TAG_variable => {
                    let (name, ty) = name_type(cu, c)?;
                    let mut at = None;
                    match c.attr_value(dw::DW_AT_location) {
                        Some(AttributeValue::Exprloc(e)) => at = frame_offset(e)?,
                        Some(v) => {
                            if let Some(mut list) = cu.dwarf.attr_locations(cu.unit, v)? {
                                while let Some(entry) = list.next()? {
                                    if let Some(o) = frame_offset(entry.data)? {
                                        at = Some(o);
                                        break;
                                    }
                                }
                            }
                        }
                        None => {}
                    }
                    if at.is_some() || !inlined {
                        out.push((name, ty, at));
                    }
                }
                // (An inlined call's locals in memory are in this frame too.)
                dw::DW_TAG_lexical_block => walk(cu, child, out, inlined, frame_offset)?,
                dw::DW_TAG_inlined_subroutine => walk(cu, child, out, true, frame_offset)?,
                _ => {}
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(cu, root, &mut out, false, &frame_offset)?;
    out.retain(|(n, _, _)| !n.is_empty());
    out.dedup();
    Ok(out)
}

/// A variable's or parameter's name and type: its own, or, in an
/// out-of-line copy of a function inlined elsewhere, its abstract origin's.
fn name_type(cu: &Cu, e: &gimli::DebuggingInformationEntry<R>) -> Result<(String, Option<UnitOffset>), gimli::Error> {
    let origin = match e.attr_value(dw::DW_AT_abstract_origin) {
        Some(AttributeValue::UnitRef(o)) => Some(cu.unit.entry(o)?),
        _ => None,
    };
    let name = match attr_string(cu, e, dw::DW_AT_name)? {
        n if n.is_empty() && origin.is_some() => attr_string(cu, origin.as_ref().unwrap(), dw::DW_AT_name)?,
        n => n,
    };
    let ty = match e.attr_value(dw::DW_AT_type).or_else(|| origin.as_ref().and_then(|o| o.attr_value(dw::DW_AT_type))) {
        Some(AttributeValue::UnitRef(t)) => Some(t),
        _ => None,
    };
    Ok((name, ty))
}

/// The formal parameters of the subprogram at `off`: (name, type).
fn params(cu: &Cu, off: UnitOffset) -> Result<Vec<(String, Option<UnitOffset>)>, gimli::Error> {
    let mut params = Vec::new();
    let mut tree = cu.unit.entries_tree(Some(off))?;
    let mut children = tree.root()?.children();
    while let Some(child) = children.next()? {
        let c = child.entry();
        if c.tag() == dw::DW_TAG_formal_parameter {
            params.push(name_type(cu, c)?);
        }
    }
    Ok(params)
}

struct Cu<'a, 'b> {
    dwarf: &'a gimli::Dwarf<R<'b>>,
    unit: &'a gimli::Unit<R<'b>>,
    /// Unit offset -> type id.
    memo: HashMap<UnitOffset, u32>,
    files: HashMap<u64, String>,
}

/// A type's row (`T id kind name size rest`): `rest` its fields after
/// the size, each with its tab ("\ts", "\t12\t4").
#[derive(Default)]
struct Row {
    kind: &'static str,
    name: String,
    size: u32,
    rest: String,
}

#[derive(Default)]
struct Builder {
    /// Rows of the type table, by id (0 is void).
    rows: Vec<Row>,
    /// P, Q, G, M, V and W lines (see types and members), in order.
    extra: Vec<String>,
    ids: HashMap<String, u32>,
    /// (tag, name) -> the id of the first complete definition, or a
    /// placeholder for it reserved by a declaration.
    by_name: HashMap<(DwTag, String), (u32, bool)>,
    /// Names of globals listed (the first of several statics wins).
    globals: std::collections::HashSet<String>,
    /// Functions given a P line.
    /// (By id: see types.)
    functions: std::collections::HashSet<u32>,
    /// Names given a W line (the first declaration of a name wins).
    located: std::collections::HashSet<String>,

}

impl Builder {
    fn new_id(&mut self) -> u32 {
        if self.rows.is_empty() {
            self.rows.push(Row::default()); // 0: void
        }
        self.rows.push(Row::default());
        self.rows.len() as u32 - 1
    }

    fn intern(&mut self, key: String, row: impl FnOnce(u32) -> Row) -> u32 {
        if let Some(&id) = self.ids.get(&key) {
            return id;
        }
        let id = self.new_id();
        self.rows[id as usize] = row(id);
        self.ids.insert(key, id);
        id
    }

    /// Where `name` (a function, a type, a global, or a member as
    /// "struct.member") is declared: a W line, once per name.
    fn locate(&mut self, name: &str, file: &str, line: u64) {
        if !name.is_empty() && !file.is_empty() && self.located.insert(name.to_string()) {
            self.extra.push(format!("W\t{name}\t{}\t{line}", file.strip_prefix("./").unwrap_or(file)));
        }
    }

    fn text(&self) -> String {
        let mut out = String::new();
        for (id, r) in self.rows.iter().enumerate().skip(1) {
            let _ = writeln!(out, "T\t{id}\t{}\t{}\t{}{}", r.kind, r.name, r.size, r.rest);
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
                self.intern(format!("base {name} {size} {enc}"), |_| Row { kind: "base", name: name.clone(), size, rest: format!("\t{enc}") })
            }
            dw::DW_TAG_const_type | dw::DW_TAG_volatile_type | dw::DW_TAG_restrict_type
            | dw::DW_TAG_atomic_type => target(self, cu)?,
            dw::DW_TAG_pointer_type => {
                // Memoize before recursing: pointers close cycles.
                let t = target(self, cu)?;
                self.intern(format!("ptr {t}"), |_| Row { kind: "ptr", name: String::new(), size: 4, rest: format!("\t{t}") })
            }
            dw::DW_TAG_typedef => {
                let t = target(self, cu)?;
                let size = self.size(t);
                let (file, line) = (decl_file(cu, &e)?, attr_u64(&e, dw::DW_AT_decl_line).unwrap_or(0));
                self.locate(&name, &file, line);
                self.intern(format!("typedef {name} {t}"), |_| Row { kind: "typedef", name: name.clone(), size, rest: format!("\t{t}") })
            }
            dw::DW_TAG_subroutine_type => self.intern("func".into(), |_| Row { kind: "func", ..Row::default() }),
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
                    id = self.intern(format!("array {id} {n}"), |_| Row { kind: "array", name: String::new(), size, rest: format!("\t{id}\t{n}") });
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
                        self.rows[id as usize] = Row { kind, name: name.clone(), ..Row::default() };
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
                    }
                    self.ids.insert(key, id);
                    self.locate(&name, &file, line);
                    self.rows[id as usize] = Row { kind, name: name.clone(), size, rest: String::new() };
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
        // (The aggregate's name, for its members' declarations: "file.f_pos".)
        let owner = self.rows[id as usize].name.clone();
        let unit = cu.unit;
        let mut tree = unit.entries_tree(Some(off))?;
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
                    let (file, line) = (decl_file(cu, c)?, attr_u64(c, dw::DW_AT_decl_line).unwrap_or(0));
                    let t = match c.attr_value(dw::DW_AT_type) {
                        Some(AttributeValue::UnitRef(t)) => self.canon(cu, t)?,
                        _ => 0,
                    };
                    self.extra.push(format!("M\t{id}\t{name}\t{bit_offset}\t{bit_size}\t{t}"));
                    if !owner.is_empty() && !name.is_empty() {
                        self.locate(&format!("{owner}.{name}"), &file, line);
                    }
                }
                dw::DW_TAG_enumerator if tag == dw::DW_TAG_enumeration_type => {
                    let v = match c.attr_value(dw::DW_AT_const_value) {
                        Some(AttributeValue::Sdata(v)) => v,
                        Some(AttributeValue::Udata(v)) => v as i64,
                        Some(v) => v.sdata_value().unwrap_or(0),
                        None => 0,
                    };
                    self.extra.push(format!("V\t{id}\t{name}\t{v}"));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The size of type `id`.
    fn size(&self, id: u32) -> u32 {
        self.rows.get(id as usize).map_or(0, |r| r.size)
    }
}
/// A variable's fixed address (DW_OP_addr or DW_OP_addrx), if it has one.
fn address(cu: &Cu, e: &gimli::DebuggingInformationEntry<R>) -> Result<Option<u64>, gimli::Error> {
    let Some(AttributeValue::Exprloc(expr)) = e.attr_value(dw::DW_AT_location) else { return Ok(None) };
    let mut ops = expr.operations(cu.unit.encoding());
    Ok(match ops.next()? {
        Some(gimli::Operation::Address { address }) => Some(address),
        Some(gimli::Operation::AddressIndex { index }) => Some(cu.dwarf.address(cu.unit, index)?),
        _ => None,
    })
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_of_an_image_without_dwarf_is_empty() {
        let image: Vec<u8> = [b"\0asm\x01\0\0\0".as_slice()].concat();
        assert_eq!(types(&image).unwrap(), "");
    }
}
