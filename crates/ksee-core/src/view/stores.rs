//! A span's stores, named as the code names what they hit (View::stores),
//! by the things they changed (Named::things): printed where shown, the
//! source's notes (View::notes) and Objects (objects.rs); and who wrote a
//! value, back through its copies (View::origin).

use super::*;

/// What a region is (see Region), as the page names it ("object").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    Task,
    Object,
    Global,
    Machine,
    Local,
    Var,
    Other,
}

/// What a span's stores hit, as the code names it (see View::stores):
/// `what` ("file in vfs_read", "x in f", "nr_inodes", "prog(19)"; the
/// address when nothing names it), its `kind` ("task", "object": a struct
/// a call's argument points to, "global", "machine": one of the machine's
/// variables, or what its functions name (arch::MACHINE), "local", "var":
/// a variable kept in a wasm local, or "other": memory nothing names), its
/// type `ty` (0 for none: memory nothing names; a var's, the variable's),
/// where it is (`addr`, `size`), for a call's local (or an object in one)
/// the call's records, its `life`, and how deep the call that names it is
/// (the task's, 0; a global's, or memory nothing names, u32::MAX): Objects
/// comes in that order.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub what: String,
    /// The function that names it, if one does ("vfs_read": "file in
    /// vfs_read").
    pub within: Option<String>,
    pub kind: RegionKind,
    pub ty: u32,
    pub addr: u32,
    pub size: u32,
    pub life: Option<(usize, usize)>,
    pub depth: u32,
}

/// A region as the code names it (see name_stores): what it is called,
/// and the function calling it so; its kind, type and address; for a
/// call's local or frame, the call it lives in (its life); the record that
/// names it (its call's depth).
struct Naming {
    what: String,
    within: Option<String>,
    kind: RegionKind,
    ty: u32,
    addr: u32,
    call: Option<usize>,
    by: Option<usize>,
}

/// A span's regions, and the things its stores and variable sets wrote
/// (Thing), in the order first written.
#[derive(Debug, Default)]
pub(crate) struct Named {
    pub regions: Vec<Region>,
    pub things: Vec<Thing>,
}

/// A store or a variable set: at record `i`, to `region`, on line `at`
/// (Recording::line_at).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wrote {
    pub i: usize,
    pub region: u32,
    pub at: Option<LineStep>,
}

/// A thing a span's writes changed: a member of a region (a path: "f_pos",
/// "f_path.dentry"; "" the region itself: a variable, or memory nothing
/// names), with its leaf (a typed region's), and its writes, in order. The
/// one path both the source's notes and Objects read.
#[derive(Debug, Clone, PartialEq)]
pub struct Thing {
    pub region: u32,
    pub member: String,
    pub leaf: Option<Leaf>,
    pub writes: Vec<Wrote>,
}

/// A member's scalar, as Types::leaf finds it: its type, its offset in the
/// object, and its bits if a bitfield.
pub type Leaf = (u32, u32, Option<(u32, u32)>);

/// What a source line stored (or set) by a moment (View::notes): each
/// thing it wrote, said `short` and in `full`; its last write at record
/// `i`.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub line: u32,
    pub i: usize,
    pub short: String,
    pub full: String,
}

/// A write to a value (`addr`): at record `i`, on `task`, by the code at
/// `at` (the line, if known); `old` what it overwrote (printed: a number,
/// in hex past 0xffff); `from`: if it
/// copied what was at another address (a value loaded and stored as it
/// is, a memcpy), that address.
#[derive(Debug, Clone, PartialEq)]
pub struct Write {
    pub i: usize,
    pub addr: u32,
    pub old: String,
    pub task: Task,
    pub at: Option<Line>,
    pub from: Option<u32>,
}

