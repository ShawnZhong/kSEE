//! A value in memory, as the page shows it (Object): a type's, read.

use super::*;

/// A value in memory. Aggregates beyond the requested depth have no
/// children yet (`open` says what to ask for them).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Object {
    pub name: String,
    pub ty: String,
    pub addr: u32,
    /// Its size in bytes.
    pub size: u32,
    pub value: Option<String>,
    /// Why it has none, if (its `value` says it too, in parentheses).
    pub missing: Option<Missing>,
    pub children: Option<Vec<Object>>,
    /// What opening it reads (its children not read yet): itself, an
    /// aggregate; or what it points to, a pointer to a struct or union not
    /// NULL.
    pub open: Option<Target>,
}

/// How many elements of an array a value shows (then a row: how many more).
const SHOWN: u32 = 64;

/// What a pointer points to: its type's id, and its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub ty: u32,
    pub addr: u32,
}

/// Why a value has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// Not stored to yet.
    Uninitialized,
    /// The recording doesn't have it.
    NotRecorded,
    /// The optimizer took it away.
    OptimizedOut,
}

impl Missing {
    /// As its value says it: "(uninitialized)".
    pub fn text(self) -> &'static str {
        match self {
            Missing::Uninitialized => "(uninitialized)",
            Missing::NotRecorded => "(not recorded)",
            Missing::OptimizedOut => "(optimized out)",
        }
    }

    /// Why, for one who wonders.
    pub fn why(self) -> &'static str {
        match self {
            Missing::Uninitialized => "nothing has been stored in it yet",
            Missing::NotRecorded => "the recording does not have this memory",
            Missing::OptimizedOut => "the compiler kept no copy of it here: once the code no longer needs a value, it may drop it, or never store it",
        }
    }
}

impl Object {
    /// Its value as the page shows it: `{…}` for one that has members.
    pub fn shown(&self) -> String {
        self.value.clone().unwrap_or_else(|| "{…}".into())
    }

    /// Its members (or what it points to, read from `mem` as `types` lay
    /// it out), if it has any.
    pub fn members(&self, types: &crate::types::Types, mem: &dyn crate::Memory) -> Option<Vec<Object>> {
        self.children.clone().or_else(|| self.open.and_then(|t| types.object(mem, &self.name, t.addr, t.ty, 1).children))
    }

    /// Without a value, for `why`.
    pub(crate) fn missing(self, why: Missing) -> Object {
        Object { value: Some(why.text().into()), missing: Some(why), children: None, open: None, ..self }
    }
}

/// An array's elements, a run of three or more the same plain value (none
/// to open: NULL, 0) one row ("[4…63]").
fn fold(rows: Vec<Object>) -> Vec<Object> {
    let plain = |o: &Object| o.children.is_none() && o.open.is_none() && o.missing.is_none() && o.value.is_some();
    let mut out = Vec::with_capacity(rows.len());
    let mut i = 0;
    while i < rows.len() {
        let j = (i + 1..rows.len()).find(|&j| !(plain(&rows[i]) && plain(&rows[j]) && rows[j].value == rows[i].value)).unwrap_or(rows.len());
        if j - i >= 3 {
            out.push(Object { name: format!("[{i}…{}]", j - 1), ..rows[i].clone() });
        } else {
            out.extend(rows[i..j].iter().cloned());
        }
        i = j;
    }
    out
}

