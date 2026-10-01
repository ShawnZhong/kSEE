//! The views' tests, on made-up recordings.

use super::*;
use crate::trace::Args;

struct Mem(Vec<u8>);
impl Memory for Mem {
    fn read(&self, addr: u32, buf: &mut [u8]) {
        buf.copy_from_slice(&self.0[addr as usize..addr as usize + buf.len()]);
    }
}

/// A name as the page says it: "p in f".
fn named(what: &str, within: &Option<String>) -> String {
    within.as_ref().map_or(what.into(), |f| format!("{what} in {f}"))
}

/// Records `first..=last`'s writes, a member each: (record, what, kind, member, before, after).
fn writes(v: &View, first: usize, last: usize) -> Vec<(usize, String, RegionKind, String, String, String)> {
    let n = v.stores(first, last);
    n.writes.iter().flat_map(|w| {
        let r = &n.regions[w.region as usize];
        v.changes(&n, w).into_iter().map(|c| (w.i, named(&r.what, &r.within), r.kind, c.member, c.before, c.after))
    }).collect()
}

#[test]
fn a_variable_in_a_wasm_local_has_its_last_value() {
    // f (1) keeps int x (variable 0, a local: no offset) in a wasm
    // local: it takes 5, then 7. Before the first set, its value is not
    // recorded (not uninitialized: the trace starts after it was set).
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nV\t1\t0\t1\tf\tx\t0\t0\n", "T\t1\tbase\tint\t4\ts\nQ\t1\tx:1:\n");
    let r = [Record::Block { func: 1, n: 0 }, Record::Var { var: 0, line: 0, value: 5 }, Record::Block { func: 1, n: 1 }, Record::Var { var: 0, line: 0, value: 7 }];
    let mut args = Args::default();
    args.push(0, &[64]);
    let rec = Recording::new(&t, r.to_vec(), (0..4).collect(), args);
    let mem = Mem(alloc::vec![0; 128]);
    let v = View { tables: &t, rec: &rec, now: &mem };
    let x = |i| v.call_stack(i, None, &mem, i)[0].locals.clone().unwrap_or_default().into_iter().find(|o| o.name == "x").and_then(|o| o.value);
    assert_eq!((x(0), x(2), x(3)), (Some("(not recorded)".into()), Some("5".into()), Some("7".into())));
}

#[test]
fn a_memory_slot_never_stored_to_is_uninitialized() {
    // f (1) has int y at frame offset 4, never stored to: its slot holds
    // another call's leftovers, so it reads as uninitialized (not merely
    // unrecorded: memory starts unknown).
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n", "T\t1\tbase\tint\t4\ts\nQ\t1\ty:1:4\n");
    let r = [Record::Block { func: 1, n: 0 }];
    let mut args = Args::default();
    args.push(0, &[32]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0], args);
    let mem = Mem(alloc::vec![0; 128]);
    let v = View { tables: &t, rec: &rec, now: &mem };
    let y = v.call_stack(0, None, &mem, 0)[0].locals.clone().unwrap_or_default().into_iter().find(|o| o.name == "y").and_then(|o| o.value);
    assert_eq!(y, Some("(uninitialized)".into()));
}

#[test]
fn each_task_where_it_last_ran() {
    // Task 0x100 runs f (1), switches (__switch_to, 2) to 0x200, which
    // runs f.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\t__switch_to\t0:1\t3\n", "");
    let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }, Record::Return { func: 2, value: 0 }, Record::Block { func: 1, n: 0 }];
    let mut args = Args::default();
    args.push(1, &[0x100, 0x200, 0]);
    let rec = Recording::new(&t, r.to_vec(), (0..4).collect(), args);
    let v = View { tables: &t, rec: &rec, now: &Mem(alloc::vec![0; 16]) };
    let at = |i| v.tasks(i).into_iter().map(|x| (x.task.addr, x.state, x.last)).collect::<Vec<_>>();
    assert_eq!(at(0), [(0x100, TaskState::Running, Some(0)), (0x200, TaskState::Sleeping, None)]);
    assert_eq!(at(3), [(0x100, TaskState::Sleeping, Some(1)), (0x200, TaskState::Running, Some(3))]);
    assert_eq!(v.tasks(3)[0].func.as_deref(), Some("f"));
}