impl View<'_> {
    /// Where the value at `addr..addr + len` after record `at` came from:
    /// its newest write, and while that copied it from elsewhere, the
    /// newest write there before it, and so on (up to `max`): the value's
    /// way through memory, newest first. (The last one computed it, or
    /// copied it from memory the recording did not see written.)
    pub fn origin(&self, addr: u32, len: u32, at: usize, max: usize) -> Vec<Write> {
        let mut out: Vec<Write> = Vec::new();
        let (mut addr, mut at) = (addr, at);
        while out.len() < max && let Some((i, old)) = self.rec.stores().last_write(addr, len, at) {
            // (A copy: from where the store and the value overlap.)
            let from = match self.rec.records[i] {
                Record::Store { addr: to, from: Some(from), .. } => Some(from.get().wrapping_add(addr.max(to) - to)),
                _ => None,
            };
            let line = self.rec.line_at(self.tables, i).map(|s| self.line_only(&s));
            out.push(Write { i, addr, old: num(old), task: self.task(self.rec.tasks[i], i), at: line, from });
            let (Some(src), Some(before)) = (from, i.checked_sub(1)) else { break };
            (addr, at) = (src, before);
        }
        out
    }

    /// What the store at record `i` wrote: memory there just after it.
    pub(super) fn wrote(&self, i: usize) -> u64 {
        let Record::Store { addr, size, .. } = self.rec.records[i] else { return 0 };
        let mut new = [0u8; 8];
        self.rec.memory_at(self.now, i).read(addr, &mut new[..size.min(8) as usize]);
        u64::from_le_bytes(new)
    }

    /// The members of `r` the store at record `i` wrote (none for a
    /// variable set): each its path and, for a typed region, the leaf there
    /// (types::leaves: one store may copy several members, a struct path's
    /// mnt and dentry at once).
    pub(super) fn members(&self, r: &Region, i: usize) -> Vec<(String, Option<Leaf>)> {
        let Record::Store { addr, size, .. } = self.rec.records[i] else { return Vec::new() };
        // (No type, memory nothing names: the store as it is.)
        if r.ty == 0 {
            return alloc::vec![(String::new(), None)];
        }
        let off = addr.max(r.addr) - r.addr;
        let len = size.min(r.size.saturating_sub(off));
        self.tables.types.leaves(r.ty, off, len).into_iter().map(|(path, leaf, off, bits)| (path, Some((leaf, off, bits)))).collect()
    }

    /// Leaf `leaf` of the object at `base` (see members) as it was after
    /// record `at`, printed.
    pub(super) fn value(&self, base: u32, (leaf, off, bits): Leaf, at: usize) -> String {
        let (b0, bn) = bits.unwrap_or((0, 0));
        self.tables.types.print(&self.rec.memory_at(self.now, at), base + off, leaf, b0, bn)
    }

    /// Variable set `i` (a Var record)'s value, printed.
    fn var_value(&self, i: usize) -> String {
        let Record::Var { var, value, .. } = self.rec.records[i] else { return String::new() };
        let v = self.rec.calls.call_at(i).and_then(|c| self.tables.vars.get(&(c.func, var)));
        v.and_then(|v| self.var_object(v, value, self.now)?.value).unwrap_or_default()
    }

    /// Thing `t`'s write `k` (of its writes), printed: from what to what (a
    /// variable from its set before, in the span).
    pub(crate) fn change(&self, named: &Named, t: &Thing, k: usize) -> (String, String) {
        let (r, w) = (&named.regions[t.region as usize], &t.writes[k]);
        if r.kind == RegionKind::Var {
            let before = k.checked_sub(1).map_or_else(|| Missing::Uninitialized.text().into(), |p| self.var_value(t.writes[p].i));
            return (before, self.var_value(w.i));
        }
        match t.leaf {
            Some(leaf) => (self.value(r.addr, leaf, w.i.saturating_sub(1)), self.value(r.addr, leaf, w.i)),
            None => {
                let Record::Store { old, .. } = self.rec.records[w.i] else { unreachable!() };
                (num(old), num(self.wrote(w.i)))
            }
        }
    }

    /// The stores in records `first..=last` (a call's), and the variables
    /// set there, in order, each by what the code names that it hit (see
    /// Region): a call's pointer argument to a struct, a local, the task
    /// running, a global of the kernel's; else by its address. The last
    /// span asked for, kept: a render asks the same span again (objects,
    /// each line's notes, origins), and naming it again each time would
    /// cost the span's records over.
    pub(crate) fn stores(&self, first: usize, last: usize) -> alloc::rc::Rc<Named> {
        if let Some((span, got)) = &*self.rec.named.borrow() && *span == (first, last) {
            return got.clone();
        }
        let got = alloc::rc::Rc::new(self.name_stores(first, last));
        *self.rec.named.borrow_mut() = Some(((first, last), got.clone()));
        got
    }

    /// What each line of `file` stored (or set) in records `first..=last`
    /// by record `cur` (the source's notes): each thing it wrote (a
    /// member, a variable), its last change and how many times, said short
    /// (the member, or the variable, and the change: the line says the
    /// rest) and in full; at the line's last write.
    pub fn notes(&self, first: usize, last: usize, file: &str, cur: usize) -> Vec<Note> {
        let named = self.stores(first, last);
        let files = self.tables.file_ids(file);
        // (Each line's things, in the order first written there: the thing,
        // its last write there by `cur`, how many; and the line's last write.)
        let mut by: BTreeMap<u32, (Vec<(&Thing, usize, usize)>, usize)> = BTreeMap::new();
        for t in &named.things {
            for (k, w) in t.writes.iter().enumerate().take_while(|(_, w)| w.i <= cur) {
                let Some(at) = w.at.filter(|at| files.contains(&at.loc.file)) else { continue };
                let (things, last) = by.entry(at.loc.line).or_default();
                *last = (*last).max(w.i);
                match things.iter_mut().find(|(x, _, _)| core::ptr::eq(*x, t)) {
                    Some((_, at, n)) => (*at, *n) = (k, *n + 1),
                    None => things.push((t, k, 1)),
                }
            }
        }
        by.into_iter().map(|(line, (mut things, i))| {
            things.sort_by_key(|(t, _, _)| t.writes.iter().find(|w| w.at.is_some_and(|a| a.loc.line == line)).map(|w| w.i));
            // (Short, a name once (two inlined copies' `sb` are one `sb` in
            // the text), each change it had once, the times added: "sb: NULL
            // → 0x1c49000, (uninitialized) → 0x1c49000 ×2".)
            let (mut short, mut full): (Vec<(String, Vec<(String, usize)>)>, Vec<String>) = (Vec::new(), Vec::new());
            for (t, k, n) in things {
                let r = &named.regions[t.region as usize];
                let (before, after) = self.change(&named, t, k);
                let join = if r.kind == RegionKind::Object { "->" } else { "." };
                // (The member after the name, the function it is in last: "filp->f_pos in vfs_read".)
                let (expr, within) = (r.what.as_str(), r.within.as_ref().map_or(String::new(), |f| format!(" in {f}")));
                let times = if n > 1 { format!(" ×{n}") } else { String::new() };
                full.push(format!("{expr}{}{within}: {before} → {after}{times}", if t.member.is_empty() { String::new() } else { format!("{join}{}", t.member) }));
                let name = if t.member.is_empty() { expr.rsplit(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or(expr) } else { &t.member };
                let change = format!("{before} → {after}");
                let k = short.iter().position(|(s, _)| s == name).unwrap_or_else(|| (short.push((name.to_string(), Vec::new())), short.len() - 1).1);
                match short[k].1.iter_mut().find(|(c, _)| *c == change) {
                    Some((_, k)) => *k += n,
                    None => short[k].1.push((change, n)),
                }
            }
            let short = short.into_iter().map(|(name, changes)| {
                let changes: Vec<String> = changes.into_iter().map(|(c, n)| if n > 1 { format!("{c} ×{n}") } else { c }).collect();
                format!("{name}: {}", changes.join(", "))
            }).collect::<Vec<_>>().join("   ");
            Note { line, i, short, full: full.join("; ") }
        }).collect()
    }

    /// The span's stores and variable sets, each named (see stores: this
    /// is the naming; that, its cache).
    fn name_stores(&self, first: usize, last: usize) -> Named {
        let types = &self.tables.types;
        // (The objects: what (as the code names it: "file in vfs_read"),
        // type, address, size, and for a call's local or frame its life (the
        // call's records: its slots are other calls' before and after). The
        // task first.)
        let task = self.rec.tasks.get(first).copied().unwrap_or(0);
        let mut regions: Vec<Region> = Vec::new();
        let mut seen = alloc::collections::BTreeSet::new();
        let mut add = |n: Naming| {
            let ty = types.resolve(n.ty);
            if n.addr != 0 && seen.insert((n.addr, ty, n.call)) {
                let life = n.call.map(|c| (c, self.rec.calls.call_end(c).unwrap_or(usize::MAX)));
                let depth = n.by.and_then(|i| self.rec.calls.call_at(i)).map_or(0, |c| c.depth);
                regions.push(Region { what: n.what, within: n.within, kind: n.kind, ty, addr: n.addr, size: types.size_of(ty), life, depth });
            }
        };
        self.task_regions(task, first, last, &mut add);
        self.call_regions(first, last, &mut add);
        let mut writes = self.store_writes(first, last, &mut regions);
        writes.extend(self.var_writes(first, last, &mut regions));
        writes.sort_by_key(|w| w.i);
        // (The things they changed, in the order first written.)
        let mut things: Vec<Thing> = Vec::new();
        let mut index: BTreeMap<(u32, String), usize> = BTreeMap::new();
        for w in writes {
            let r = &regions[w.region as usize];
            let members = if r.kind == RegionKind::Var { alloc::vec![(String::new(), None)] } else { self.members(r, w.i) };
            for (member, leaf) in members {
                let k = *index.entry((w.region, member.clone())).or_insert_with(|| {
                    things.push(Thing { region: w.region, member, leaf, writes: Vec::new() });
                    things.len() - 1
                });
                things[k].writes.push(w);
            }
        }
        Named { regions, things }
    }

    /// The task running as the span begins, by its name ("prog(19)"), and
    /// what its pointer members point to as the span ends.
    fn task_regions(&self, task: u32, first: usize, last: usize,
                    add: &mut impl FnMut(Naming)) {
        let types = &self.tables.types;
        let Some(ty) = types.find(arch::TASK) else { return };
        // (By its name, "prog(19)": the task running as the span begins,
        // which another may follow in it; not "current".)
        let who = self.task(task, first);
        let who = who.named();
        add(Naming { what: who.clone(), within: None, kind: RegionKind::Task, ty, addr: task, call: None, by: None });
        // (And what its pointer members point to, as the span ends:
        // current->files, ->fs, ->mm: "files->next_fd" through a local
        // the optimizer kept in a register is reached this way.)
        let end = self.rec.memory_at(self.now, last.min(self.rec.last()));
        if let Kind::Struct(members) = &types.get(types.resolve(ty)).kind {
            for mb in members.iter().filter(|mb| mb.bit_size == 0) {
                if let Some(to) = types.struct_target(mb.ty) {
                    add(Naming { what: format!("{who}->{}", mb.name), within: None, kind: RegionKind::Object, ty: to,
                                 addr: end.read_u32(task.wrapping_add(mb.bit_offset / 8)), call: None, by: None });
                }
            }
        }
    }

    /// What the calls' code names in records `first..=last`: a call's
    /// pointer argument to a struct, its locals, and what its wasm-local
    /// variables point to.
    fn call_regions(&self, first: usize, last: usize,
                    add: &mut impl FnMut(Naming)) {
        let (t, types) = (self.tables, &self.tables.types);
        let (records, calls, args) = (&self.rec.records, &self.rec.calls, &self.rec.args);
        let last = last.min(self.rec.last());
        for i in first..=last {
            let Record::Block { func, n: 0 } = records[i] else { continue };
            let Some((f, (words, frame))) = t.functions.get(&func).zip(args.get(i)) else { continue };
            // (What the machine's own functions name, its: the call's plumbing.)
            let machine = t.file(f.loc).starts_with(arch::MACHINE);
            let kind = |k: RegionKind| if machine { RegionKind::Machine } else { k };
            for ((pname, ty), v) in types.params.get(&func).into_iter().flatten().zip(t.values(func, words)) {
                let v = v as u32;
                if let Some(to) = types.struct_target(*ty) {
                    add(Naming { what: pname.clone(), within: Some(f.name.clone()), kind: kind(RegionKind::Object), ty: to, addr: v,
                                 call: Self::frame_of(calls, args, i, v), by: Some(i) });
                }
            }
            // (Where its pointers point as the call ends.)
            let end = calls.call_end(i).unwrap_or(last).min(last);
            for (var, ty, off) in types.locals.get(&func).into_iter().flatten().filter_map(|(v, t, o)| Some((v, t, (*o)?))) {
                add(Naming { what: var.clone(), within: Some(f.name.clone()), kind: kind(RegionKind::Local), ty: *ty, addr: frame.wrapping_add(off),
                             call: Some(i), by: Some(i) });
                // (A local pointing at a struct: that too ("files->next_fd"
                // through `struct files_struct *files = current->files`).)
                if let Some(to) = types.struct_target(*ty) {
                    let v = self.rec.memory_at(self.now, end).read_u32(frame.wrapping_add(off));
                    add(Naming { what: var.clone(), within: Some(f.name.clone()), kind: kind(RegionKind::Object), ty: to, addr: v,
                                 call: Self::frame_of(calls, args, i, v), by: Some(i) });
                }
            }
            // (One the code keeps in a wasm local, pointing at a struct
            // (Record::Var): its last value; in the order the code first
            // sets them: the call's own filp names the file before an
            // inlined callee's file.)
            let mut values: Vec<(&Var, u32)> = Vec::new();
            for j in calls.vars_of(i, end) {
                if let Record::Var { var, value, .. } = records[j] && let Some(v) = t.vars.get(&(func, var)) {
                    match values.iter_mut().find(|(w, _)| w.function == v.function && w.name == v.name) {
                        Some(w) => w.1 = value as u32,
                        None => values.push((v, value as u32)),
                    }
                }
            }
            for (v, value) in values {
                if let Some(to) = self.var_ty(v).and_then(|ty| types.struct_target(ty)) {
                    add(Naming { what: v.name.clone(), within: Some(v.function.clone()), kind: kind(RegionKind::Object), ty: to, addr: value,
                                 call: Self::frame_of(calls, args, i, value), by: Some(i) });
                }
            }
        }
    }

    /// The call whose frame holds address `p`, seen from the call at
    /// record `i`: an object a pointer reaches on the stack lives as long
    /// as that call, the slot another's before and after; None: not on
    /// the stack.
    fn frame_of(calls: &stack::Calls, args: &crate::trace::Args, i: usize, p: u32) -> Option<usize> {
        let mut c = calls.call_from(i)?;
        loop {
            let parent = &calls.calls[c.parent? as usize];
            let (below, above) = (args.get(c.first)?.1, args.get(parent.first)?.1);
            if below <= p && p < above {
                return Some(c.first);
            }
            c = parent;
        }
    }
    /// Each store in records `first..=last` to the outermost region
    /// holding its first byte (a global, or an address nothing names, gets
    /// a region of its own).
    fn store_writes(&self, first: usize, last: usize, regions: &mut Vec<Region>) -> Vec<Wrote> {
        let (t, types) = (self.tables, &self.tables.types);
        let records = &self.rec.records;
        // (The kernel's globals, by address, for the stores nothing else holds.)
        let mut globals: Vec<(u32, u32, &str, u32)> = types.globals.iter()
            .map(|(name, &(addr, ty))| (addr, types.size_of(ty), name.as_str(), ty)).filter(|g| g.1 > 0).collect();
        globals.sort_unstable();
        // (The regions by address: a store's are those starting at most the
        // largest one's size below it.)
        let mut by_start: Vec<usize> = (0..regions.len()).collect();
        by_start.sort_unstable_by_key(|&k| regions[k].addr);
        let widest = regions.iter().map(|r| r.size).max().unwrap_or(0).max(1);
        // (Regions of their own, once each: a global, an address nothing names.)
        let mut own: BTreeMap<u32, u32> = BTreeMap::new();
        let mut writes = Vec::new();
        for &i in self.rec.stores_between(first, last) {
            let Record::Store { addr, .. } = records[i] else { continue };
            let end = by_start.partition_point(|&k| regions[k].addr <= addr);
            // (The outermost: an object inside another (a page's refcount,
            // the atomic_t a callee names) is the other's member; of equal
            // ones, the first named.)
            let hit = by_start[..end].iter().rev().take_while(|&&k| addr - regions[k].addr < widest).copied()
                .filter(|&k| addr < regions[k].addr.saturating_add(regions[k].size) && regions[k].life.is_none_or(|(a, b)| a <= i && i <= b))
                .min_by_key(|&k| (core::cmp::Reverse(regions[k].size), k));
            let region = match hit {
                Some(k) => k as u32,
                None => {
                    let r = match Self::global_at(&globals, addr) {
                        Some((gaddr, size, name, ty)) => {
                            let machine = types.declared.get(name).is_some_and(|(f, _)| f.starts_with(arch::MACHINE));
                            Region { what: name.into(), within: None, kind: if machine { RegionKind::Machine } else { RegionKind::Global }, ty: types.resolve(ty), addr: gaddr, size, life: None, depth: u32::MAX }
                        }
                        None => Region { what: format!("{addr:#x}"), within: None, kind: RegionKind::Other, ty: 0, addr, size: 0, life: None, depth: u32::MAX },
                    };
                    *own.entry(r.addr).or_insert_with(|| {
                        regions.push(r);
                        regions.len() as u32 - 1
                    })
                }
            };
            writes.push(Wrote { i, region, at: self.rec.line_at(t, i) });
        }
        writes
    }

    /// The global holding `addr` (globals do not overlap: the one starting
    /// at the nearest start at or below it), if one does.
    fn global_at<'a>(globals: &'a [(u32, u32, &'a str, u32)], addr: u32) -> Option<(u32, u32, &'a str, u32)> {
        // (Of the names starting there, ...: the sorted globals.)
        let k = globals.partition_point(|g| g.0 <= addr);
        let start = globals[..k].last()?.0;
        globals[..k].iter().rev().take_while(|g| g.0 == start).find(|g| addr < g.0.saturating_add(g.1)).copied()
    }

    /// Each variable the code keeps in a wasm local, as it sets it in
    /// records `first..=last`: from its value before, in its call, to its
    /// value then.
    fn var_writes(&self, first: usize, last: usize, regions: &mut Vec<Region>) -> Vec<Wrote> {
        let (t, calls, records) = (self.tables, &self.rec.calls, &self.rec.records);
        // (Each call's variable's region (none: of no type known).)
        let mut vars: BTreeMap<(usize, u32), Option<u32>> = BTreeMap::new();
        let mut writes = Vec::new();
        for i in first..=last {
            let Record::Var { var, .. } = records[i] else { continue };
            let Some(c) = calls.call_at(i) else { continue };
            let region = *vars.entry((c.first, var)).or_insert_with(|| {
                let v = t.vars.get(&(c.func, var));
                let r = v.and_then(|v| Some(Region { what: v.name.clone(), within: Some(v.function.clone()), kind: RegionKind::Var, ty: self.var_ty(v)?, addr: 0, size: 0, life: None, depth: c.depth }));
                r.map(|r| (regions.push(r), regions.len() as u32 - 1).1)
            });
            let Some(region) = region else { continue };
            writes.push(Wrote { i, region, at: self.rec.line_at(t, i) });
        }
        writes
    }
}