impl Types {
    /// What the struct of type `id` (resolved) at `addr` is, if it is one
    /// a value names (arch's): a task, comm(pid); an open file, where it
    /// is; a dentry, its name.
    fn label(&self, mem: &dyn Memory, id: u32, addr: u32) -> Option<String> {
        let is = |name: &str| self.find(name).map(|t| self.resolve(t)) == Some(id);
        let at = |s: &str, f: &str| self.field(s, f).map(|(off, _)| mem.read_u32(addr.wrapping_add(off)));
        if is(arch::TASK) {
            return self.read_task(mem, addr).map(|t| format!("{}({})", t.comm, t.pid));
        }
        if is(arch::FILE) {
            let (d, m, i) = arch::FILE_FIELDS;
            return Some(self.path(mem, at(arch::FILE, d)?, at(arch::FILE, m)?, at(arch::FILE, i)?));
        }
        if is(arch::DENTRY.0) {
            return Some(String::from_utf8_lossy(&c_str(mem, at(arch::DENTRY.0, arch::DENTRY.1)?, 256)).into_owned());
        }
        None
    }

    /// Where the dentry `dentry` in the mount whose vfsmount is at
    /// `vfsmount` is: its names up to the top mount's root ("/mnt/note.txt").
    /// One that is no place's (its own parent, with no name: a pipe's), as
    /// Linux names it: its file system's type and its inode's number
    /// ("pipefs:[12]").
    fn path(&self, mem: &dyn Memory, dentry: u32, vfsmount: u32, inode: u32) -> String {
        let field = |s: &str, f: &str| self.field(s, f).map_or(0, |(off, _)| off);
        let (d, name, parent, sb) = arch::DENTRY;
        let (mount, mnt, mnt_parent, mountpoint) = arch::MOUNT;
        let name_of = |d_: u32| String::from_utf8_lossy(&c_str(mem, mem.read_u32(d_.wrapping_add(field(d, name))), 256)).into_owned();
        let (mut at, mut m) = (dentry, vfsmount.wrapping_sub(field(mount, mnt)));
        let mut parts: Vec<String> = Vec::new();
        // (Guards: a tree read from memory as it was may be torn.)
        for _ in 0..64 {
            let up = mem.read_u32(at.wrapping_add(field(d, parent)));
            if up != at && up != 0 {
                parts.push(name_of(at));
                at = up;
                continue;
            }
            // (A file system's root: on in the mount it is mounted on, if any.)
            let above = mem.read_u32(m.wrapping_add(field(mount, mnt_parent)));
            if above == m || above == 0 {
                break;
            }
            (at, m) = (mem.read_u32(m.wrapping_add(field(mount, mountpoint))), above);
        }
        if parts.is_empty() && name_of(dentry).is_empty() {
            let (sup, s_type, fs_type, fs_name) = arch::SUPER;
            let ty = mem.read_u32(mem.read_u32(dentry.wrapping_add(field(d, sb))).wrapping_add(field(sup, s_type)));
            let fs = String::from_utf8_lossy(&c_str(mem, mem.read_u32(ty.wrapping_add(field(fs_type, fs_name))), 32)).into_owned();
            return format!("{fs}:[{}]", mem.read_u32(inode.wrapping_add(field(arch::INODE_NO.0, arch::INODE_NO.1))));
        }
        parts.reverse();
        format!("/{}", parts.join("/"))
    }

    /// The value at `path` in the value of type `ty` at `addr`, as C names
    /// it: members by `.`, a pointer followed by `->` ("files->fd_array");
    /// named by the path. None if a member is not there, or a pointer on
    /// the way is NULL.
    pub fn at_path(&self, mem: &dyn Memory, ty: u32, addr: u32, path: &str) -> Option<Object> {
        let (mut ty, mut addr) = (ty, addr);
        for (k, step) in path.split("->").enumerate() {
            if k > 0 {
                let Kind::Pointer(to) = self.get(self.resolve(ty)).kind else { return None };
                (ty, addr) = (to, mem.read_u32(addr));
                if addr == 0 {
                    return None;
                }
            }
            for name in step.split('.') {
                let m = self.members(&self.get(self.resolve(ty)).kind).into_iter().find(|m| m.name == name)?;
                (ty, addr) = (m.ty, addr + m.bit_offset / 8);
            }
        }
        Some(self.object(mem, path, addr, ty, 0))
    }