#[test]
fn the_tasks_are_the_kernels_list_then_the_recordings_others() {
    // init_task (0x100) and B (0x200, on the run queue) on the kernel's
    // list, a thread each (its signal_struct's list); 0x300 ran, and is not
    // on it: exited, once the recording goes on in B.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n", "T\t10\tstruct\ttask_struct\t24\nM\t10\ttasks\t0\t0\t11\nM\t10\tthread_node\t64\t0\t11\n\
        M\t10\tsignal\t128\t0\t15\nM\t10\ton_rq\t160\t0\t14\nT\t11\tstruct\tlist_head\t8\nM\t11\tnext\t0\t0\t13\nM\t11\tprev\t32\t0\t13\n\
        T\t12\tstruct\tsignal_struct\t8\nM\t12\tthread_head\t0\t0\t11\nT\t13\tptr\t\t4\t11\nT\t14\tbase\tu8\t1\tu\nT\t15\tptr\t\t4\t12\nG\tinit_task\t256\t10\n");
    let mut m = alloc::vec![0u8; 1024];
    let put = |m: &mut Vec<u8>, at: usize, v: u32| m[at..at + 4].copy_from_slice(&v.to_le_bytes());
    // (Each task: its tasks.next, its signal, and its signal's one thread, itself.)
    for (task, next, sig) in [(0x100, 0x200, 0x80), (0x200, 0x100, 0x90)] {
        put(&mut m, task, next as u32);
        put(&mut m, task + 16, sig as u32);
        put(&mut m, sig, (task + 8) as u32);
        put(&mut m, task + 8, sig as u32);
    }
    m[0x214] = 1;
    let r = [Record::Task { task: 0x300 }, Record::Block { func: 1, n: 0 }, Record::Task { task: 0x200 }, Record::Block { func: 1, n: 0 }];
    let rec = Recording::new(&t, r.to_vec(), (0..4).collect(), Args::default());
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    let at = |i| v.tasks(i).into_iter().map(|x| (x.task.addr, x.state, x.last)).collect::<Vec<_>>();
    assert_eq!(at(1), [(0x100, TaskState::Sleeping, None), (0x200, TaskState::Runnable, None), (0x300, TaskState::Running, Some(1))]);
    assert_eq!(at(3), [(0x100, TaskState::Sleeping, None), (0x200, TaskState::Running, Some(3)), (0x300, TaskState::Exited, Some(1))]);
}

#[test]
fn a_store_is_a_change_to_its_member() {
    // struct s { int a; int b; } at 4 (f's argument p): record 1 stores
    // b (2, was 1), record 3 a byte of a (5): each a change to its member.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t9\tf\t0:1\t0\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t9\tf\tp:3\n");
    let r = [Record::Block { func: 9, n: 0 }, Record::Store { addr: 8, size: 4, old: Some(1), line: 0, from: None },
             Record::Block { func: 9, n: 1 }, Record::Store { addr: 5, size: 1, old: Some(0), line: 0, from: None }];
    let mut args = Args::default();
    args.push(0, &[4, 64]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 4], args);
    let mut m = alloc::vec![0u8; 16];
    (m[8], m[5]) = (2, 5);
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    let got: Vec<_> = writes(&v, 0, 3).into_iter().map(|(i, _, _, member, before, after)| (i, member, before, after)).collect();
    assert_eq!(got, [(1, "b".into(), "1".into(), "2".into()), (3, "a".into(), "0".into(), "1280".into())]);
}

