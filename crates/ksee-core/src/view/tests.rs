//! The views' tests, on made-up recordings.

use super::*;
use crate::trace::Args;


/// A name as the page says it: "p in f".
fn named(what: &str, within: &Option<String>) -> String {
    within.as_ref().map_or(what.into(), |f| format!("{what} in {f}"))
}

/// Records `first..=last`'s writes, a member each: (record, what, kind, member, before, after).
fn writes(v: &View, first: usize, last: usize) -> Vec<(usize, String, RegionKind, String, String, String)> {
    let n = v.stores(first, last);
    let mut out: Vec<_> = n.things.iter().flat_map(|t| {
        let r = &n.regions[t.region as usize];
        (0..t.writes.len()).map(|k| {
            let (before, after) = v.change(&n, t, k);
            (t.writes[k].i, named(&r.what, &r.within), r.kind, t.member.clone(), before, after)
        }).collect::<Vec<_>>()
    }).collect();
    out.sort_by_key(|w| w.0);
    out
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
    let mem = alloc::vec![0; 128];
    let v = View { tables: &t, rec: &rec, now: &mem };
    let x = |i| v.call_stack(i, None, 0, i)[0].locals.clone().into_iter().find(|o| o.name == "x").and_then(|o| o.value);
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
    let mem = alloc::vec![0; 128];
    let v = View { tables: &t, rec: &rec, now: &mem };
    let y = v.call_stack(0, None, 0, 0)[0].locals.clone().into_iter().find(|o| o.name == "y").and_then(|o| o.value);
    assert_eq!(y, Some("(uninitialized)".into()));
}

#[test]
fn each_task_where_it_last_ran() {
    // Task 0x100 runs f (1), switches (__switch_to, 2) to 0x200, which
    // runs f.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\t__switch_to\t0:1\t3\n", "");
    let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }, Record::Return { func: 2, value: Some(0) }, Record::Block { func: 1, n: 0 }];
    let mut args = Args::default();
    args.push(1, &[0x100, 0x200, 0]);
    let rec = Recording::new(&t, r.to_vec(), (0..4).collect(), args);
    let v = View { tables: &t, rec: &rec, now: &alloc::vec![0; 16] };
    let at = |i| v.tasks(i).into_iter().map(|x| (x.task.addr, x.state)).collect::<Vec<_>>();
    assert_eq!(at(0), [(0x100, TaskState::Running), (0x200, TaskState::Sleeping)]);
    assert_eq!(at(3), [(0x100, TaskState::Sleeping), (0x200, TaskState::Running)]);
    assert_eq!(v.tasks(3)[0].doing.as_deref(), Some("f"));
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
    let v = View { tables: &t, rec: &rec, now: &m };
    let at = |i| v.tasks(i).into_iter().map(|x| (x.task.addr, x.state)).collect::<Vec<_>>();
    assert_eq!(at(1), [(0x100, TaskState::Sleeping), (0x200, TaskState::Runnable), (0x300, TaskState::Running)]);
    assert_eq!(at(3), [(0x100, TaskState::Sleeping), (0x200, TaskState::Running), (0x300, TaskState::Exited)]);
}

