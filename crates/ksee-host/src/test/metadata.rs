use std::collections::BTreeMap;

use super::harness::{value, KernelHarness};

pub(super) async fn run(t: &mut KernelHarness) {
    let m = t.machine.clone();
    let def = |n: &str| m.view(|v| v.definition(n));
    t.check("definitions: a function, a type, a global",
            def("vfs_write").is_some_and(|d| d.file == "fs/read_write.c") && def("task_struct").is_some_and(|d| d.file == "include/linux/sched.h")
            && def("runqueues").is_some_and(|d| d.line > 0) && def("no_such").is_none()
            && def("list_add").is_some_and(|d| d.file == "include/linux/list.h" && d.last.is_none()));
    let member = |n: &str| m.view(|v| v.member_definition(n));
    t.check("a field's declaration: file.f_pos, by its struct, or of the one struct with it; not of many",
            def("file.f_pos").is_some_and(|d| d.file == "include/linux/fs.h")
            && member("f_pos").is_some_and(|d| d.file == "include/linux/fs.h") && member("next").is_none());
    {
        let aio = m.make(None, "io_setup(1, NULL)").err().unwrap_or_default();
        let uring = m.make(None, "io_uring_setup(1, NULL)").err().unwrap_or_default();
        let uring_calls = m.complete("io_uring", 8, usize::MAX, &[]);
        let uring_constants = m.complete("close(IORING_", 13, usize::MAX, &[]);
        t.check(format!("the syscalls built: read and setuid, not aio or io_uring ({aio}; {uring})"),
                m.tables().types.params_of("read").is_some() && m.tables().types.params_of("setuid").is_some() && m.tables().types.params_of("io_setup").is_none()
                && m.tables().types.params_of("io_uring_setup").is_none() && aio.contains("no syscall io_setup") && uring.contains("no syscall io_uring_setup")
                && uring_calls.is_empty() && uring_constants.is_empty());
        let su = t.line("setuid(0)").await;
        t.check(format!("setuid(0) = {:?}, recorded", su.ret), su.ret == Some(0) && su.syscall.as_ref().is_some_and(|s| s.name == "setuid") && t.ran(&su.syscall, "__sys_setuid"));
    }
    t.check("a syscall's parameters, as C declares them", m.tables().types.params_of("openat") == Some(vec!["int dfd".into(), "char *filename".into(), "int flags".into(), "umode_t mode".into()]));
    {
        let op = m.view(|v| (0..v.rec.runs.len()).filter_map(|k| v.run(k)).find(|s| s.name == "openat")).unwrap();
        let oplast = op.last;
        let at = m.first_call("do_sys_openat2", op.first, oplast, None).unwrap_or(0);
        let stack = t.stack(at, None, at);
        let ops = m.view(|v| v.objects(op.first, oplast));
        t.check("a store through the task's files is named", ops.iter().any(|n| n.what.ends_with("->files") && n.changes.iter().any(|d| d.member == "next_fd")));
        let dso_def = def("do_sys_openat2");
        t.check("a call begins at its signature line", stack.first().zip(dso_def.as_ref()).is_some_and(|(a, d)| a.line == Some(d.line) && a.file.as_deref() == Some(&d.file)));
        let e = stack.last();
        let args: Vec<String> = e.map(|e| e.args.iter().map(|a| value(Some(a))).collect()).unwrap_or_default();
        t.check(format!("the entry frame: the syscall's arguments ({})", args.join(", ")),
                e.is_some_and(|e| e.func == "wasm_syscall") && args.first().map(String::as_str) == Some("AT_FDCWD")
                && args.get(2).map(String::as_str) == Some("O_WRONLY | O_CREAT | O_TRUNC")
                && stack.iter().any(|f| f.func == "__wasm_sys_openat" && !f.inlined && f.args.first().is_some_and(|a| a.name == "dfd" && a.value.as_deref() == Some("AT_FDCWD"))));
    }
    {
        let mm = t.line("mmap2(NULL, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 7)").await.syscall.unwrap();
        let mmlast = mm.last;
        let i = m.view(|v| v.lines(mm.first, mmlast, mm.task.addr)).into_iter().find(|x| x.func == "vm_mmap_pgoff").map_or(0, |x| x.i);
        let stack = t.stack(i, None, i);
        let e = stack.last();
        let args = e.map(|e| e.args.clone()).unwrap_or_default();
        t.check(format!("a syscall's six arguments, at the bottom of its stack: {:?}", e.map(|e| &e.func)),
                e.is_some_and(|e| e.func == "wasm_syscall") && args.len() == 6 && args[5].name == "pgoff" && args[5].value.as_deref() == Some("7"));
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
        t.check("syscall names completed, with their parameters", names.first().is_some_and(|n| n.label == "close" && n.detail == "(unsigned int fd)"));
    }
    let refusal = |text: &str, line: usize| m.can_make(None, text, line).err().unwrap_or_default();
    t.check("a line checked: too many positional values, a name not made yet, too many arguments",
            refusal("clock_nanosleep(1, 0, {1, 2, 3}, NULL)", usize::MAX).contains("too many") && refusal("close(rfd)", 0).contains("rfd")
            && refusal("close(rfd)", usize::MAX).is_empty() && refusal("getpid(1)", usize::MAX).contains("arguments"));
}