#[test]
fn one_store_copying_a_struct_changes_each_member() {
    // f (1) takes struct path { void *mnt; void *dentry; } *p (at 8) and
    // copies one into it with one 8-byte store, on line 3 (its block's
    // second): mnt 0 → 5, dentry 0 → 6, in the Objects and the line's note.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nL\t1\t0\t0:1;:3\n",
                          "T\t1\tptr\t\t4\t0\nT\t2\tstruct\tpath\t8\nM\t2\tmnt\t0\t0\t1\nM\t2\tdentry\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t1\tf\tp:3\n");
    let r = [Record::Block { func: 1, n: 0 }, Record::Store { addr: 8, size: 8, old: Some(0), line: 1, from: None }];
    let mut args = Args::default();
    args.push(0, &[8, 64]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 2], args);
    let mut m = alloc::vec![0u8; 32];
    (m[8], m[12]) = (5, 6);
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    let nodes = v.objects(0, 1);
    let got: Vec<_> = nodes[0].changes.iter().map(|d| (d.member.clone(), d.after.clone())).collect();
    assert_eq!(got, [("mnt".into(), "0x5".into()), ("dentry".into(), "0x6".into())]);
    let notes = v.notes(0, 1, "f.c", 1);
    assert_eq!((notes[0].line, notes[0].short.as_str()), (3, "mnt: NULL → 0x5   dentry: NULL → 0x6"));
    assert_eq!(notes[0].full.as_str(), "p->mnt in f: NULL → 0x5; p->dentry in f: NULL → 0x6");
}

#[test]
fn a_lines_note_says_a_name_once() {
    // f (1) calls g (2) twice, each with its own struct s *p (at 4, then
    // 16), which stores p->b on line 3: 1 → 2, then 3 → 4. The line says
    // b once, both changes after it.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:2\t0\nL\t2\t0\t0:2;:3\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t2\tg\tp:3\n");
    let st = |addr, old| Record::Store { addr, size: 4, old: Some(old), line: 1, from: None };
    let ret = Record::Return { func: 2, value: 0 };
    let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }, st(8, 1), ret, Record::Block { func: 2, n: 0 }, st(20, 3), ret];
    let mut args = Args::default();
    args.push(1, &[4, 64]);
    args.push(4, &[16, 64]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 7], args);
    let mut m = alloc::vec![0u8; 32];
    (m[8], m[20]) = (2, 4);
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    let notes = v.notes(0, 6, "f.c", 6);
    assert_eq!((notes[0].line, notes[0].short.as_str()), (3, "b: 1 → 2, 3 → 4"));
    assert_eq!(notes[0].full.as_str(), "p->b in g: 1 → 2; p->b in g: 3 → 4");
}

#[test]
fn a_calls_stores_are_named_as_the_code_names_them() {
    // f (1, "f") takes struct s *p (at 4) and has a local int x (at
    // frame 32 + 4): it stores p->b, then x; calls g (2), whose frame is
    // at 16, which stores a byte of its own frame that no variable of its
    // names; and one byte nobody names: both by their addresses.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\n\
                           P\t1\tf\tp:3\nQ\t1\tx:1:4\n");
    let r = [Record::Block { func: 1, n: 0 },
             Record::Store { addr: 8, size: 4, old: Some(1), line: 0, from: None }, Record::Store { addr: 36, size: 4, old: Some(0), line: 0, from: None },
             Record::Block { func: 2, n: 0 },
             Record::Store { addr: 20, size: 1, old: Some(0), line: 0, from: None }, Record::Return { func: 2, value: 0 },
             Record::Store { addr: 100, size: 1, old: Some(0), line: 0, from: None }, Record::Store { addr: 21, size: 1, old: Some(0), line: 0, from: None }];
    let mut args = Args::default();
    args.push(0, &[4, 32]);
    args.push(3, &[16]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 8], args);
    let mut m = alloc::vec![0u8; 128];
    (m[8], m[36], m[20], m[100]) = (2, 7, 9, 5);
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    let got: Vec<_> = writes(&v, 0, 7).into_iter().map(|(_, what, kind, member, _, after)| (what, kind, member, after)).collect();
    assert_eq!(got, [("p in f".into(), RegionKind::Object, "b".into(), "2".into()), ("x in f".into(), RegionKind::Local, "".into(), "7".into()),
                     ("0x14".into(), RegionKind::Other, "".into(), "9".into()), ("0x64".into(), RegionKind::Other, "".into(), "5".into()),
                     ("0x15".into(), RegionKind::Other, "".into(), "0".into())]);
}