#[test]
fn a_store_is_a_change_to_its_member() {
    // struct s { int a; int b; } at 4 (f's argument p): record 1 stores
    // b (2, was 1), record 3 a byte of a (5): each a change to its member.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t9\tf\t0:1\t0\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t9\tf\tp:3\n");
    let r = [Record::Block { func: 9, n: 0 }, Record::Store { addr: 8, size: 4, old: 1, line: 0, from: None },
             Record::Block { func: 9, n: 1 }, Record::Store { addr: 5, size: 1, old: 0, line: 0, from: None }];
    let mut args = Args::default();
    args.push(0, &[4, 64]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 4], args);
    let mut m = alloc::vec![0u8; 16];
    (m[8], m[5]) = (2, 5);
    let v = View { tables: &t, rec: &rec, now: &m };
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
    let r = [Record::Block { func: 1, n: 0 }, Record::Store { addr: 8, size: 8, old: 0, line: 1, from: None }];
    let mut args = Args::default();
    args.push(0, &[8, 64]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 2], args);
    let mut m = alloc::vec![0u8; 32];
    (m[8], m[12]) = (5, 6);
    let v = View { tables: &t, rec: &rec, now: &m };
    let nodes = v.objects(0, 1);
    let got: Vec<_> = nodes[0].changes.iter().map(|d| (d.member.clone(), d.after.clone())).collect();
    assert_eq!(got, [("mnt".into(), "0x5".into()), ("dentry".into(), "0x6".into())]);
    let notes = v.notes(0, 1, "f.c", 1);
    assert_eq!((notes[0].line, notes[0].short.as_str()), (3, "mnt = NULL → 0x5   dentry = NULL → 0x6"));
    assert_eq!(notes[0].full.as_str(), "p->mnt in f = NULL → 0x5; p->dentry in f = NULL → 0x6");
}

#[test]
fn a_lines_note_says_a_name_once() {
    // f (1) calls g (2) twice, each with its own struct s *p (at 4, then
    // 16), which stores p->b on line 3: 1 → 2, then 3 → 4. The line says
    // b once, both changes after it.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:2\t0\nL\t2\t0\t0:2;:3\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t2\tg\tp:3\n");
    let st = |addr, old| Record::Store { addr, size: 4, old, line: 1, from: None };
    let ret = Record::Return { func: 2, value: Some(0) };
    let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }, st(8, 1), ret, Record::Block { func: 2, n: 0 }, st(20, 3), ret];
    let mut args = Args::default();
    args.push(1, &[4, 64]);
    args.push(4, &[16, 64]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 7], args);
    let mut m = alloc::vec![0u8; 32];
    (m[8], m[20]) = (2, 4);
    let v = View { tables: &t, rec: &rec, now: &m };
    let notes = v.notes(0, 6, "f.c", 6);
    assert_eq!((notes[0].line, notes[0].short.as_str()), (3, "b = 1 → 2, 3 → 4"));
    assert_eq!(notes[0].full.as_str(), "p->b in g = 1 → 2; p->b in g = 3 → 4");
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
             Record::Store { addr: 8, size: 4, old: 1, line: 0, from: None }, Record::Store { addr: 36, size: 4, old: 0, line: 0, from: None },
             Record::Block { func: 2, n: 0 },
             Record::Store { addr: 20, size: 1, old: 0, line: 0, from: None }, Record::Return { func: 2, value: Some(0) },
             Record::Store { addr: 100, size: 1, old: 0, line: 0, from: None }, Record::Store { addr: 21, size: 1, old: 0, line: 0, from: None }];
    let mut args = Args::default();
    args.push(0, &[4, 32]);
    args.push(3, &[16]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 8], args);
    let mut m = alloc::vec![0u8; 128];
    (m[8], m[36], m[20], m[100]) = (2, 7, 9, 5);
    let v = View { tables: &t, rec: &rec, now: &m };
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
    let r = [Record::Block { func: 1, n: 0 }, Record::Var { var: 0, line: 0, value: 4 }, Record::Store { addr: 8, size: 4, old: 1, line: 0, from: None }];
    let mut args = Args::default();
    args.push(0, &[64]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
    let mut m = alloc::vec![0u8; 128];
    m[8] = 2;
    let v = View { tables: &t, rec: &rec, now: &m };
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
    let st = |addr, old| Record::Store { addr, size: 4, old, line: 0, from: None };
    let r = [Record::Block { func: 1, n: 0 }, st(8, 1), st(32, 0), st(36, 0)];
    let mut args = Args::default();
    args.push(0, &[4, 32, 64]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 4], args);
    let mut m = alloc::vec![0u8; 64];
    (m[8], m[32], m[36]) = (2, 7, 4);
    let v = View { tables: &t, rec: &rec, now: &m };
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
    let st = |addr, old| Record::Store { addr, size: 4, old, line: 0, from: None };
    let r = [Record::Block { func: 1, n: 0 }, Record::Task { task: 40 }, Record::Block { func: 2, n: 0 }, st(20, 3), Record::Task { task: 0 }, st(8, 1)];
    let mut args = Args::default();
    args.push(0, &[4, 64]);
    args.push(2, &[16, 64]);
    let rec = Recording::new(&t, r.to_vec(), (0..6).collect(), args);
    let mut m = alloc::vec![0u8; 64];
    (m[8], m[20]) = (2, 4);
    let v = View { tables: &t, rec: &rec, now: &m };
    let got: Vec<_> = v.objects(0, 5).into_iter().map(|n| (named(&n.what, &n.within), n.by.map(|t| t.addr))).collect();
    assert_eq!(got, [("p in f".into(), None), ("q in g".into(), Some(40))]);
}

