use std::collections::BTreeMap;

use super::harness::{value, KernelHarness};

pub(super) async fn run(t: &mut KernelHarness, file_id: u32) {
    let m = t.machine.clone();
    let def = |n: &str| m.view(|v| v.definition(n));
    t.check("definitions: a function, a type, a global",
            def("vfs_write").is_some_and(|d| d.file == "fs/read_write.c") && def("task_struct").is_some_and(|d| d.file == "include/linux/sched.h")
            && def("runqueues").is_some_and(|d| d.line > 0) && def("no_such").is_none()
            && def("list_add").is_some_and(|d| d.file == "include/linux/list.h" && d.last.is_none()));
    let member = |ty: Option<u32>, n: &str| m.view(|v| v.member_definition(ty, n));
    t.check("a field's declaration: file.f_pos, by its struct (or a pointer to it), or of the one struct with it; not of many",
            def("file.f_pos").is_some_and(|d| d.file == "include/linux/fs.h") && member(Some(file_id), "f_pos").is_some_and(|d| d.file == "include/linux/fs.h")
            && member(None, "f_pos").is_some_and(|d| d.file == "include/linux/fs.h") && member(None, "next").is_none());
    {
        let aio = m.make(None, "io_setup(1, NULL)").err().unwrap_or_default();
        let uring = m.make(None, "io_uring_setup(1, NULL)").err().unwrap_or_default();
        let uring_calls = m.complete("io_uring", 8, usize::MAX, &[]);
        let uring_constants = m.complete("close(IORING_", 13, usize::MAX, &[]);
        t.check(format!("the syscalls built: read and setuid, not aio or io_uring ({aio}; {uring})"),
                m.params("read").is_some() && m.params("setuid").is_some() && m.params("io_setup").is_none()
                && m.params("io_uring_setup").is_none() && aio.contains("no syscall io_setup") && uring.contains("no syscall io_uring_setup")
                && uring_calls.is_empty() && uring_constants.is_empty());
        let su = t.line("setuid(0)").await;
        t.check(format!("setuid(0) = {:?}, recorded", su.ret), su.ret == Some(0) && su.syscall.as_ref().is_some_and(|s| s.name == "setuid") && t.ran(&su.syscall, "__sys_setuid"));
    }
    t.check("a syscall's parameters, as C declares them", m.params("openat") == Some(vec!["int dfd".into(), "char *filename".into(), "int flags".into(), "umode_t mode".into()]));
    {
        let op = m.view(|v| v.syscalls().into_iter().find(|s| s.name == "openat")).unwrap();
        let oplast = t.last(&op);
        let at = m.first_call("do_sys_openat2", op.first, oplast, None).unwrap_or(0);
        let stack = t.stack(at, None, at);
        let ops = m.view(|v| v.objects(op.first, oplast));
        t.check("a store through the task's files is named", ops.iter().any(|n| n.what.ends_with("->files") && n.changes.iter().any(|d| d.member == "next_fd")));
        let dso_def = def("do_sys_openat2");
        t.check("a call begins at its signature line", stack.first().zip(dso_def.as_ref()).is_some_and(|(a, d)| a.line == Some(d.line) && a.file.as_deref() == Some(&d.file)));
        let e = stack.last();
        let args: Vec<String> = e.map(|e| e.args.clone().unwrap_or_default().iter().map(|a| value(Some(a))).collect()).unwrap_or_default();
        t.check(format!("the entry frame: the syscall {:?}({})", e.and_then(|e| e.syscall.clone()), args.join(", ")),
                e.is_some_and(|e| e.func == "wasm_syscall" && e.syscall.as_deref() == Some("openat")) && args.first().map(String::as_str) == Some("AT_FDCWD")
                && args.get(2).map(String::as_str) == Some("O_WRONLY | O_CREAT | O_TRUNC")
                && stack.iter().any(|f| f.func == "__wasm_sys_openat" && !f.inlined && f.args.as_ref().and_then(|a| a.first()).is_some_and(|a| a.name == "dfd" && a.value.as_deref() == Some("AT_FDCWD"))));
    }
    {
        let mm = t.line("mmap2(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 7)").await.syscall.unwrap();
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
    let refusal = |text: &str, line: usize| m.can_make(None, text, line).err().unwrap_or_default();
    t.check("a line checked: too many positional values, a name not made yet, too many arguments",
            refusal("clock_nanosleep(1, 0, {1, 2, 3}, NULL)", usize::MAX).contains("too many") && refusal("close(rfd)", 0).contains("rfd")
            && refusal("close(rfd)", usize::MAX).is_empty() && refusal("getpid(1)", usize::MAX).contains("arguments"));
}