#[test]
fn a_store_through_a_pointer_in_a_wasm_local_is_named_by_it() {
    // f (1) keeps struct s *q (variable 0) in a wasm local, 4 (a set,
    // itself one of the call's stores); it stores q->b.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nV\t1\t0\t1\tf\tq\t0\t0\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nQ\t1\tq:3:\n");
    let r = [Record::Block { func: 1, n: 0 }, Record::Var { var: 0, line: 0, value: 4 }, Record::Store { addr: 8, size: 4, old: Some(1), line: 0, from: None }];
    let mut args = Args::default();
    args.push(0, &[64]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
    let mut m = alloc::vec![0u8; 128];
    m[8] = 2;
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    let got: Vec<_> = writes(&v, 0, 2).into_iter().map(|(_, what, _, member, _, after)| (what, member, after)).collect();
    assert_eq!(got, [("q in f".into(), "".into(), "0x4".into()), ("q in f".into(), "b".into(), "2".into())]);
}

#[test]
fn the_objects_a_call_wrote_are_a_graph() {
    // f (1) takes struct s *p (at 4) and struct t *q (at 32: int c, then
    // struct s *link): it stores p->b, q->c, and q->link, pointing at
    // *p: two nodes, p under q, by its link.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\n\
                           T\t4\tstruct\tt\t8\nM\t4\tc\t0\t0\t1\nM\t4\tlink\t32\t0\t3\nT\t5\tptr\t\t4\t4\nP\t1\tf\tp:3,q:5\n");
    let st = |addr, old| Record::Store { addr, size: 4, old: Some(old), line: 0, from: None };
    let r = [Record::Block { func: 1, n: 0 }, st(8, 1), st(32, 0), st(36, 0)];
    let mut args = Args::default();
    args.push(0, &[4, 32, 64]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 4], args);
    let mut m = alloc::vec![0u8; 64];
    (m[8], m[32], m[36]) = (2, 7, 4);
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    let nodes = v.objects(0, 3);
    let got: Vec<_> = nodes.iter().map(|n| (named(&n.what, &n.within), n.changes.iter().map(|d| (d.member.clone(), format!("{} → {}", d.before, d.after), d.to)).collect::<Vec<_>>())).collect();
    // (As a tree, in the order shown: q, then p under it, by its link.)
    assert_eq!(got, [("q in f".into(), alloc::vec![("c".into(), "0 → 7".into(), None), ("link".into(), "NULL → 0x4".into(), Some(1))]),
                     ("p in f".into(), alloc::vec![("b".into(), "1 → 2".into(), None)])]);
    assert_eq!(nodes.iter().map(|n| (n.under, n.via.clone())).collect::<Vec<_>>(), [(None, None), (Some(0), Some("link".into()))]);
}

#[test]
fn another_tasks_objects_come_after_the_spans_own() {
    // Task 0's f (1) takes struct s *p (at 4) and stores p->b; while it
    // runs, task 40's g (2) takes struct s *q (at 16) and stores q->b
    // first: f's p first, then g's q, by task 40.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:2\t0\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t1\tf\tp:3\nP\t2\tg\tq:3\n");
    let st = |addr, old| Record::Store { addr, size: 4, old: Some(old), line: 0, from: None };
    let r = [Record::Block { func: 1, n: 0 }, Record::Task { task: 40 }, Record::Block { func: 2, n: 0 }, st(20, 3), Record::Task { task: 0 }, st(8, 1)];
    let mut args = Args::default();
    args.push(0, &[4, 64]);
    args.push(2, &[16, 64]);
    let rec = Recording::new(&t, r.to_vec(), (0..6).collect(), args);
    let mut m = alloc::vec![0u8; 64];
    (m[8], m[20]) = (2, 4);
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    let got: Vec<_> = v.objects(0, 5).into_iter().map(|n| (named(&n.what, &n.within), n.by.map(|t| t.addr))).collect();
    assert_eq!(got, [("p in f".into(), None), ("q in g".into(), Some(40))]);
}