#[test]
fn a_calls_locals_are_not_objects() {
    // f (1) stores its local int x (at frame 32 + 4): the Stack's, not
    // an object, before it returns as after.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n", "T\t1\tbase\tint\t4\ts\nQ\t1\tx:1:4\n");
    let r = [Record::Block { func: 1, n: 0 }, Record::Store { addr: 36, size: 4, old: 0, line: 0, from: None }, Record::Return { func: 1, value: Some(0) }];
    let mut args = Args::default();
    args.push(0, &[32]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
    let mut m = alloc::vec![0u8; 64];
    m[36] = 7;
    let v = View { tables: &t, rec: &rec, now: &m };
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
    let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }, Record::Store { addr: 12, size: 4, old: 0, line: 0, from: None }];
    let mut args = Args::default();
    args.push(0, &[4, 64]);
    args.push(1, &[8, 48]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
    let mut m = alloc::vec![0u8; 128];
    m[12] = 3;
    let v = View { tables: &t, rec: &rec, now: &m };
    let got: Vec<_> = v.objects(0, 2).into_iter().map(|n| (named(&n.what, &n.within), n.changes.into_iter().map(|d| (d.member, format!("{} → {}", d.before, d.after))).collect::<Vec<_>>())).collect();
    assert_eq!(got, [("p in f".into(), alloc::vec![("inner.b".into(), "0 → 3".into())])]);
}

#[test]
fn two_objects_of_one_name_are_numbered() {
    // f (1), called twice, takes struct s *p (at 4, then at 20) and
    // stores p->b: two objects, "p in f" and "p in f #2".
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t1\tf\tp:3\n");
    let st = |addr| Record::Store { addr, size: 4, old: 0, line: 0, from: None };
    let r = [Record::Block { func: 1, n: 0 }, st(8), Record::Return { func: 1, value: Some(0) }, Record::Block { func: 1, n: 0 }, st(24), Record::Return { func: 1, value: Some(0) }];
    let mut args = Args::default();
    args.push(0, &[4, 64]);
    args.push(3, &[20, 64]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 6], args);
    let mut m = alloc::vec![0u8; 128];
    (m[8], m[24]) = (1, 2);
    let v = View { tables: &t, rec: &rec, now: &m };
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
    let st = |old| Record::Store { addr: 8, size: 4, old, line: 0, from: None };
    let r = [Record::Block { func: 1, n: 0 }, st(1), Record::Block { func: 2, n: 0 }, st(2), Record::Return { func: 2, value: Some(0) }];
    let mut args = Args::default();
    args.push(0, &[4, 64]);
    args.push(2, &[4, 48]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 5], args);
    let mut m = alloc::vec![0u8; 128];
    m[8] = 1;
    let v = View { tables: &t, rec: &rec, now: &m };
    let nodes = v.objects(0, 4);
    let d = &nodes[0].changes[0];
    let streaks: Vec<_> = d.streaks.iter().map(|r| (r.at.as_ref().map(|l| l.func), r.before.clone(), r.after.clone(), r.last)).collect();
    assert_eq!((named(&nodes[0].what, &nodes[0].within).as_str(), d.member.as_str(), d.before.as_str(), d.after.as_str()), ("p in f", "b", "1", "1"));
    // (Left as it was, in the end: an object of the "same" group.)
    assert_eq!(nodes[0].group, Group::Same);
    assert_eq!(streaks, [(Some("f".into()), "1".into(), "2".into(), 1), (Some("g".into()), "2".into(), "1".into(), 3)]);
}