    /// The value of type `ty` at `addr`, aggregates expanded `depth` deep.
    pub fn object(&self, mem: &dyn Memory, name: &str, addr: u32, ty: u32, depth: u32) -> Object {
        self.object_bits(mem, name, addr, ty, None, depth)
    }

    /// The same, for a value that may be a bitfield: `bits`, its (offset in
    /// the byte at `addr`, size).
    fn object_bits(&self, mem: &dyn Memory, name: &str, addr: u32, ty: u32, bits: Option<(u32, u32)>, depth: u32) -> Object {
        let id = self.resolve(ty);
        let t = self.get(id);
        let mut o = Object {
            name: name.to_string(), ty: self.name(ty), addr, size: self.size_of(ty),
            value: None, missing: None, children: None, open: None,
        };
        match &t.kind {
            Kind::Struct(_) | Kind::Union(_) => {
                o.value = self.label(mem, id, addr);
                if depth == 0 {
                    o.open = Some(Target { ty: id, addr });
                } else {
                    // (Not one of size 0: nothing in it, a struct its
                    // config left empty, as rseq_data without CONFIG_RSEQ.)
                    o.children = Some(self.members(&t.kind).iter().filter(|m| m.bit_size > 0 || self.size_of(m.ty) > 0).map(|m| {
                        let bits = (m.bit_size > 0).then_some((m.bit_offset % 8, m.bit_size));
                        self.object_bits(mem, &m.name, addr + m.bit_offset / 8, m.ty, bits, depth - 1)
                    }).collect());
                }
            }
            Kind::Array { elem, count } if !self.is_string(*elem) => {
                if depth == 0 {
                    o.open = Some(Target { ty: id, addr });
                } else {
                    let esize = self.size_of(*elem);
                    let mut shown: Vec<Object> = (0..(*count).min(SHOWN)).map(|i| {
                        self.object_bits(mem, &format!("[{i}]"), addr + i * esize, *elem, None, depth - 1)
                    }).collect();
                    shown = fold(shown);
                    // (Past the first SHOWN: a row saying how many more.)
                    if *count > SHOWN {
                        shown.push(Object { name: format!("[{SHOWN}…{}]", count - 1), ty: String::new(), addr: addr + SHOWN * esize, size: 0,
                                            value: Some(format!("… {} more", count - SHOWN)), missing: None, children: None, open: None });
                    }
                    o.children = Some(shown);
                }
            }
            Kind::Pointer(to) => {
                let p = mem.read_u32(addr);
                // (A char *: and the string it points to: "0x1d40130 → \"/sub\"", as any pointer's target;
                // a __user one too: a program's memory is kernel memory. Past 64 characters, "…".)
                let text = (p != 0 && self.is_string(*to)).then(|| quote(&c_str(mem, p, 65)));
                o.value = Some(match text {
                    _ if p == 0 => "NULL".into(),
                    Some(t) => format!("{p:#x} → {t}"),
                    None => format!("{p:#x}"),
                });
                let target = self.resolve(*to);
                match self.get(target).kind {
                    Kind::Struct(_) | Kind::Union(_) if p != 0 && self.size_of(target) > 0 => {
                        o.open = Some(Target { ty: target, addr: p });
                        if let Some(l) = self.label(mem, target, p) {
                            o.value = Some(format!("{p:#x} → {l}"));
                        }
                    }
                    // (A pointer to a scalar: what it points to, in line: "0x7ffd8 → 0".)
                    Kind::Base(_) | Kind::Enum(_) | Kind::Pointer(_) if p != 0 && !self.is_string(*to) => {
                        let v = self.print(mem, p, *to, 0, 0);
                        o.value = Some(format!("{p:#x} → {v}"));
                    }
                    _ => {}
                }
            }
            _ => {
                let (off, size) = bits.unwrap_or((0, 0));
                o.value = Some(self.print(mem, addr, ty, off, size));
            }
        }
        o
    }
}
