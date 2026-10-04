use ksee_core::trace::Record;
use ksee_core::view::run_to;

use super::harness::{arg, child, local, value, KernelHarness};

pub(super) async fn run(t: &mut KernelHarness, main: u32) {
    let m = t.machine.clone();
    // A read: f_pos over time, who wrote it, a local.
    t.line("int fd = openat(AT_FDCWD, \"/tmp/note\", O_CREAT | O_TRUNC | O_WRONLY, 0644)").await;
    t.line("write(fd, \"hello from the page cache\\n\", 26)").await;
    t.line("close(fd)").await;
    t.line("int rfd = openat(AT_FDCWD, \"/tmp/note\", O_RDONLY)").await;
    let got = t.line("char got[64]").await.var.unwrap();
    let read = t.line("read(rfd, got, 64)").await.syscall.unwrap();
    let rlast = read.last;
    t.check("read 26 bytes back", read.ret == Some(26) && m.show(got.0, got.1, Some(64)).starts_with("\"hello from the page cache"));
    let read_steps = m.view(|v| v.lines(read.first, rlast, read.task.addr));
    let in_read = read_steps.iter().find(|x| x.func == "vfs_read").map_or(0, |x| x.i);
    let returned = read_steps.iter().find_map(|x| x.called.as_ref().filter(|c| c.func == "vfs_read")).map(|c| c.value);
    t.check(format!("the line calling vfs_read says what it returned: {returned:?}"), returned == Some(26));
    let file = t.stack(in_read, None, in_read).first().and_then(|f| arg(f, "file").cloned()).unwrap();
    let open = file.open.unwrap();
    let fpos = |i: usize| m.at_moment(i, |v, mem| value(child(&v.tables.types.object(mem, "file", open.addr, open.ty, 1), "f_pos")));
    t.check("f_pos before the read, and after", fpos(read.first) == "0" && fpos(rlast) == "26");
    let buf = |i: usize| t.stack(in_read, None, i).first().map(|f| value(arg(f, "buf"))).unwrap_or_default();
    t.check("the user's buffer, before the read and after", !buf(read.first).contains("hello") && buf(rlast).contains("\"hello from the page cache"));
    let pos_addr = m.at_moment(rlast, |v, mem| child(&v.tables.types.object(mem, "file", open.addr, open.ty, 1), "f_pos").map(|o| o.addr)).unwrap_or(0);
    let who = m.view(|v| v.origin(pos_addr, 8, rlast, 8)).into_iter().next();
    t.check("who wrote f_pos: ksys_read", who.and_then(|w| w.at).is_some_and(|l| l.func == "ksys_read"));
    {
        let nodes = m.view(|v| v.objects(read.first, rlast));
        let obj = nodes.iter().find(|n| n.addr == open.addr && n.id == open.ty);
        let d = obj.and_then(|o| o.changes.iter().find(|x| x.member == "f_pos"));
        t.check(format!("the read wrote its file: f_pos {:?} → {:?}", d.map(|d| &d.before), d.map(|d| &d.after)),
                d.is_some_and(|d| d.before == "0" && d.after == "26" && d.runs.len() == 1 && d.runs[0].at.as_ref().is_some_and(|l| l.func == "ksys_read")));
        let notes = m.view(|v| v.notes(read.first, rlast, "fs/read_write.c", rlast));
        t.check("the read's notes in fs/read_write.c: the kiocb's ki_pos, named as the code names it",
                notes.iter().any(|n| n.short.starts_with("ki_pos: ") && (n.full.contains("kiocb->ki_pos in ") || n.full.contains("kiocb.ki_pos in "))));
        t.check("what the read changed: its file, not its kiocb (on the stack)",
                nodes.iter().any(|n| n.ty == "struct file") && !nodes.iter().any(|n| n.ty == "struct kiocb"));
    }
    {
        // Where the read's bytes came from: copied from the page cache, which the write filled.
        let b = u32::from_str_radix(t.stack(in_read, None, in_read).first().map(|f| value(arg(f, "buf"))).unwrap_or_default().trim_start_matches("0x").split(' ').next().unwrap_or("0"), 16).unwrap_or(0);
        let way = m.view(|v| v.origin(b, 4, rlast, 8));
        t.check(format!("the read's bytes, back through their copies: {} writes", way.len()), way.len() >= 2 && way[0].from.is_some() && Some(way[1].addr) == way[0].from);
    }
    // A program's own variables; a blocked read goes on as a later write wakes it.
    {
        let buf = t.line("char buf[16]").await.var.unwrap();
        let fds = t.line("int fds[2]").await.var.unwrap();
        t.line("pipe2(fds, 0)").await;
        t.line("write(fds[1], \"hi\", 2)").await;
        let n = t.line("read(fds[0], buf, 16)").await.ret;
        t.check(format!("variables: fds = {}, buf = {}", t.show(fds), t.show(buf)), n == Some(2) && t.show(buf) == "\"hi\"");
        t.line("void *mem = mmap2(NULL, 65536, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0)").await;
        t.line("pid_t thread = clone(CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD | CLONE_SYSVSEM, mem + 65536, NULL, NULL, 0)").await;
        let reading = t.line_on(Some("thread"), "read(fds[0], buf, 8)").await;
        let wrote = t.line("write(fds[1], \"ok\", 2)").await;
        let later = reading.id.and_then(|id| t.finished.get(&id)).map(|f| f.ret);
        t.check(format!("a blocked read, woken: wrote {:?}, read {later:?}: buf = {}", wrote.ret, t.show(buf)),
                reading.how.is_none() && wrote.ret == Some(2) && later == Some(2) && t.show(buf) == "\"ok\"");
        let got = t.line("char got16[16]").await.var.unwrap();
        let set = t.line("prctl(PR_SET_NAME, \"ksee\", 0, 0, 0)").await.ret;
        let get = t.line("prctl(PR_GET_NAME, got16, 0, 0, 0)").await.ret;
        let reset = t.line("prctl(PR_SET_NAME, \"prog\", 0, 0, 0)").await.ret;
        t.check(format!("prctl(PR_SET_NAME) = {set:?}, PR_GET_NAME: {}; name reset = {reset:?}", t.show(got)),
                set == Some(0) && get == Some(0) && reset == Some(0) && t.show(got) == "\"ksee\"");
        let ts = t.line("struct __kernel_timespec ts = {1, 5}").await.var.unwrap();
        t.check(format!("a positional struct initializer: ts = {}", t.show(ts)), t.show(ts) == "{tv_sec = 1, tv_nsec = 5}");
    }
    // A 64-bit argument, whole: llseek's offset (high 1, low 0) is vfs_llseek's loff_t 1 << 32.
    {
        t.line("long long res").await;
        let ls = t.line("llseek(rfd, 1, 0, &res, 0)").await.syscall.unwrap();
        let lslast = ls.last;
        let at = m.first_call("vfs_llseek", ls.first, lslast, Some(ls.task.addr));
        let off = at.map(|i| t.stack(i, None, i)).and_then(|s| s.into_iter().find(|f| f.func == "vfs_llseek")).map(|f| value(arg(&f, "offset")));
        t.check(format!("a 64-bit argument whole: vfs_llseek's offset {off:?}"), off.as_deref() == Some("4294967296"));
        let got = m.view(|v| v.lines(ls.first, lslast, ls.task.addr)).into_iter().find_map(|s| s.called.filter(|c| c.func == "vfs_llseek")).map(|c| c.value);
        t.check(format!("a 64-bit result whole: vfs_llseek returned {got:?}"), got == Some(4294967296));
    }
    t.check("line steps sit at their first store: some line is at a record past its block's, none before it, and a store is on its line",
            read_steps.iter().any(|s| s.at > s.i) && read_steps.iter().all(|s| s.at >= s.i)
            && read_steps.iter().any(|s| s.at > s.i && m.view(|v| v.line_of(s.at + 1)) == Some((s.i, s.nth))));
    let exact = read_steps.iter().enumerate().skip(1).find(|(k, s)| {
        s.i == read_steps[k - 1].i || m.view(|v| matches!(v.rec.records.get(s.i), Some(Record::Return { .. })))
    });
    let exact_run = exact.is_some_and(|(k, s)| run_to(&read_steps, k - 1, &s.file, s.line, 1) == Some(k));
    t.check("run to a line sharing a record or resumed at a return: its exact step", exact_run);
    let ksys_last = read_steps.iter().rev().find(|x| x.func == "ksys_read").map_or(0, |x| x.i);
    let pos = |i: usize| t.stack(i, None, i).iter().find(|c| c.func == "ksys_read").map(|f| value(local(f, "pos")));
    let (before, after) = (pos(in_read), pos(ksys_last));
    t.check(format!("a local on the stack, as it was: ksys_read's pos, 0 then 26 ({before:?}, {after:?})"), before.as_deref() == Some("0") && after.as_deref() == Some("26"));
    let ret = t.stack(ksys_last, None, ksys_last).iter().find(|c| c.func == "ksys_read").map(|f| value(local(f, "ret")));
    t.check(format!("a local in a wasm local: ksys_read's ret = {ret:?}"), ret.as_deref() == Some("26"));
    t.line("close(rfd)").await;
    let bad = t.line("write(1, 0x1000, 4)").await.ret;
    let pid = t.line("getpid()").await.ret;
    t.check("a bad user pointer: EFAULT", bad == Some(-14) && pid == Some(main as i64));
    // A char * shows its string: mkdir's struct filename, its name.
    {
        let mk = t.line("mkdirat(AT_FDCWD, \"/mnt/named\", 0755)").await.syscall.unwrap();
        let mklast = mk.last;
        let i = m.first_call("filename_create", mk.first, mklast, None).unwrap_or(0);
        let a = t.stack(i, None, i).into_iter().find(|f| f.func == "filename_create").and_then(|f| f.args.into_iter().find(|a| a.ty == "struct filename *"));
        let name = a.and_then(|a| a.open).map(|o| m.at_moment(i, |v, mem| value(child(&v.tables.types.object(mem, "name", o.addr, o.ty, 1), "name"))));
        t.check(format!("a char * with its string: filename->name = {name:?}"), name.is_some_and(|n| n.ends_with("\"/mnt/named\"")));
        t.line("unlinkat(AT_FDCWD, \"/mnt/named\", AT_REMOVEDIR)").await;
    }
}