#[test]
fn a_member_one_function_writes_back_is_no_change() {
    // f (1) takes struct s *p (at 4): it stores p->b 2 (was 1), then 1
    // again: b left as it was, by the one function: no change, no object.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n",
                          "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t8\nM\t2\ta\t0\t0\t1\nM\t2\tb\t32\t0\t1\nT\t3\tptr\t\t4\t2\nP\t1\tf\tp:3\n");
    let st = |old| Record::Store { addr: 8, size: 4, old, line: 0, from: None };
    let r = [Record::Block { func: 1, n: 0 }, st(1), st(2)];
    let mut args = Args::default();
    args.push(0, &[4, 32]);
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 3], args);
    let mut m = alloc::vec![0u8; 64];
    m[8] = 1;
    let v = View { tables: &t, rec: &rec, now: &m };
    assert!(v.objects(0, 2).is_empty());
}

#[test]
fn a_value_is_followed_through_its_copies() {
    // 7 stored at 0x30 (computed), then copied to 0x10 (a load, a store),
    // then to 0x20 by a memcpy of 8 (two stores): 0x24's way back is
    // 0x24 <- 0x14 <- ... 0x14 was never written; 0x20's is 0x20 <- 0x10 <- 0x30.
    let t = Tables::default();
    let st = |addr, from| Record::Store { addr, size: 4, old: 0, line: 0, from: core::num::NonZeroU32::new(from) };
    let r = [st(0x30, 0), st(0x10, 0x30), st(0x20, 0x10), st(0x24, 0x14)];
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0; 4], Args::default());
    let v = View { tables: &t, rec: &rec, now: &alloc::vec![0; 64] };
    let way = |a| v.origin(a, 4, 3, 8).iter().map(|w| (w.addr, w.from)).collect::<Vec<_>>();
    assert_eq!(way(0x20), [(0x20, Some(0x10)), (0x10, Some(0x30)), (0x30, None)]);
    assert_eq!(way(0x24), [(0x24, Some(0x14))]);
}

#[test]
fn overlapping_copy_chunks_do_not_become_each_others_origins() {
    // Copy 0x10..0x20 to 0x14..0x24, as __ksee_bulk logs it: the higher
    // chunk first, so neither destination exists when its source is sought.
    let copy = |addr: u32| Record::Store { addr, size: 8, old: 0, line: 0, from: core::num::NonZeroU32::new(addr - 4) };
    let r: Vec<Record> = alloc::vec![copy(0x1c), copy(0x14)];
    let rec = Recording::new(&Tables::default(), r, alloc::vec![0; 2], Args::default());
    let v = View { tables: &Tables::default(), rec: &rec, now: &alloc::vec![0; 64] };
    let way = |addr| v.origin(addr, 4, 1, 8).iter().map(|w| (w.addr, w.from)).collect::<Vec<_>>();
    assert_eq!(way(0x1c), [(0x1c, Some(0x18))]);
    assert_eq!(way(0x14), [(0x14, Some(0x10))]);
}

