//! Tasks: what a task_struct pointer in the trace is.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::tables::Tables;
use crate::trace::Record;
use crate::Memory;

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub pid: i32,
    pub comm: String,
}

/// A task_struct's pid and comm.
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

/// The task pointers the records mention (current tasks, and arguments of
/// type task_struct*).
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
            Record::Block { .. } | Record::Return { .. } => {}
        }
    }
    tasks.remove(&0);
    tasks.into_keys().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{decode, tests::*};

    struct Mem(Vec<u8>);
    impl Memory for Mem {
        fn read(&self, addr: u32, buf: &mut [u8]) {
            buf.copy_from_slice(&self.0[addr as usize..addr as usize + buf.len()]);
        }
    }

    #[test]
    fn finds_and_reads_tasks() {
        let t = Tables::parse("F\t9\tf\tf.c:1\ttask_struct*\n", "T\t1\tbase\tint\t4\ts\nT\t2\tbase\tchar\t1\tc\n\
            T\t3\tarray\t\t16\t2\t16\nT\t4\tstruct\ttask_struct\t20\nM\t4\tpid\t0\t0\t1\nM\t4\tcomm\t32\t0\t3\n");
        let mut m = alloc::vec![0u8; 0x200];
        m[0x100..0x104].copy_from_slice(&7u32.to_le_bytes());
        m[0x104..0x108].copy_from_slice(b"ping");
        let records = decode(&[enter(9, 1, 0x100, [0x180, 0, 0, 0]), exit(9, 2, 0x100, 0)].concat());
        assert_eq!(tasks_in(&t, records), [0x100, 0x180]);
        assert_eq!(read_task(&t, &Mem(m), 0x100), Some(Task { pid: 7, comm: "ping".into() }));
    }
}