#[test]
fn a_calls_locals_are_not_objects() {
    // f (1) stores its local int x (at frame 32 + 4): the Stack's, not
    // an object, before it returns as after.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n", "T\t1\tbase\tint\t4\ts\nQ\t1\tx:1:4\n");
    let r = [Record::Block { func: 1, n: 0 }, Record::Store { addr: 36, size: 4, old: Some(0), line: 0, from: None }, Record::Return { func: 1, value: 0 }];
    let mut args = Args::default();
    args.push(0, &[32]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
    let mut m = alloc::vec![0u8; 64];
    m[36] = 7;
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    assert!(v.objects(0, 2).is_empty() && v.objects(0, 1).is_empty());
}

#[test]
fn an_object_inside_another_is_its_member() {
    // f (1) takes struct u *p (at 4: int a, then struct s inner); g
    // (2), which f calls, takes struct s *q (p's inner, at 8) and stores
    // q->b: one object, p in f, its inner.b changed.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\n\
                           T\t4\tstruct\tu\t12\nM\t4\ta\t0\t0\t1\nM\t4\tinner\t32\t0\t2\nT\t5\tptr\t\t4\t4\nP\t1\tf\tp:5\nP\t2\tg\tq:3\n");
    let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }, Record::Store { addr: 12, size: 4, old: Some(0), line: 0, from: None }];
    let mut args = Args::default();
    args.push(0, &[4, 64]);
    args.push(1, &[8, 48]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
    let mut m = alloc::vec![0u8; 128];
    m[12] = 3;
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    let got: Vec<_> = v.objects(0, 2).into_iter().map(|n| (named(&n.what, &n.within), n.changes.into_iter().map(|d| (d.member, format!("{} → {}", d.before, d.after))).collect::<Vec<_>>())).collect();
    assert_eq!(got, [("p in f".into(), alloc::vec![("inner.b".into(), "0 → 3".into())])]);
}

#[test]
fn two_objects_of_one_name_are_numbered() {
    // f (1), called twice, takes struct s *p (at 4, then at 20) and
    // stores p->b: two objects, "p in f" and "p in f #2".
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t1\tf\tp:3\n");
    let st = |addr| Record::Store { addr, size: 4, old: Some(0), line: 0, from: None };
    let r = [Record::Block { func: 1, n: 0 }, st(8), Record::Return { func: 1, value: 0 }, Record::Block { func: 1, n: 0 }, st(24), Record::Return { func: 1, value: 0 }];
    let mut args = Args::default();
    args.push(0, &[4, 64]);
    args.push(3, &[20, 64]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 6], args);
    let mut m = alloc::vec![0u8; 128];
    (m[8], m[24]) = (1, 2);
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    let got: Vec<_> = v.objects(0, 5).into_iter().map(|n| named(&n.what, &n.within)).collect();
    assert_eq!(got, ["p in f", "p #2 in f"]);
}

#[test]
fn a_members_changes_are_a_run_a_function() {
    // f (1) takes struct s *p (at 4) and stores p->b 2 (was 1); g (2),
    // which it calls with p, stores it back to 1: b left as it was, f's
    // run 1 → 2, then g's 2 → 1.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:5\t0\nL\t1\t0\t0:1\nL\t2\t0\t0:5\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t1\tf\tp:3\nP\t2\tg\tp:3\n");
    let st = |old| Record::Store { addr: 8, size: 4, old: Some(old), line: 0, from: None };
    let r = [Record::Block { func: 1, n: 0 }, st(1), Record::Block { func: 2, n: 0 }, st(2), Record::Return { func: 2, value: 0 }];
    let mut args = Args::default();
    args.push(0, &[4, 64]);
    args.push(2, &[4, 48]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 5], args);
    let mut m = alloc::vec![0u8; 128];
    m[8] = 1;
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    let nodes = v.objects(0, 4);
    let d = &nodes[0].changes[0];
    let runs: Vec<_> = d.runs.iter().map(|r| (r.at.as_ref().map(|l| l.func.clone()), r.before.clone(), r.after.clone(), r.last)).collect();
    assert_eq!((named(&nodes[0].what, &nodes[0].within).as_str(), d.member.as_str(), d.before.as_str(), d.after.as_str()), ("p in f", "b", "1", "1"));
    // (Left as it was, in the end: an object of the "same" group.)
    assert_eq!(nodes[0].group, Group::Same);
    assert_eq!(runs, [(Some("f".into()), "1".into(), "2".into(), 1), (Some("g".into()), "2".into(), "1".into(), 3)]);
}

