//! What an ext4 disk keeps in a block, read from the disk as it is (its
//! superblock, group descriptors, inodes and their extents), each field
//! where the kernel's own types (DWARF's) say it is: the Disk tab's names
//! for a transfer's blocks.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::types::Types;
use crate::Memory;

/// Where the superblock is, in bytes.
const SUPERBLOCK: u32 = 1024;
/// The superblock's magic number (EXT4_SUPER_MAGIC).
const MAGIC: u32 = 0xef53;

/// A disk's layout: its block size, and each block that is not a file's
/// data, what it is; a file's (an inode's extents), whose.
pub struct Layout {
    pub block_size: u32,
    named: BTreeMap<u32, String>,
}

impl Layout {
    /// The layout of the ext4 disk `disk` (`len` bytes), as types `t` lay
    /// its structures out: none if it is not one (its magic number), or
    /// the types lack them.
    pub fn read(t: &Types, disk: &dyn Memory) -> Option<Layout> {
        let sb = |f: &str| t.field("ext4_super_block", f).map(|(off, size)| int(disk, SUPERBLOCK + off, size));
        if sb("s_magic")? != MAGIC as u64 {
            return None;
        }
        let block_size = 1024u32 << sb("s_log_block_size")?;
        let (first, per_group, inodes_per_group) = (sb("s_first_data_block")? as u32, sb("s_blocks_per_group")? as u32, sb("s_inodes_per_group")? as u32);
        let (blocks, inode_size) = (sb("s_blocks_count_lo")? as u32, sb("s_inode_size")? as u32);
        let desc_size = match sb("s_desc_size").unwrap_or(0) as u32 { 0 => 32, n => n };
        let groups = (blocks - first).div_ceil(per_group.max(1));
        let mut named = BTreeMap::new();
        named.insert(SUPERBLOCK / block_size, "the superblock".into());
        let gdt = first + 1;
        for b in gdt..gdt + (groups * desc_size).div_ceil(block_size) {
            named.insert(b, "the group descriptors".into());
        }
        // (Each group's bitmaps and inode table: where its descriptor says.)
        let gd = |g: u32, f: &str| t.field("ext4_group_desc", f).map(|(off, size)| int(disk, gdt * block_size + g * desc_size + off, size) as u32);
        let table_blocks = (inodes_per_group * inode_size).div_ceil(block_size);
        let mut tables = Vec::new();
        for g in 0..groups {
            let (Some(bb), Some(ib), Some(it)) = (gd(g, "bg_block_bitmap_lo"), gd(g, "bg_inode_bitmap_lo"), gd(g, "bg_inode_table_lo")) else { break };
            named.insert(bb, format!("group {g}'s block bitmap"));
            named.insert(ib, format!("group {g}'s inode bitmap"));
            for k in 0..table_blocks {
                let a = g * inodes_per_group + k * block_size / inode_size + 1;
                named.insert(it + k, format!("the inode table (inodes {a}-{})", a + block_size / inode_size - 1));
            }
            tables.push(it);
        }
        // (Each inode's blocks, by its extents (a tree of one level: a
        // small disk's): the journal's, a file's.)
        let journal = sb("s_journal_inum").unwrap_or(0) as u32;
        let inode = |ino: u32| -> Option<u32> {
            let (g, k) = ((ino - 1) / inodes_per_group, (ino - 1) % inodes_per_group);
            Some(tables.get(g as usize)? * block_size + k * inode_size)
        };
        let (i_mode, i_block) = (t.field("ext4_inode", "i_mode")?.0, t.field("ext4_inode", "i_block")?.0);
        let (eh_entries, eh_depth) = (t.field("ext4_extent_header", "eh_entries")?.0, t.field("ext4_extent_header", "eh_depth")?.0);
        let (ee_block, ee_len, ee_start) = (t.field("ext4_extent", "ee_block")?, t.field("ext4_extent", "ee_len")?, t.field("ext4_extent", "ee_start_lo")?);
        let extent = t.find("ext4_extent").map_or(12, |id| t.size_of(id));
        for ino in 1..=groups * inodes_per_group {
            let Some(at) = inode(ino) else { break };
            if int(disk, at + i_mode, 2) == 0 {
                continue;
            }
            let head = at + i_block;
            if int(disk, head, 2) != 0xf30a || int(disk, head + eh_depth, 2) != 0 {
                continue;
            }
            let what = if ino == journal { "the journal".into() } else { format!("inode {ino}'s data") };
            for e in 0..int(disk, head + eh_entries, 2) as u32 {
                let x = head + extent * (e + 1);
                let (_, len, start) = (int(disk, x + ee_block.0, ee_block.1), int(disk, x + ee_len.0, ee_len.1) as u32 & 0x7fff, int(disk, x + ee_start.0, ee_start.1) as u32);
                for b in start..start + len {
                    named.entry(b).or_insert_with(|| String::clone(&what));
                }
            }
        }
        Some(Layout { block_size, named })
    }