#[test]
fn frames_are_where_their_calls_are() {
    // f (1) runs lines 1 and 2, calls g (2) at 2, then runs 3.
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\nF\t2\tg\t0:9\t0\n\
                           L\t1\t0\t0:1;:2;|;:3\nL\t2\t0\t0:10\n", "");
    let r = [Record::Block { func: 1, n: 0 }, Record::Block { func: 2, n: 0 }];
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0, 1], Args::default());
    let v = View { tables: &t, rec: &rec, now: &Vec::new() };
    let lines: Vec<_> = v.call_stack(1, None, 0, 1).iter().map(|f| (f.func.clone(), f.at.as_ref().map(|a| a.1))).collect();
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
    let v = View { tables: &t, rec: &rec, now: &Vec::new() };
    let frames: Vec<_> = v.call_stack(1, None, 0, 1).iter().map(|f| (f.func.clone(), f.at.as_ref().map(|a| a.1), f.inlined)).collect();
    assert_eq!(frames, [("g".into(), Some(10), false), ("h".into(), Some(20), true), ("f".into(), Some(2), false)]);
    // (Line 2, where h is inlined, ran too: h's line 20 ran for it.)
    assert!(v.coverage("f.c", None).contains_key(&2));
    // At record 0, its first line (nth 0): f at line 1, nothing inlined.
    let first: Vec<_> = v.call_stack(0, Some(0), 0, 0).iter().map(|f| (f.func.clone(), f.at.as_ref().map(|a| a.1))).collect();
    assert_eq!(first, [("f".into(), Some(1))]);
}

#[test]
fn two_inlined_frames_of_one_call_are_bars_of_their_own() {
    // f (1) runs h's line 20 (h inlined into f at line 2), itself g's
    // line 30 (g inlined into h at its line 3): one call, two inlined
    // frames, each with an identity of its own (the chart's bars: hovering
    // one must not light up the other).
    let t = Tables::parse("U\nP\t0\tf.c\nP\t1\th.c\nP\t2\tg.c\nF\t1\tf\t0:1\t0\n\
                           I\t0\th\t0:2\nI\t1\tg\t1:3\nL\t1\t0\t2:30@1@0\n", "");
    let r = [Record::Block { func: 1, n: 0 }];
    let rec = Recording::new(&t, r.to_vec(), alloc::vec![0], Args::default());
    let v = View { tables: &t, rec: &rec, now: &Vec::new() };
    let frames: Vec<_> = v.call_stack(0, None, 0, 0).iter().map(|f| (f.func.clone(), f.inlined)).collect();
    assert_eq!(frames, [("g".into(), true), ("h".into(), true), ("f".into(), false)]);
    // (Three steps, as stepping enters them: the lines calling h and g, then g's.)
    let tl = v.timeline();
    let bars: Vec<_> = tl.at(2).iter().map(|&b| (tl.bars[b as usize].first, tl.bars[b as usize].name.clone())).collect();
    assert_eq!(bars, [(0, "f".into()), (0, "h".into()), (0, "g".into())]);
    // (Each its own file: code inlined, its function's.)
    let files: Vec<_> = tl.at(2).iter().map(|&b| tl.bars[b as usize].file.clone().unwrap_or_default()).collect();
    assert_eq!(files, ["f.c", "h.c", "g.c"]);
    // (Each bar its own span: no two share an identity.)
    assert_eq!(tl.rows, [alloc::vec![(0, 3, 0)], alloc::vec![(1, 3, 1)], alloc::vec![(2, 3, 2)]]);
}

#[test]
fn char_pointers_show_their_strings() {
    let t = Types::parse("T\t1\tbase\tchar\t1\tc\nT\t2\tptr\t\t4\t1\n");
    let mut m = alloc::vec![0u8; 128];
    m[0..4].copy_from_slice(&8u32.to_le_bytes());
    m[8..12].copy_from_slice(b"tmp\0");
    m[4..8].copy_from_slice(&16u32.to_le_bytes()); // (to bytes not printable: escaped, as C writes them)
    m[16] = 0xff;
    let value = |at| t.object(&m.clone(), "p", at, 2, 0).value.unwrap();
    assert_eq!((value(0).as_str(), value(4).as_str()), ("0x8 → \"tmp\"", "0x10 → \"\\xff\""));
}

#[test]
fn scalar_pointers_show_what_they_point_to() {
    let t = Types::parse("T\t1\tbase\tint\t4\ts\nT\t2\tptr\t\t4\t1\n");
    let mut m = alloc::vec![0u8; 16];
    m[0..4].copy_from_slice(&8u32.to_le_bytes());
    m[8..12].copy_from_slice(&(-3i32).to_le_bytes());
    let o = t.object(&m, "pos", 0, 2, 0);
    assert_eq!((o.value.as_deref(), o.open), (Some("0x8 → -3"), None));
}