#[test]
fn a_member_one_function_writes_back_is_no_change() {
    // f (1) takes struct s *p (at 4): it stores p->b 2 (was 1), then 1
    // again: b left as it was, by the one function: no change, no object.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t1\tf\tp:3\n");
    let st = |old| Record::Store { addr: 8, size: 4, old: Some(old), line: 0, from: None };
    let r = [Record::Block { func: 1, n: 0 }, st(1), st(2)];
    let mut args = Args::default();
    args.push(0, &[4, 32]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
    let mut m = alloc::vec![0u8; 64];
    m[8] = 1;
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    assert!(v.objects(0, 2).is_empty());
}

#[test]
fn a_value_is_followed_through_its_copies() {
    // 7 stored at 0x30 (computed), then copied to 0x10 (a load, a store),
    // then to 0x20 by a memcpy of 8 (two stores): 0x24's way back is
    // 0x24 <- 0x14 <- ... 0x14 was never written; 0x20's is 0x20 <- 0x10 <- 0x30.
    let t = Tables::default();
    let st = |addr, from| Record::Store { addr, size: 4, old: Some(0), line: 0, from: core::num::NonZeroU32::new(from) };
    let r = [st(0x30, 0), st(0x10, 0x30), st(0x20, 0x10), st(0x24, 0x14)];
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 4], Args::default());
    let v = View { tables: &t, rec: &rec, now: &Mem(alloc::vec![0; 64]) };
    let way = |a| v.origin(a, 4, 3, 8).iter().map(|w| (w.i, w.addr, w.from)).collect::<Vec<_>>();
    assert_eq!(way(0x20), [(2, 0x20, Some(0x10)), (1, 0x10, Some(0x30)), (0, 0x30, None)]);
    assert_eq!(way(0x24), [(3, 0x24, Some(0x14))]);
}

#[test]
fn frames_are_where_their_calls_are() {
    // f (1) runs lines 1 and 2, calls g (2) at 2, then runs 3.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\n\
                           L\t1\t0\t0:1;:2;|;:3\nL\t2\t0\t0:10\n", "");
    let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }];
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0, 1], Args::default());
    let v = View { tables: &t, rec: &rec, now: &Mem(Vec::new()) };
    let lines: Vec<_> = v.call_stack(1, None, &Mem(Vec::new()), 1).iter().map(|f| (f.func.clone(), f.line)).collect();
    assert_eq!(lines, [("g".into(), Some(10)), ("f".into(), Some(2))]);
}

#[test]
fn inlined_code_is_a_frame_of_its_own() {
    // f (1) runs line 1, then h's line 20 (h inlined into f at line 2),
    // which calls g (2).
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\n\
                           I\t0\th\t0:2\nL\t1\t0\t0:1;:20@0;|\nL\t2\t0\t0:10\n", "");
    let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }];
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0, 1], Args::default());
    let v = View { tables: &t, rec: &rec, now: &Mem(Vec::new()) };
    let frames: Vec<_> = v.call_stack(1, None, &Mem(Vec::new()), 1).iter().map(|f| (f.func.clone(), f.line, f.inlined)).collect();
    assert_eq!(frames, [("g".into(), Some(10), false), ("h".into(), Some(20), true), ("f".into(), Some(2), false)]);
    // (Line 2, where h is inlined, ran too: h's line 20 ran for it.)
    assert!(v.coverage("f.c", None).iter().any(|l| l.line == 2));
    // At record 0, its first line (nth 0): f at line 1, nothing inlined.
    let first: Vec<_> = v.call_stack(0, Some(0), &Mem(Vec::new()), 0).iter().map(|f| (f.func.clone(), f.line)).collect();
    assert_eq!(first, [("f".into(), Some(1))]);
}

#[test]
fn char_pointers_show_their_strings() {
    let t = Types::parse("T\t1\tbase\tchar\t1\tc\nT\t2\tptr\t\t4\t1\n");
    let mut m = alloc::vec![0u8; 128];
    m[0..4].copy_from_slice(&8u32.to_le_bytes());
    m[8..12].copy_from_slice(b"tmp\0");
    m[4..8].copy_from_slice(&16u32.to_le_bytes()); // (to bytes not printable: escaped, as C writes them)
    m[16] = 0xff;
    let value = |at| t.object(&Mem(m.clone()), "p", at, 2, 0).value.unwrap();
    assert_eq!((value(0).as_str(), value(4).as_str()), ("0x8 → \"tmp\"", "0x10 → \"\\xff\""));
}

