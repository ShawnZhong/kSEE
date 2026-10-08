//! The tasks' open files: each fd table, a number to an open file each.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::View;
use crate::arch;
use crate::types::c_str;
use crate::Memory;

/// A number in an fd table and the open file it stands for (a struct
/// file): its address, where it is ("/note.txt"; a file of no place's, as
/// Linux names it: "pipefs:[12]"), its position, and its mode ("r", "w",
/// "rw"); and the other numbers that stand for it too, in this table or
/// another (`also`: each the table's index in those asked for, and its fd).
#[derive(Debug, Clone, PartialEq)]
pub struct OpenFile {
    pub fd: u32,
    pub file: u32,
    pub name: String,
    pub pos: i64,
    pub mode: String,
    pub also: Vec<(usize, u32)>,
}

impl View<'_> {
    /// The fd tables `tables` (files_structs, TaskAt::files), as memory was
    /// after record `at`: each its open files, by number.
    pub fn open_files(&self, tables: &[u32], at: usize) -> Vec<Vec<OpenFile>> {
        let mem = self.rec.memory_at(self.now, at);
        let mut all: Vec<Vec<OpenFile>> = tables.iter().map(|&t| self.table(&mem, t).unwrap_or_default()).collect();
        // (The numbers that stand for the same open file: each the others'.)
        let places: Vec<(usize, u32, u32)> = all.iter().enumerate().flat_map(|(k, t)| t.iter().map(move |f| (k, f.fd, f.file))).collect();
        for (k, table) in all.iter_mut().enumerate() {
            for f in table {
                f.also = places.iter().filter(|p| p.2 == f.file && (p.0, p.1) != (k, f.fd)).map(|p| (p.0, p.1)).collect();
            }
        }
        all
    }

    /// Fd table `files`' open files.
    fn table(&self, mem: &dyn Memory, files: u32) -> Option<Vec<OpenFile>> {
        let t = &self.tables.types;
        let field = |s: &str, f: &str| t.field(s, f).map(|(off, _)| off);
        let (fdt, fdtable, fd, max) = arch::FDTABLE;
        let table = mem.read_u32(files.wrapping_add(field(arch::TASK_FILES.1, fdt)?));
        let (array, max) = (mem.read_u32(table.wrapping_add(field(fdtable, fd)?)), mem.read_u32(table.wrapping_add(field(fdtable, max)?)));
        let (dentry, mnt, pos, mode, inode) = arch::FILE_FIELDS;
        let (dentry, mnt, pos, mode, inode) = (field(arch::FILE, dentry)?, field(arch::FILE, mnt)?, field(arch::FILE, pos)?, field(arch::FILE, mode)?, field(arch::FILE, inode)?);
        // (A torn table, as memory was: no more than a page's numbers.)
        Some((0..max.min(1024)).filter_map(|n| {
            let file = mem.read_u32(array.wrapping_add(4 * n));
            (file != 0).then(|| {
                let bits = mem.read_u32(file.wrapping_add(mode));
                let mode: String = arch::FMODE.iter().filter(|(b, _)| bits & b != 0).map(|(_, m)| *m).collect();
                let name = self.place(mem, mem.read_u32(file.wrapping_add(dentry)), mem.read_u32(file.wrapping_add(mnt)), mem.read_u32(file.wrapping_add(inode)));
                let p = (mem.read_u32(file.wrapping_add(pos)) as u64 | (mem.read_u32(file.wrapping_add(pos + 4)) as u64) << 32) as i64;
                OpenFile { fd: n, file, name, pos: p, mode, also: Vec::new() }
            })
        }).collect())
    }

    /// Where the dentry `dentry` in the mount whose vfsmount is at `vfsmount`
    /// is: its names up to the top mount's root ("/mnt/note.txt"). One that
    /// is no place's (its own parent, with no name: a pipe's), as Linux
    /// names it: its file system's type and its inode's number.
    fn place(&self, mem: &dyn Memory, dentry: u32, vfsmount: u32, inode: u32) -> String {
        let t = &self.tables.types;
        let field = |s: &str, f: &str| t.field(s, f).map_or(0, |(off, _)| off);
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
            let (s_type, fs_type, fs_name) = (arch::SUPER.1, arch::SUPER.2, arch::SUPER.3);
            let ty = mem.read_u32(mem.read_u32(dentry.wrapping_add(field(d, sb))).wrapping_add(field(arch::SUPER.0, s_type)));
            let fs = String::from_utf8_lossy(&c_str(mem, mem.read_u32(ty.wrapping_add(field(fs_type, fs_name))), 32)).into_owned();
            return format!("{fs}:[{}]", mem.read_u32(inode.wrapping_add(field(arch::INODE_NO.0, arch::INODE_NO.1))));
        }
        parts.reverse();
        format!("/{}", parts.join("/"))
    }
}