#[test]
fn objects_expose_pointer_targets() {
    let t = Types::parse("T\t1\tbase\tint\t4\ts\nT\t2\tstruct\tnode\t8\nM\t2\tv\t0\t0\t1\n\
                          M\t2\tnext\t32\t0\t3\nT\t3\tptr\t\t4\t2\n");
    let mut m = alloc::vec![0u8; 16];
    m[0..4].copy_from_slice(&7i32.to_le_bytes());
    m[4..8].copy_from_slice(&8u32.to_le_bytes());
    let o = t.object(&m, "n", 0, 2, 1);
    let c = o.children.unwrap();
    assert_eq!((c[0].value.as_deref(), c[1].value.as_deref(), c[1].open), (Some("7"), Some("0x8"), Some(Target { ty: 2, addr: 8 })));
}

#[test]
fn a_member_of_size_0_is_not_shown() {
    // struct s { int a; struct e {} e; }: the config left e empty, nothing in it.
    let t = Tables::parse("U\n", "T\t1\tbase\tint\t4\ts\nT\t2\tstruct\ts\t4\nM\t2\ta\t0\t0\t1\nM\t2\te\t32\t0\t3\nT\t3\tstruct\te\t0\n");
    let o = t.types.object(&alloc::vec![0u8; 8], "x", 0, 2, 1);
    assert_eq!(o.children.unwrap().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a"]);
}

#[test]
fn a_long_array_says_how_many_more() {
    // int a[100], each its index: the first 64, then "… 36 more".
    let t = Tables::parse("U\n", "T\t1\tbase\tint\t4\ts\nT\t2\tarray\t\t400\t1\t100\n");
    let m: Vec<u8> = (0..100u32).flat_map(|i| i.to_le_bytes()).collect();
    let c = t.types.object(&m, "a", 0, 2, 1).children.unwrap();
    assert_eq!((c.len(), c[64].name.as_str(), c[64].value.as_deref()), (65, "[64…99]", Some("… 36 more")));
}

#[test]
fn an_array_s_runs_fold() {
    // int a[8] = {1, 0, 0, 0, 0, 2, 0, 0}: a run of three or more one row.
    let t = Tables::parse("U\n", "T\t1\tbase\tint\t4\ts\nT\t2\tarray\t\t32\t1\t8\n");
    let m: Vec<u8> = [1u32, 0, 0, 0, 0, 2, 0, 0].iter().flat_map(|i| i.to_le_bytes()).collect();
    let c = t.types.object(&m, "a", 0, 2, 1).children.unwrap();
    let names: Vec<&str> = c.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["[0]", "[1…4]", "[5]", "[6]", "[7]"]);
}

#[test]
fn a_task_is_named_as_it_was_then() {
    // task_struct { int pid; char comm[8]; } at 0x40, pid 19: renamed from
    // "prog" to "hi" at record 2 (a store of comm's first 4 bytes).
    let t = Tables::parse("U\nP\t0\tf.c\nF\t1\tf\t0:1\t0\n", "T\t1\tbase\tint\t4\ts\nT\t2\tbase\tchar\t1\tc\nT\t3\tarray\t\t8\t2\t8\n\
        T\t4\tstruct\ttask_struct\t12\nM\t4\tpid\t0\t0\t1\nM\t4\tcomm\t32\t0\t3\n");
    let r = [Record::Task { task: 0x40 }, Record::Block { func: 1, n: 0 },
             Record::Store { addr: 0x44, size: 4, old: u32::from_le_bytes(*b"prog") as u64, line: 0, from: None }, Record::Block { func: 1, n: 0 }];
    let rec = Recording::new(&t, r.to_vec(), (0..4).collect(), Args::default());
    let mut m = alloc::vec![0u8; 128];
    m[0x40] = 19;
    m[0x44..0x47].copy_from_slice(b"hi\0");
    let v = View { tables: &t, rec: &rec, now: &m };
    let name = |i| v.task(0x40, i).id.map(|t| t.comm);
    assert_eq!((name(1), name(3), v.task(0x40, 3).pid()), (Some("prog".into()), Some("hi".into()), 19));
}