#[test]
fn scalar_pointers_show_what_they_point_to() {
    let t = Types::parse("T\t1\tbase\tint\t4\ts\nT\t2\tptr\t\t4\t1\n");
    let mut m = alloc::vec![0u8; 16];
    m[0..4].copy_from_slice(&8u32.to_le_bytes());
    m[8..12].copy_from_slice(&(-3i32).to_le_bytes());
    let o = t.object(&Mem(m), "pos", 0, 2, 0);
    assert_eq!((o.value.as_deref(), o.open), (Some("0x8 → -3"), None));
}

#[test]
fn objects_expose_pointer_targets() {
    let t = Types::parse("T\t1\tbase\tint\t4\ts\nT\t2\tstruct\tnode\t8\nM\t2\tv\t0\t0\t1\n\
                          M\t2\tnext\t32\t0\t3\nT\t3\tptr\t\t4\t2\n");
    let mut m = alloc::vec![0u8; 16];
    m[0..4].copy_from_slice(&7i32.to_le_bytes());
    m[4..8].copy_from_slice(&8u32.to_le_bytes());
    let o = t.object(&Mem(m), "n", 0, 2, 1);
    let c = o.children.unwrap();
    assert_eq!((c[0].value.as_deref(), c[1].value.as_deref(), c[1].open), (Some("7"), Some("0x8"), Some(Target { ty: 2, addr: 8 })));
}

