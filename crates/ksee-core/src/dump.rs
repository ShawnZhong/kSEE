//! The trace dump: what the runtime saves so the trace can be read offline.
//!
//! ```text
//! "KSEEDUMP" u32 version
//! u32 n, u32 words[n]             the ring, oldest word first
//! u32 m, m * { u32 task, i32 pid, u8 comm[16] }
//! ```
//! (all little endian). Task pointers in the ring refer to the snapshot,
//! taken when the dump was made.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::tables::Tables;
use crate::trace::{self, Record};
use crate::Memory;

const MAGIC: &[u8; 8] = b"KSEEDUMP";
const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub pid: i32,
    pub comm: String,
}

#[derive(Debug, Default)]
pub struct Dump {
    pub words: Vec<u32>,
    pub tasks: BTreeMap<u32, Task>,
}

/// Read a task_struct's pid and comm.
pub fn read_task(t: &Tables, mem: &dyn Memory, p: u32) -> Option<Task> {
    let (pid, comm) = (t.field("task_struct", "pid")?, t.field("task_struct", "comm")?);
    if p == 0 {
        return None;
    }
    let mut name = alloc::vec![0; comm.size as usize];
    mem.read(p + comm.offset, &mut name);
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Some(Task {
        pid: mem.read_u32(p + pid.offset) as i32,
        comm: String::from_utf8_lossy(&name[..len]).into(),
    })
}

/// Task pointers the records mention.
pub fn tasks_in(t: &Tables, records: impl IntoIterator<Item = Record>) -> Vec<u32> {
    let mut tasks = BTreeMap::new();
    for r in records {
        match r {
            Record::Enter { func, current, args, .. } => {
                tasks.insert(current, ());
                if let Some(f) = t.functions.get(&func) {
                    for (ty, a) in f.arg_types.iter().zip(args) {
                        if ty == "task_struct*" {
                            tasks.insert(a, ());
                        }
                    }
                }
            }
            Record::Exit { current, .. } => {
                tasks.insert(current, ());
            }
            Record::Block { .. } => {}
        }
    }
    tasks.remove(&0);
    tasks.into_keys().collect()
}

/// Serialize the ring at `ring` and the tasks it mentions, in pieces, to
/// `sink` (without holding a copy of the ring).
pub fn write(t: &Tables, mem: &dyn Memory, ring: u32, sink: &mut dyn FnMut(&[u8])) {
    let (first, pairs) = trace::window(mem, ring, t.ring_words);
    let mut head = Vec::from(&MAGIC[..]);
    head.extend_from_slice(&VERSION.to_le_bytes());
    head.extend_from_slice(&(pairs * 2).to_le_bytes());
    sink(&head);
    // Oldest first: from pair `first` to the end of the ring, then the rest.
    let total = t.ring_words / 2;
    let mut chunk = alloc::vec![0u8; 64 << 10];
    let mut done = 0;
    while done < pairs {
        let slot = (first + done) % total;
        let n = (pairs - done).min(total - slot).min(chunk.len() as u32 / 8);
        let buf = &mut chunk[..n as usize * 8];
        mem.read(ring + 4 + slot * 8, buf);
        sink(buf);
        done += n;
    }
    let tasks: Vec<(u32, Task)> = tasks_in(t, trace::ring(mem, ring, t.ring_words)).into_iter()
        .filter_map(|p| Some((p, read_task(t, mem, p)?))).collect();
    let mut tail = Vec::from((tasks.len() as u32).to_le_bytes());
    for (p, task) in tasks {
        tail.extend_from_slice(&p.to_le_bytes());
        tail.extend_from_slice(&task.pid.to_le_bytes());
        let mut comm = [0u8; 16];
        let n = task.comm.len().min(16);
        comm[..n].copy_from_slice(&task.comm.as_bytes()[..n]);
        tail.extend_from_slice(&comm);
    }
    sink(&tail);
}

impl Dump {
    pub fn parse(buf: &[u8]) -> Result<Dump, &'static str> {
        let mut pos = 0;
        let mut take = |n: usize| -> Result<&[u8], &'static str> {
            let s = buf.get(pos..pos + n).ok_or("truncated dump")?;
            pos += n;
            Ok(s)
        };
        if take(8)? != MAGIC {
            return Err("not a kSEE dump");
        }
        let u32 = |b: &[u8]| u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        if u32(take(4)?) != VERSION {
            return Err("unsupported dump version");
        }
        let n = u32(take(4)?) as usize;
        let words = take(n * 4)?.chunks_exact(4).map(u32).collect();
        let mut tasks = BTreeMap::new();
        for _ in 0..u32(take(4)?) {
            let e = take(24)?;
            let len = e[8..].iter().position(|&c| c == 0).unwrap_or(16);
            tasks.insert(u32(&e[0..4]), Task {
                pid: u32(&e[4..8]) as i32,
                comm: String::from_utf8_lossy(&e[8..8 + len]).into(),
            });
        }
        Ok(Dump { words, tasks })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::tests::*;

    struct Mem(Vec<u8>);
    impl Memory for Mem {
        fn read(&self, addr: u32, buf: &mut [u8]) {
            buf.copy_from_slice(&self.0[addr as usize..addr as usize + buf.len()]);
        }
    }

    #[test]
    fn round_trips() {
        let t = Tables::parse("R\t16\n", "T\t1\tbase\tint\t4\ts\nT\t2\tbase\tchar\t1\tc\n\
            T\t3\tarray\t\t16\t2\t16\nT\t4\tstruct\ttask_struct\t20\nM\t4\tpid\t0\t0\t1\nM\t4\tcomm\t32\t0\t3\n");
        // A task at 0x100, and a 16-word ring at 0x200 holding one exit record.
        let mut m = alloc::vec![0u8; 0x300];
        m[0x100..0x104].copy_from_slice(&7u32.to_le_bytes());
        m[0x104..0x108].copy_from_slice(b"ping");
        let rec = exit(1, 5, 0x100, 0);
        m[0x200..0x204].copy_from_slice(&(rec.len() as u32).to_le_bytes());
        for (i, w) in rec.iter().enumerate() {
            m[0x204 + i * 4..0x208 + i * 4].copy_from_slice(&w.to_le_bytes());
        }
        let mut out = Vec::new();
        write(&t, &Mem(m), 0x200, &mut |b| out.extend_from_slice(b));
        let d = Dump::parse(&out).unwrap();
        assert_eq!(d.words, rec);
        assert_eq!(d.tasks[&0x100], Task { pid: 7, comm: "ping".into() });
    }
}