#[test]
fn a_chart_window_follows_one_wheel_axis() {
    // Horizontal trackpad movement pans without changing the span,
    // even with a little vertical noise; Shift pans by the vertical delta.
    let horizontal = wheel_window((100.0, 100.0), 500.0, 1000.0, 80.0, 4.0, false, 1.5);
    let shifted = wheel_window((100.0, 100.0), 500.0, 1000.0, 80.0, 20.0, true, 1.5);
    assert!((horizontal.0 - 108.0).abs() < 1e-9 && horizontal.1 == 100.0);
    assert!((shifted.0 - 102.0).abs() < 1e-9 && shifted.1 == 100.0);
}

#[test]
fn a_chart_window_zooms_around_the_pointer() {
    let out = wheel_window((100.0, 100.0), 500.0, 1000.0, 4.0, 80.0, false, 1.2);
    let into = wheel_window((100.0, 100.0), 500.0, 1000.0, 4.0, -80.0, false, 0.8);
    assert!(out.0 < 100.0 && out.1 > 100.0);
    assert!(into.0 > 100.0 && into.1 < 100.0);

    // A pointer outside the chart is anchored to its nearest edge.
    assert_eq!(wheel_window((100.0, 100.0), -100.0, 1000.0, 0.0, 80.0, false, 1.2),
               wheel_window((100.0, 100.0), 0.0, 1000.0, 0.0, 80.0, false, 1.2));
    assert_eq!(wheel_window((100.0, 100.0), 1100.0, 1000.0, 0.0, 80.0, false, 1.2),
               wheel_window((100.0, 100.0), 1000.0, 1000.0, 0.0, 80.0, false, 1.2));
}

#[test]
fn no_wheel_delta_keeps_a_chart_window() {
    assert_eq!(wheel_window((100.0, 100.0), 500.0, 1000.0, 0.0, 0.0, false, 1.0), (100.0, 100.0));
}

#[test]
fn steps_go_into_or_over() {
    // f (depth 0) line 1, calls g (depth 1): lines 10, 11; then f's line 2.
    let line = |depth, line| Line { i: 0, at: 0, nth: 0, cut: 0, func: "", file: "", line, depth, turn: 1,
                                   tests: Vec::new(), called: None };
    let lines = [line(0, 1), line(1, 10), line(1, 11), line(0, 2)];
    // (Into: the very next; within f (depth 0): over g; within g, its next.)
    assert_eq!((step(&lines, 0, 1, None), step(&lines, 0, 1, Some(0)), step(&lines, 1, 1, Some(1))), (Some(1), Some(3), Some(2)));
    assert_eq!((step(&lines, 3, -1, Some(0)), step(&lines, 0, -1, None)), (Some(0), None));
    assert_eq!(step(&lines, 3, 1, None), None);
    // (The same line again at its depth and turn, a stop to the recording, is
    // no step: past it; another turn of it is.)
    let lines = [line(0, 1), line(0, 1), line(0, 2), Line { turn: 2, ..line(0, 2) }];
    assert_eq!((step(&lines, 0, 1, Some(0)), step(&lines, 2, 1, Some(0)), step(&lines, 2, -1, Some(0))), (Some(2), Some(3), Some(1)));
}