#[test]
fn a_member_of_size_0_is_not_shown() {
    // struct s { int a; struct e {} e; }: the config left e empty, nothing in it.
    let t = Tables::parse("U\n", "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t4\nM\t2\ta\t0\t0\t1\nM\t2\te\t32\t0\t3\nT\t3\tstruct\te\t0\n");
    let o = t.types.object(&Mem(alloc::vec![0u8; 8]), "x", 0, 2, 1);
    assert_eq!(o.children.unwrap().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a"]);
}

#[test]
fn a_long_array_says_how_many_more() {
    // int a[100]: the first 64, then "… 36 more".
    let t = Tables::parse("U\n", "T\t1\tbase\tint\t4\ts\nT\t2\tarray\t\t400\t1\t100\n");
    let c = t.types.object(&Mem(alloc::vec![0u8; 400]), "a", 0, 2, 1).children.unwrap();
    assert_eq!((c.len(), c[64].name.as_str(), c[64].value.as_deref()), (65, "[64…99]", Some("… 36 more")));
}

#[test]
fn a_task_is_named_as_it_was_then() {
    // task_struct { int pid; char comm[8]; } at 0x40, pid 19: renamed from
    // "prog" to "hi" at record 2 (a store of comm's first 4 bytes).
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n", "T\t1\tbase\tint\t4\ts\nT\t2\tbase\tchar\t1\tc\nT\t3\tarray\t\t8\t2\t8\n\
        T\t4\tstruct\ttask_struct\t12\nM\t4\tpid\t0\t0\t1\nM\t4\tcomm\t32\t0\t3\n");
    let r = [Record::Task { task: 0x40 }, Record::Block { func: 1, n: 0 },
             Record::Store { addr: 0x44, size: 4, old: Some(u32::from_le_bytes(*b"prog") as u64), line: 0, from: None }, Record::Block { func: 1, n: 0 }];
    let rec = Recording::new(&t, r.to_vec(), (0..4).collect(), Args::default());
    let mut m = alloc::vec![0u8; 128];
    m[0x40] = 19;
    m[0x44..0x47].copy_from_slice(b"hi\0");
    let v = View { tables: &t, rec: &rec, now: &Mem(m) };
    let name = |i| v.task(0x40, i).comm;
    assert_eq!((name(1), name(3), v.task(0x40, 3).pid), (Some("prog".into()), Some("hi".into()), Some(19)));
}

#[test]
fn a_chart_folds_into_runs() {
    // Three columns: bars [0] / [0, 1] / [1], tasks 7, 7, 8.
    let chart = Chart {
        bars: Vec::new(),
        cols: alloc::vec![
            Column { i: 0, task: 7, frames: alloc::vec![0] },
            Column { i: 1, task: 7, frames: alloc::vec![0, 1] },
            Column { i: 2, task: 8, frames: alloc::vec![1] },
        ],
    };
    // (Each row's runs: row 0 has bar 0 over columns 0..2, then bar 1 at
    // column 2; row 1 has bar 1 at column 1 only.)
    assert_eq!(chart.bar_runs(), alloc::vec![(0, 0, 2, 0), (0, 2, 3, 1), (1, 1, 2, 1)]);
    assert_eq!(chart.task_runs(), alloc::vec![(0, 2, 7), (2, 3, 8)]);
    // (No columns, no runs.)
    assert!(Chart::default().bar_runs().is_empty() && Chart::default().task_runs().is_empty());
}

#[test]
fn steps_go_into_over_or_out() {
    // f (depth 0) line 1, calls g (depth 1): lines 10, 11; then f's line 2.
    let line = |depth, line| Line { i: 0, at: 0, nth: 0, func: String::new(), file: String::new(), line, inlined: None, depth, inl: 0, run: 1,
                                   tests: Vec::new(), ret: None, called: None, site: None };
    let lines = [line(0, 1), line(1, 10), line(1, 11), line(0, 2)];
    assert_eq!((step(&lines, 0, 1, Move::Into), step(&lines, 0, 1, Move::Over), step(&lines, 1, 1, Move::Out)), (Some(1), Some(3), Some(3)));
    assert_eq!((step(&lines, 3, -1, Move::Over), step(&lines, 2, -1, Move::Out), step(&lines, 0, -1, Move::Into)), (Some(0), Some(0), None));
    assert_eq!(step(&lines, 3, 1, Move::Into), None);
}

#[test]
fn the_raw_trace_is_a_line_a_record() {
    // f (1) is called (its arguments: none; its frame at 64), keeps x = 5
    // in a wasm local, runs its block 1, and returns 3.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nV\t1\t0\t1\tf\tx\t0\t0\n", "T\t1\tbase\tint\t4\ts\nQ\t1\tx:1:\n");
    let r = [Record::Block { func: 1, n: 0 }, Record::Var { var: 0, line: 0, value: 5 }, Record::Block { func: 1, n: 1 }, Record::Return { func: 1, value: 3 }];
    let mut args = Args::default();
    args.push(0, &[64]);
    let rec = Recording::new(&t, r.to_vec(), (0..4).collect(), args);
    let v = View { tables: &t, rec: &rec, now: &Mem(alloc::vec![0; 128]) };
    let raw = v.raw(0, 3);
    let kinds: Vec<RecordKind> = raw.iter().map(|r| r.1).collect();
    assert_eq!(kinds, [RecordKind::Call, RecordKind::Var, RecordKind::Block, RecordKind::Return]);
    assert!(raw[0].2.starts_with("call f(") && raw[0].2.ends_with("frame=0x40"), "{}", raw[0].2);
    assert_eq!((raw[1].2.as_str(), raw[3].2.as_str()), ("var x = 5", "return f = 3"));
    assert!(raw[2].2.starts_with("block f #1"), "{}", raw[2].2);
}

#[test]
fn a_context_switch_names_both_tasks() {
    // f (1) runs on 0x100, switches (__switch_to, 2) to 0x200, which runs
    // f: every record of the switch's call names prev and next; outside it,
    // none.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\t__switch_to\t0:1\t3\n", "");
    let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }, Record::Return { func: 2, value: 0 }, Record::Block { func: 1, n: 0 }];
    let mut args = Args::default();
    args.push(1, &[0x100, 0x200, 0]);
    let rec = Recording::new(&t, r.to_vec(), (0..4).collect(), args);
    let v = View { tables: &t, rec: &rec, now: &Mem(alloc::vec![0; 16]) };
    let at = |i| v.switch_at(i).map(|(p, n)| (p.addr, n.addr));
    assert_eq!((at(0), at(1), at(2), at(3)), (None, Some((0x100, 0x200)), Some((0x100, 0x200)), None));
}
