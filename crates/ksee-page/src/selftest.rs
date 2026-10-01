//! The kernel's tests, in the page's module (feature `selftest`, which
//! tests/web.js builds and runs): the program makes syscalls, recorded, as
//! the page's lessons do, and the checks are of what the recordings show,
//! through the views the page uses; then every lesson and try, on a fresh
//! kernel each, its calls finished and its links found.

use std::collections::BTreeMap;
use std::rc::Rc;

use ksee_core::program::{statement, Line as Stmt};
use ksee_core::view::{Frame, Object, RecordKind, RegionKind, SyscallSummary};
use ksee_runtime::machine::Machine;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::lesson::{Example, Finished, News};
use crate::text::{self, Piece};

fn log(s: &str) {
    web_sys::console::log_1(&s.into());
}

/// A promise of `ms` milliseconds passed.
async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |ok, _| {
        let set: js_sys::Function = js_sys::Reflect::get(&js_sys::global(), &"setTimeout".into()).unwrap().into();
        let _ = set.call2(&JsValue::NULL, &ok, &ms.into());
    });
    let _ = JsFuture::from(p).await;
}

/// A call a line made, as the kernel left it once quiet: its id, how it
/// finished (none: it blocked) and its result, its syscall summary.
struct Made {
    id: Option<u32>,
    how: Option<String>,
    ret: Option<i64>,
    s: Option<SyscallSummary>,
    var: Option<(u32, u32, Option<u32>)>,
}

struct T {
    m: Rc<Machine>,
    failed: bool,
    /// The console's text so far.
    out: String,
    /// The calls finished, by id.
    fin: BTreeMap<u32, Finished>,
}

impl T {
    fn check(&mut self, what: impl AsRef<str>, ok: bool) -> bool {
        log(&format!("{}: {}", if ok { "ok" } else { "FAIL" }, what.as_ref()));
        self.failed |= !ok;
        ok
    }

    fn console(&mut self) {
        self.out.push_str(&self.m.console().replace('\r', ""));
    }

    /// The kernel run until it is quiet: the calls finished meanwhile kept.
    async fn quiet(&mut self) {
        let news = JsFuture::from(self.m.quiet()).await.ok().and_then(|n| serde_wasm_bindgen::from_value::<News>(n).ok()).unwrap_or_default();
        self.m.snapshot();
        self.console();
        for f in news.finished {
            self.fin.insert(f.id, f);
        }
    }

    fn syscall(&self, id: u32) -> Option<SyscallSummary> {
        let k = self.m.syscall_of(id)?;
        self.m.view(|v| v.syscalls().into_iter().nth(k))
    }

    /// Line `text` of the program, made; a call: the kernel run until quiet.
    async fn line(&mut self, text: &str) -> Made {
        let made = match self.m.make(text) {
            Ok(m) => m,
            Err(e) => {
                self.check(format!("{text}: {e}"), false);
                return Made { id: None, how: None, ret: None, s: None, var: None };
            }
        };
        if let Some(v) = made.decl {
            return Made { id: None, how: None, ret: None, s: None, var: Some((v.ty, v.addr, v.count)) };
        }
        let Some(id) = made.id else { return Made { id: None, how: None, ret: None, s: None, var: None } };
        self.quiet().await;
        let f = self.fin.get(&id).cloned();
        Made { id: Some(id), how: f.as_ref().map(|f| f.how.clone()), ret: f.map(|f| f.ret as i64), s: self.syscall(id), var: None }
    }

    fn last(&self, s: &SyscallSummary) -> usize {
        s.last.unwrap_or(self.m.records().saturating_sub(1))
    }

    /// Whether syscall `s` ran function `func` (on its task).
    fn ran(&self, s: &Option<SyscallSummary>, func: &str) -> bool {
        s.as_ref().is_some_and(|s| s.last.is_some() && self.m.first_call(func, s.first, self.last(s), Some(s.task.addr)).is_some())
    }

    /// The call stack at record `i` (its `nth` line), its values as memory was after record `at`.
    fn stack(&self, i: usize, nth: Option<usize>, at: usize) -> Vec<Frame> {
        self.m.at_moment(at, |v, mem| v.call_stack(i, nth, mem, at))
    }

    fn show(&self, v: (u32, u32, Option<u32>)) -> String {
        self.m.show(v.0, v.1, v.2)
    }