#[test]
fn an_inlined_call_is_stepped_into_as_a_call_is() {
    // f (1) runs line 1, then g's line 30 inlined in h inlined in f (h at
    // f's line 2, g at h's line 3), then f's line 4: the lines calling h
    // and g are stops of their own, each with the frames it is in.
    let t = Tables::parse("U\nP\t0\tf.c\nP\t1\th.c\nP\t2\tg.c\nF\t1\tf\t0:1\t0\n\
                           I\t0\th\t0:2\nI\t1\tg\t1:3\nL\t1\t0\t0:1;2:30@1@0;0:4\n", "");
    let rec = Recording::new(&t, alloc::vec![Record::Block { func: 1, n: 0 }], alloc::vec![0], Args::default());
    let v = View { tables: &t, rec: &rec, now: &Vec::new() };
    let lines = v.lines(0, 0, 0);
    let at: Vec<_> = lines.iter().map(|l| (l.file, l.line, l.depth)).collect();
    assert_eq!(at, [("f.c", 1, 0), ("f.c", 2, 0), ("h.c", 3, 1), ("g.c", 30, 2), ("f.c", 4, 0)]);
    let stack = |l: &Line| v.call_stack(l.i, Some(l.nth as usize), l.cut as usize, l.i).iter().map(|f| (f.func.clone(), f.at.as_ref().map(|a| a.1), f.depth)).collect::<Vec<_>>();
    assert_eq!(stack(&lines[1]), [("f".into(), Some(2), 0)]);
    assert_eq!(stack(&lines[2]), [("h".into(), Some(3), 1), ("f".into(), Some(2), 0)]);
    assert_eq!(stack(&lines[3]), [("g".into(), Some(30), 2), ("h".into(), Some(3), 1), ("f".into(), Some(2), 0)]);
    // (Within f, over the code inlined at its line 2: its next line, 4;
    // within h, from g's line: h's own next, none, so f's 4.)
    assert_eq!((step(&lines, 1, 1, Some(0)), step(&lines, 3, 1, Some(1))), (Some(4), Some(4)));
}

#[test]
fn the_steps_are_the_lines_stepping_visits() {
    // f's lines 1 and 4, between them g's line 30 inlined in h inlined in
    // f: a step for each line View::lines has, the lines calling h and g too.
    let t = Tables::parse("U\nP\t0\tf.c\nP\t1\th.c\nP\t2\tg.c\nF\t1\tf\t0:1\t0\n\
                           I\t0\th\t0:2\nI\t1\tg\t1:3\nL\t1\t0\t0:1;2:30@1@0;0:4\n", "");
    let rec = Recording::new(&t, alloc::vec![Record::Block { func: 1, n: 0 }], alloc::vec![0], Args::default());
    let v = View { tables: &t, rec: &rec, now: &Vec::new() };
    let lines: Vec<_> = v.lines(0, 0, 0).iter().map(|l| (l.i, l.nth, l.cut)).collect();
    assert_eq!(rec.steps, lines);
    // (Each step its line's stack.)
    let tl = v.timeline();
    let depths: Vec<_> = (0..5).map(|s| tl.at(s).len()).collect();
    assert_eq!(depths, [1, 1, 2, 3, 1]);
    // (A window's spans and depth: step 3, the third row's one bar; steps 0
    // and 1, one row.)
    assert_eq!((tl.spans(2, 3.0, 4.0), tl.depth(3.0, 4.0), tl.depth(0.0, 2.0)), (&[(3, 4, 2)][..], 3, 1));
}


#[test]
fn a_path_follows_pointers() {
    // struct f { int a[4]; }; struct t { int pid; struct f *files; }: at
    // 0x10, files at 0x40 (a = {0, 0, 0, 7}); "files->a", by its path.
    let t = Tables::parse("U\n", "T\t1\tbase\tint\t4\ts\nT\t2\tarray\t\t16\t1\t4\nT\t3\tstruct\tf\t16\nM\t3\ta\t0\t0\t2\n\
        T\t4\tptr\t\t4\t3\nT\t5\tstruct\tt\t8\nM\t5\tpid\t0\t0\t1\nM\t5\tfiles\t32\t0\t4\n");
    let mut m = alloc::vec![0u8; 0x80];
    m[0x14..0x18].copy_from_slice(&0x40u32.to_le_bytes());
    m[0x4c..0x50].copy_from_slice(&7u32.to_le_bytes());
    let o = t.types.at_path(&m, 5, 0x10, "files->a").unwrap();
    assert_eq!((o.name.as_str(), o.addr), ("files->a", 0x40));
    assert!(t.types.at_path(&m, 5, 0x10, "files->b").is_none());
}