    /// What block `b` is: what the layout says, else free space (or a
    /// block nothing above names).
    pub fn name(&self, b: u32) -> String {
        self.named.get(&b).cloned().unwrap_or_else(|| "a block no file has".into())
    }
}

/// The little-endian number of `size` bytes (8 at most) at `addr`.
fn int(m: &dyn Memory, addr: u32, size: u32) -> u64 {
    let mut b = [0u8; 8];
    m.read(addr, &mut b[..size.min(8) as usize]);
    u64::from_le_bytes(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disk_s_blocks_named() {
        // (A 64-block disk of 1 KiB blocks, one group of 16 inodes of 128
        // bytes: the bitmaps at 3 and 4, the inode table at 5-6; inode 8 the
        // journal (blocks 20-21), inode 12 a file (block 30).)
        let t = Types::parse("T\t1\tbase\tu16\t2\tu\nT\t2\tbase\tu32\t4\tu\nT\t3\tstruct\t\t60\n\
            T\t10\tstruct\text4_super_block\t1024\nM\t10\ts_blocks_count_lo\t32\t0\t2\nM\t10\ts_first_data_block\t160\t0\t2\n\
            M\t10\ts_log_block_size\t192\t0\t2\nM\t10\ts_blocks_per_group\t256\t0\t2\nM\t10\ts_inodes_per_group\t320\t0\t2\n\
            M\t10\ts_magic\t448\t0\t1\nM\t10\ts_inode_size\t704\t0\t1\nM\t10\ts_journal_inum\t1792\t0\t2\n\
            T\t11\tstruct\text4_group_desc\t32\nM\t11\tbg_block_bitmap_lo\t0\t0\t2\nM\t11\tbg_inode_bitmap_lo\t32\t0\t2\nM\t11\tbg_inode_table_lo\t64\t0\t2\n\
            T\t12\tstruct\text4_inode\t128\nM\t12\ti_mode\t0\t0\t1\nM\t12\ti_block\t320\t0\t3\n\
            T\t13\tstruct\text4_extent_header\t12\nM\t13\teh_magic\t0\t0\t1\nM\t13\teh_entries\t16\t0\t1\nM\t13\teh_depth\t48\t0\t1\n\
            T\t14\tstruct\text4_extent\t12\nM\t14\tee_block\t0\t0\t2\nM\t14\tee_len\t32\t0\t1\nM\t14\tee_start_lo\t64\t0\t2\n");
        let mut d = alloc::vec![0u8; 64 * 1024];
        let put = |d: &mut Vec<u8>, at: usize, v: u32, n: usize| d[at..at + n].copy_from_slice(&v.to_le_bytes()[..n]);
        for (off, v, n) in [(4, 64, 4), (20, 1, 4), (24, 0, 4), (32, 8192, 4), (40, 16, 4), (56, 0xef53, 2), (88, 128, 2), (224, 8, 4)] {
            put(&mut d, 1024 + off, v, n);
        }
        for (off, v) in [(0, 3), (4, 4), (8, 5)] {
            put(&mut d, 2 * 1024 + off, v, 4);
        }
        // (An inode at the table, its extents: one, `len` blocks from `start`.)
        let mut inode = |d: &mut Vec<u8>, ino: usize, start: u32, len: u32| {
            let at = 5 * 1024 + (ino - 1) * 128;
            put(d, at, 0o100644, 2);
            put(d, at + 40, 0xf30a, 2);
            put(d, at + 42, 1, 2);
            put(d, at + 46, 0, 2);
            put(d, at + 40 + 12 + 4, len, 2);
            put(d, at + 40 + 12 + 8, start, 4);
        };
        inode(&mut d, 8, 20, 2);
        inode(&mut d, 12, 30, 1);
        let l = Layout::read(&t, &d).expect("an ext4 disk");
        let names: Vec<String> = [1, 2, 3, 4, 5, 6, 20, 21, 30, 40].iter().map(|&b| l.name(b)).collect();
        assert_eq!(names, ["the superblock", "the group descriptors", "group 0's block bitmap", "group 0's inode bitmap",
                           "the inode table (inodes 1-8)", "the inode table (inodes 9-16)", "the journal", "the journal",
                           "inode 12's data", "a block no file has"]);
        assert!(Layout::read(&t, &alloc::vec![0u8; 4096]).is_none(), "not ext4: no magic");
    }
}