    /// A lesson's program (or a try) on a fresh kernel: each line's call
    /// id (none: a declaration, an assignment).
    async fn program(&mut self, lines: &[String]) -> Vec<Option<u32>> {
        let _ = self.m.restart().await;
        self.console();
        let mut ids = Vec::new();
        for l in lines {
            ids.push(self.line(l).await.id);
        }
        ids
    }
}

/// A value's child named `name`, if any.
fn child<'a>(o: &'a Object, name: &str) -> Option<&'a Object> {
    o.children.as_ref()?.iter().find(|c| c.name == name)
}

fn arg<'a>(f: &'a Frame, name: &str) -> Option<&'a Object> {
    f.args.as_ref()?.iter().find(|a| a.name == name)
}

fn local<'a>(f: &'a Frame, name: &str) -> Option<&'a Object> {
    f.locals.as_ref()?.iter().find(|a| a.name == name)
}

fn value(o: Option<&Object>) -> String {
    o.and_then(|o| o.value.clone()).unwrap_or_default()
}

/// Run the tests on the booted `machine`, with the lessons (examples.json):
/// whether all passed (each check is logged, "ok: …" or "FAIL: …").
#[wasm_bindgen]
pub async fn self_test(machine: Machine, examples: JsValue) -> Result<bool, JsValue> {
    let examples: Vec<Example> = serde_wasm_bindgen::from_value(examples)?;
    let mut t = T { m: Rc::new(machine), failed: false, out: String::new(), fin: BTreeMap::new() };
    let m = t.m.clone();
    let main = m.main().unwrap_or(0);
    t.check(format!("the program ready: a runner ({main}), not /init"), main > 1 && m.runners().contains(&main));
    m.snapshot();
    t.check("nothing recorded at first", m.records() == 0);

    // Calls, traced.
    let w = t.line("write(1, \"hello kSEE\\n\", 11)").await.s.unwrap();
    for _ in 0..100 {
        t.console();
        if t.out.contains("hello kSEE") {
            break;
        }
        sleep(10).await;
    }
    t.check(format!("a write, by the program ({}, pid {:?}), shown on the console", w.task.comm.clone().unwrap_or_default(), w.task.pid),
            w.ret == Some(11) && w.task.comm.as_deref() == Some("prog") && w.task.pid == Some(main as i32) && t.out.contains("hello kSEE"));
    t.check("write ran vfs_write", t.ran(&Some(w.clone()), "vfs_write"));
    let s = t.line("getpid()").await.s;
    t.check("getpid ran __task_pid_nr_ns", t.ran(&s, "__task_pid_nr_ns"));
    let s = t.line("mkdirat(AT_FDCWD, \"/tmp\", 0755)").await.s;
    t.check("mkdirat ran vfs_mkdir", t.ran(&s, "vfs_mkdir"));
    let nap = t.line("clock_nanosleep(CLOCK_MONOTONIC, 0, {0, 30000000}, NULL)").await.s.unwrap();
    let idle = m.view(|v| v.chart(nap.first, nap.last.unwrap_or(0), 50).cols.iter().any(|c| v.task(c.task, nap.first).comm.as_deref() == Some("swapper/0")));
    t.check("sleeping switched to the idle task and back", t.ran(&Some(nap.clone()), "__schedule") && idle);
    let long = t.line("clock_nanosleep(CLOCK_MONOTONIC, 0, {2, 0}, NULL)").await;
    t.check("a 2 s sleep returns (the clock skipped ahead)", long.how.as_deref() == Some("returned") && long.ret == Some(0));
    let sel = t.line("pselect6(0, NULL, NULL, NULL, {1, 0}, NULL)").await;
    t.check("a 1 s select() times out", sel.how.as_deref() == Some("returned") && sel.ret == Some(0));
    m.snapshot();
    let wi = m.view(|v| v.syscalls().iter().position(|s| s.first == w.first));
    let vfs = m.view(|v| v.definition("vfs_write"));
    t.check("vfs_write: where it is", vfs.as_ref().is_some_and(|d| d.file == "fs/read_write.c" && d.last.is_some_and(|l| l > d.line)));
    {
        let cs = m.view(|v| v.syscalls().into_iter().find(|s| s.name == "mkdirat").map(|mk| v.objects(mk.first, mk.last.unwrap_or(0)))).unwrap_or_default();
        let globals: Vec<String> = cs.iter().filter(|c| c.kind == RegionKind::Global).map(|c| c.what.clone()).collect();
        t.check(format!("mkdirat's objects name the kernel's globals: {}; not the machine's", globals.join(", ")),
                globals.iter().any(|g| g == "nr_inodes") && !cs.iter().any(|c| c.what == "wasm_irq_flags"));
    }
    t.check("fs/read_write.c coverage", m.view(|v| v.coverage("fs/read_write.c", wi)).iter().any(|l| l.count > 0));
    let wlast = t.last(&w);
    let chart = m.view(|v| v.chart(w.first, wlast, 200));
    t.check("a stack chart of write's records", chart.cols.len() == 200 && chart.cols[0].task == w.task.addr && chart.bars.len() > 10);
    let steps = m.view(|v| v.lines(w.first, wlast, w.task.addr));
    t.check("write, line by line (inlined lines too)", steps.len() > 50 && steps.iter().any(|x| x.inlined.is_some()));
    if let Some(k) = steps.iter().position(|x| x.inlined.is_some()) {
        let s = &steps[k];
        let inl = t.stack(s.i, Some(s.nth as usize), s.i);
        t.check(format!("inlined code is a frame of its own ({} in {})", s.inlined.clone().unwrap_or_default(), s.func),
                inl.first().is_some_and(|f| f.inlined && Some(&f.func) == s.inlined.as_ref() && f.line == Some(s.line)) && inl.iter().any(|f| f.func == s.func && !f.inlined));
    }
    t.check("and a bar of its own in the chart", chart.bars.iter().any(|b| b.inlined));
    t.check(format!("a syscall's entry says the syscall: {}", chart.bars[0].label), chart.bars[0].label == "wasm_syscall · write");
    {
        // (Each column is a moment, its record: the Stack there has every call it draws.)
        let (mut drawn, mut missing) = (0, Vec::new());
        for c in &chart.cols {
            let frames = t.stack(c.i, None, c.i);
            for b in c.frames.iter().map(|&k| &chart.bars[k as usize]) {
                drawn += 1;
                if !frames.iter().any(|f| f.first == b.first && f.func == b.name && f.inlined == b.inlined) {
                    missing.push(b.name.clone());
                }
            }
        }
        t.check(format!("a chart's column is its record's stack: {drawn} calls drawn, {} not there", missing.len()), drawn > 0 && missing.is_empty());
    }
    let mid = steps.iter().find(|x| x.func == "vfs_write").map_or(0, |x| x.i);
    let stack = t.stack(mid, None, mid);
    let kw = stack.iter().find(|f| f.func == "ksys_write");
    let kw_def = m.view(|v| v.definition("ksys_write"));
    t.check("an outer call is where it calls the next one in",
            kw.is_some_and(|f| f.file.as_deref() == Some("fs/read_write.c") && f.line > kw_def.as_ref().map(|d| d.line)));
    t.check("a past call stack, with arguments", stack.first().is_some_and(|f| f.func == "vfs_write") && kw.is_some_and(|f| value(arg(f, "count")) == "11"));
    let undone = |i: usize| m.view(|v| v.rec.stores().after(i));
    t.check("memory as it was: writes undone", undone(w.first) > undone(wlast));
    let in_write = m.first_call("vfs_write", w.first, wlast, Some(w.task.addr)).unwrap_or(0);
    let comm = m.at_moment(in_write, |v, mem| v.task_fields(mem, w.task.addr)).fields.into_iter().find(|c| c.name == "comm").and_then(|c| c.value);
    t.check("the task, as it was", comm.as_deref() == Some("\"prog\""));
    let rq = m.at_moment(in_write, |v, mem| v.tables.types.globals.get("runqueues").map(|&(a, ty)| v.tables.types.object(mem, "runqueues", a, ty, 1)));
    t.check("a global", rq.is_some_and(|o| child(&o, "nr_running").is_some()));

    // A read: f_pos over time, who wrote it, a local.
    t.line("int fd = openat(AT_FDCWD, \"/tmp/note\", O_CREAT | O_TRUNC | O_WRONLY, 0644)").await;
    t.line("write(fd, \"hello from the page cache\\n\", 26)").await;
    t.line("close(fd)").await;
    t.line("int rfd = openat(AT_FDCWD, \"/tmp/note\", O_RDONLY)").await;
    let got = t.line("char got[64]").await.var.unwrap();
    let read = t.line("read(rfd, got, 64)").await.s.unwrap();
    let rlast = t.last(&read);
    t.check("read 26 bytes back", read.ret == Some(26) && m.show(got.0, got.1, Some(64)).starts_with("\"hello from the page cache"));
    let read_steps = m.view(|v| v.lines(read.first, rlast, read.task.addr));
    let in_read = read_steps.iter().find(|x| x.func == "vfs_read").map_or(0, |x| x.i);
    let last = read_steps.iter().rev().find(|x| x.func == "vfs_read");
    t.check(format!("vfs_read's last line ({:?}) says what it returns: {:?}", last.map(|l| l.line), last.and_then(|l| l.ret)), last.and_then(|l| l.ret) == Some(26));
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
        let reading = t.line("thread: read(fds[0], buf, 8)").await;
        let wrote = t.line("write(fds[1], \"ok\", 2)").await;
        let later = reading.id.and_then(|id| t.fin.get(&id)).map(|f| f.ret as i64);
        t.check(format!("a blocked read, woken: wrote {:?}, read {later:?}: buf = {}", wrote.ret, t.show(buf)),
                reading.how.is_none() && wrote.ret == Some(2) && later == Some(2) && t.show(buf) == "\"ok\"");
        let name = t.line("char name[16] = \"ksee\"").await.var.unwrap();
        let got = t.line("char got16[16]").await.var.unwrap();
        let set = t.line("prctl(PR_SET_NAME, name, 0, 0, 0)").await.ret;
        let get = t.line("prctl(PR_GET_NAME, got16, 0, 0, 0)").await.ret;
        t.line("name = \"prog\"").await;
        t.line("prctl(PR_SET_NAME, name, 0, 0, 0)").await;
        t.check(format!("prctl(PR_SET_NAME) = {set:?}, PR_GET_NAME: {}; name set again: {}", t.show(got), t.show(name)),
                set == Some(0) && get == Some(0) && t.show(got) == "\"ksee\"" && t.show(name) == "\"prog\"");
        let ts = t.line("struct __kernel_timespec ts").await.var.unwrap();
        t.line("ts = {1, 5}").await;
        t.check(format!("a struct assigned: ts = {}", t.show(ts)), t.show(ts) == "{tv_sec = 1, tv_nsec = 5}");
    }
    // A 64-bit argument, whole: llseek's offset (high 1, low 0) is vfs_llseek's loff_t 1 << 32.
    {
        t.line("long long res").await;
        let ls = t.line("llseek(rfd, 1, 0, &res, 0)").await.s.unwrap();
        let lslast = t.last(&ls);
        let at = m.first_call("vfs_llseek", ls.first, lslast, Some(ls.task.addr));
        let off = at.map(|i| t.stack(i, None, i)).and_then(|s| s.into_iter().find(|f| f.func == "vfs_llseek")).map(|f| value(arg(&f, "offset")));
        t.check(format!("a 64-bit argument whole: vfs_llseek's offset {off:?}"), off.as_deref() == Some("4294967296"));
        let got = m.view(|v| v.lines(ls.first, lslast, ls.task.addr)).into_iter().find_map(|s| s.called.filter(|c| c.func == "vfs_llseek")).map(|c| c.value);
        t.check(format!("a 64-bit result whole: vfs_llseek returned {got:?}"), got == Some(4294967296));
    }
    t.check("line steps sit at their first store: some line is at a record past its block's, none before it, and a store is on its line",
            read_steps.iter().any(|s| s.at > s.i) && read_steps.iter().all(|s| s.at >= s.i)
            && read_steps.iter().any(|s| s.at > s.i && m.view(|v| v.at(s.at)).and_then(|a| a.line) == Some(s.line)));
    if let Some(s5) = read_steps.iter().find(|x| x.func == "ksys_read" && m.view(|v| v.at(x.i)).is_some_and(|a| a.kind == RecordKind::Block)) {
        let next = m.run_of(&s5.file, s5.line, s5.i - 1, 1);
        let back = m.run_of(&s5.file, s5.line, s5.i + 1, -1);
        t.check(format!("run to {}:{}, forward and back: its run", s5.file, s5.line), next == Some(s5.i) && back == Some(s5.i));
    }
    let ksys_last = read_steps.iter().rev().find(|x| x.func == "ksys_read").map_or(0, |x| x.i);
    let pos = |i: usize| t.stack(i, None, i).iter().find(|c| c.func == "ksys_read").map(|f| value(local(f, "pos")));
    t.check("a local on the stack, as it was: ksys_read's pos, 0 then 26", pos(in_read).as_deref() == Some("0") && pos(ksys_last).as_deref() == Some("26"));
    let ret = t.stack(ksys_last, None, ksys_last).iter().find(|c| c.func == "ksys_read").map(|f| value(local(f, "ret")));
    t.check(format!("a local in a wasm local: ksys_read's ret = {ret:?}"), ret.as_deref() == Some("26"));
    t.line("close(rfd)").await;
    let bad = t.line("write(1, 0x1000, 4)").await.ret;
    let pid = t.line("getpid()").await.ret;
    t.check("a bad user pointer: EFAULT", bad == Some(-14) && pid == Some(main as i64));
    // A char * shows its string: mkdir's struct filename, its name.
    {
        let mk = t.line("mkdirat(AT_FDCWD, \"/mnt/named\", 0755)").await.s.unwrap();
        let mklast = t.last(&mk);
        let i = m.first_call("filename_create", mk.first, mklast, None).unwrap_or(0);
        let a = t.stack(i, None, i).into_iter().find(|f| f.func == "filename_create").and_then(|f| f.args.unwrap_or_default().into_iter().find(|a| a.ty == "struct filename *"));
        let name = a.and_then(|a| a.open).map(|o| m.at_moment(i, |v, mem| value(child(&v.tables.types.object(mem, "name", o.addr, o.ty, 1), "name"))));
        t.check(format!("a char * with its string: filename->name = {name:?}"), name.is_some_and(|n| n.ends_with("\"/mnt/named\"")));
        t.line("unlinkat(AT_FDCWD, \"/mnt/named\", AT_REMOVEDIR)").await;
    }
    let def = |n: &str| m.view(|v| v.definition(n));
    t.check("definitions: a function, a type, a global",
            def("vfs_write").is_some_and(|d| d.file == "fs/read_write.c") && def("task_struct").is_some_and(|d| d.file == "include/linux/sched.h")
            && def("runqueues").is_some_and(|d| d.line > 0) && def("no_such").is_none()
            && def("list_add").is_some_and(|d| d.file == "include/linux/list.h" && d.last.is_none()));
    let member = |ty: Option<u32>, n: &str| m.view(|v| v.member_definition(ty, n));
    t.check("a field's declaration: file.f_pos, by its struct (or a pointer to it), or of the one struct with it; not of many",
            def("file.f_pos").is_some_and(|d| d.file == "include/linux/fs.h") && member(Some(file.id), "f_pos").is_some_and(|d| d.file == "include/linux/fs.h")
            && member(None, "f_pos").is_some_and(|d| d.file == "include/linux/fs.h") && member(None, "next").is_none());
    {
        let refused = m.make("io_setup(1, NULL)").err().unwrap_or_default();
        t.check(format!("the syscalls built: read and setuid, not io_setup ({refused})"),
                m.params("read").is_some() && m.params("setuid").is_some() && m.params("io_setup").is_none() && refused.contains("no syscall io_setup"));
        let su = t.line("setuid(0)").await;
        t.check(format!("setuid(0) = {:?}, recorded", su.ret), su.ret == Some(0) && su.s.as_ref().is_some_and(|s| s.name == "setuid") && t.ran(&su.s, "__sys_setuid"));
    }
    t.check("a syscall's parameters, as C declares them", m.params("openat") == Some(vec!["int dfd".into(), "char *filename".into(), "int flags".into(), "umode_t mode".into()]));
    {
        let op = m.view(|v| v.syscalls().into_iter().find(|s| s.name == "openat")).unwrap();
        let oplast = t.last(&op);
        let at = m.first_call("do_sys_openat2", op.first, oplast, None).unwrap_or(0);
        let stack = t.stack(at, None, at);
        let ops = m.view(|v| v.objects(op.first, oplast));
        t.check("a store through the task's files is named", ops.iter().any(|n| n.what.ends_with("->files") && n.changes.iter().any(|d| d.member == "next_fd")));
        let dso = m.view(|v| v.at(at));
        let dso_def = def("do_sys_openat2");
        t.check("a call begins at its signature line", dso.as_ref().zip(dso_def.as_ref()).is_some_and(|(a, d)| a.line == Some(d.line) && a.file.as_deref() == Some(&d.file)));
        let e = stack.last();
        let args: Vec<String> = e.map(|e| e.args.clone().unwrap_or_default().iter().map(|a| value(Some(a))).collect()).unwrap_or_default();
        t.check(format!("the entry frame: the syscall {:?}({})", e.and_then(|e| e.syscall.clone()), args.join(", ")),
                e.is_some_and(|e| e.func == "wasm_syscall" && e.syscall.as_deref() == Some("openat")) && args.first().map(String::as_str) == Some("AT_FDCWD")
                && args.get(2).map(String::as_str) == Some("O_WRONLY | O_CREAT | O_TRUNC")
                && stack.iter().any(|f| f.func == "__wasm_sys_openat" && !f.inlined && f.args.as_ref().and_then(|a| a.first()).is_some_and(|a| a.name == "dfd" && a.value.as_deref() == Some("AT_FDCWD"))));
    }
    {
        let mm = t.line("mmap2(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 7)").await.s.unwrap();
        let mmlast = t.last(&mm);
        let i = m.view(|v| v.lines(mm.first, mmlast, mm.task.addr)).into_iter().find(|x| x.func == "vm_mmap_pgoff").map_or(0, |x| x.i);
        let stack = t.stack(i, None, i);
        let e = stack.last();
        let args = e.and_then(|e| e.args.clone()).unwrap_or_default();
        t.check(format!("a syscall's six arguments, at the bottom of its stack: {:?}", e.and_then(|e| e.syscall.clone())),
                e.is_some_and(|e| e.syscall.as_deref() == Some("mmap2")) && args.len() == 6 && args[5].name == "pgoff" && args[5].value.as_deref() == Some("7"));
    }
    {
        let done = |text: &str| m.complete(text, text.len(), usize::MAX, &[]).into_iter().map(|c| (c.label, c.detail)).collect::<BTreeMap<_, _>>();
        let mut c = done("openat(AT_FDCWD, NULL, O_CR");
        c.extend(done("openat(AT_"));
        c.extend(done("close(ENOTE"));
        let flags = m.complete("openat(AT_FDCWD, NULL, O", 24, usize::MAX, &[]);
        let get = |k: &str| c.get(k).cloned().unwrap_or_default();
        t.check("the kernel's constants, completed; for flags, O_ first",
                get("O_CREAT") == "64" && get("AT_FDCWD") == "-100" && get("AT_REMOVEDIR") == "512" && get("ENOTEMPTY") == "39" && flags.first().is_some_and(|f| f.label.starts_with("O_")));
        let names = m.complete("clos", 4, usize::MAX, &[]);
        t.check("syscall names completed, with their signatures", names.first().is_some_and(|n| n.label == "close" && n.detail == "close(unsigned int fd)"));
    }
    let refusal = |text: &str, line: usize| m.can_make(text, line).err().unwrap_or_default();
    t.check("a line checked: a wrong member, a name not made yet, too many arguments",
            refusal("clock_nanosleep(1, 0, {.tv_min = 1}, NULL)", usize::MAX).contains("no member") && refusal("close(rfd)", 0).contains("rfd")
            && refusal("close(rfd)", usize::MAX).is_empty() && refusal("getpid(1)", usize::MAX).contains("arguments"));

    // The lessons: each on a fresh kernel, its calls finished, its links found; its tries, each made.
    for ex in &examples {
        let ids = t.program(&ex.steps).await;
        let is_call = |l: &String| matches!(statement(l).map(|s| s.line), Ok(Stmt::Call { .. }));
        let blocked: Vec<String> = ex.steps.iter().zip(&ids).filter(|(l, id)| is_call(l) && !id.is_some_and(|id| t.fin.contains_key(&id))).map(|(l, _)| l.clone()).collect();
        let calls: Vec<Option<u32>> = ex.steps.iter().zip(&ids).filter(|(l, _)| is_call(l)).map(|(_, id)| *id).collect();
        let mut missing = Vec::new();
        for p in &ex.text {
            for x in text::pieces(p) {
                let Piece::Link { target, .. } = x else { continue };
                let parts: Vec<&str> = target.split(':').collect();
                let Ok(n) = parts[0].parse::<usize>() else { continue };
                // (N, N:func, N:func:any: as the page goes there; with a tab, not checked.)
                let (func, any) = match parts.as_slice() {
                    [_] => (None, false),
                    [_, f] => (Some(*f), false),
                    [_, f, "any"] => (Some(*f), true),
                    _ => continue,
                };
                let s = calls.get(n - 1).copied().flatten().and_then(|id| t.syscall(id));
                let found = s.as_ref().is_some_and(|s| func.is_none_or(|f| m.first_call(f, s.first, t.last(s), (!any).then_some(s.task.addr)).is_some()));
                if !found {
                    missing.push(target.clone());
                }
            }
        }
        for (k, try_) in ex.tries.iter().enumerate() {
            let lines = m.program_lines(try_);
            let ran = t.program(&lines).await;
            t.check(format!("example {}, try {}: {} lines made", ex.name, k + 1, lines.len()), ran.len() == lines.len() && !lines.is_empty());
        }
        t.check(format!("example {}: {} lines, all finished, the explanation's links found (missing {missing:?}, blocked {blocked:?})", ex.name, ex.steps.len()),
                missing.is_empty() && blocked.is_empty());
    }

    // A fresh kernel, in place: nothing of the calls before (/tmp is gone).
    t.line("mkdirat(AT_FDCWD, \"/tmp\", 0755)").await;
    let again = t.line("mkdirat(AT_FDCWD, \"/tmp\", 0755)").await.ret;
    t.check("/tmp is there before", again == Some(-17));
    // (Restarted while a call is under way, its quiet not come: the calls after it still finish.)
    let _ = m.make("getpid()");
    let ignore = Closure::<dyn FnMut(JsValue)>::new(|_| {});
    let _ = m.quiet().catch(&ignore);
    ignore.forget();
    let _ = m.restart().await;
    let after = t.line("getpid()").await.ret;
    t.check(format!("a call after a restart cut one short finishes ({after:?})"), after == m.main().map(|p| p as i64));
    let pid = t.line("getpid()").await.ret;
    let names = js_sys::Array::from(&m.names_before(usize::MAX)).length();
    t.check("restarted: the program ready again, its names gone", m.runners().contains(&m.main().unwrap_or(0)) && pid == m.main().map(|p| p as i64) && names == 0);
    let made = t.line("mkdirat(AT_FDCWD, \"/tmp\", 0755)").await.ret;
    t.check("... on a fresh kernel: /tmp is not there", made == Some(0));
    // Deterministic: the same calls on a fresh kernel make the same recording, record for record.
    for name in ["create", "futex"] {
        let Some(ex) = examples.iter().find(|e| e.name == name) else { continue };
        let mut runs = Vec::new();
        for _ in 0..2 {
            let ids = t.program(&ex.steps).await;
            let seen: Vec<_> = ids.iter().flatten().map(|&id| t.syscall(id).map(|s| (s.id, s.first, s.last, t.fin.get(&id).map(|f| f.ret as i64)))).collect();
            runs.push((m.records(), seen));
        }
        t.check(format!("deterministic: the same calls, the same records ({name}: {} records)", runs[0].0), runs[0] == runs[1]);
    }
    // An execve with its argv (a vfork child runs the program again): it finishes as an exec.
    {
        let lines = ["pid_t child = clone(CLONE_VM | CLONE_VFORK | SIGCHLD, 0, NULL, NULL, 0)".to_string(), "child: execve(\"/bin/prog\", {\"prog\", \"-x\", NULL}, NULL)".to_string()];
        let ids = t.program(&lines).await;
        let how = ids[1].and_then(|id| t.fin.get(&id)).map(|f| f.how.clone());
        t.check(format!("an execve with argv runs the program ({how:?})"), how.as_deref() == Some("exec") && !t.out.contains("NULL argv"));
    }
    // LINUX_REBOOT_CMD_POWER_OFF (no result: the machine halts).
    let _ = m.restart().await;
    let _ = m.make("reboot(0xfee1dead, 672274793, 0x4321fedc, 0)");
    for _ in 0..1000 {
        if m.halted().is_some() {
            break;
        }
        sleep(10).await;
    }
    t.check("powered off", m.halted() == Some(0));
    t.console();
    t.check("no warning from the kernel (an execve has its argv)", !t.out.contains("NULL argv"));
    if t.failed {
        log(&t.out);
    }
    Ok(!t.failed)
}
