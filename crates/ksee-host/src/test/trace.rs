use ksee_core::view::RegionKind;

use super::harness::{arg, child, until, value, KernelHarness};

pub(super) async fn run(t: &mut KernelHarness, main: u32) {
    let m = t.machine.clone();
    // Calls, traced.
    let w = t.line("write(1, \"hello kSEE\\n\", 11)").await.syscall.unwrap();
    until(|| { t.console(); t.console_output.contains("hello kSEE") }, 10, 100).await;
    t.check(format!("a write, by the program ({}, pid {:?}), shown on the console", w.task.comm.clone().unwrap_or_default(), w.task.pid),
            w.ret == Some(11) && w.task.comm.as_deref() == Some("prog") && w.task.pid == Some(main as i32) && t.console_output.contains("hello kSEE"));
    t.check("write ran vfs_write", t.ran(&Some(w.clone()), "vfs_write"));
    let s = t.line("getpid()").await.syscall;
    t.check("getpid ran __task_pid_nr_ns", t.ran(&s, "__task_pid_nr_ns"));
    let s = t.line("mkdirat(AT_FDCWD, \"/tmp\", 0755)").await.syscall;
    t.check("mkdirat ran vfs_mkdir", t.ran(&s, "vfs_mkdir"));
    let nap = t.line("clock_nanosleep(CLOCK_MONOTONIC, 0, {0, 30000000}, NULL)").await.syscall.unwrap();
    let idle = m.view(|v| v.chart(v.rec.step_of(nap.first), v.rec.step_of(nap.last), 50).cols.iter().any(|c| v.task(c.task, nap.first).comm.as_deref() == Some("swapper/0")));
    t.check("sleeping switched to the idle task and back", t.ran(&Some(nap.clone()), "__schedule") && idle);
    let long = t.line("clock_nanosleep(CLOCK_MONOTONIC, 0, {2, 0}, NULL)").await;
    t.check("a 2 s sleep returns (the clock skipped ahead)", long.ret == Some(0));
    let sel = t.line("pselect6(0, NULL, NULL, NULL, {1, 0}, NULL)").await;
    t.check("a 1 s select() times out", sel.ret == Some(0));
    m.snapshot();
    let wi = m.view(|v| v.rec.runs.iter().position(|s| s.first == w.first));
    let vfs = m.view(|v| v.definition("vfs_write"));
    t.check("vfs_write: where it is", vfs.as_ref().is_some_and(|d| d.file == "fs/read_write.c" && d.last.is_some_and(|l| l > d.line)));
    {
        let cs = m.view(|v| (0..v.rec.runs.len()).filter_map(|k| v.run(k)).find(|s| s.name == "mkdirat").map(|mk| v.objects(mk.first, mk.last))).unwrap_or_default();
        let globals: Vec<String> = cs.iter().filter(|c| c.kind == RegionKind::Global).map(|c| c.what.clone()).collect();
        t.check(format!("mkdirat's objects name the kernel's globals: {}; not the machine's", globals.join(", ")),
                globals.iter().any(|g| g == "nr_inodes") && !cs.iter().any(|c| c.what == "wasm_irq_flags"));
    }
    t.check("fs/read_write.c coverage", m.view(|v| v.coverage("fs/read_write.c", wi)).values().any(|&n| n > 0));
    let wlast = w.last;
    let chart = m.view(|v| v.chart(v.rec.step_of(w.first), v.rec.step_of(wlast), 200));
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
    t.check(format!("a syscall's entry is the outermost bar: {}", chart.bars[0].name), chart.bars[0].name == "wasm_syscall");
    {
        // (Each column is a moment, its record's line: the Stack there has every call it draws.)
        let (mut drawn, mut missing) = (0, Vec::new());
        for c in &chart.cols {
            let (_, nth, cut) = m.view(|v| v.rec.steps[c.step]);
            let frames = m.view(|v| v.call_stack(c.i, Some(nth as usize), cut as usize, c.i));
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
    let undone = |i: usize| m.view(|v| v.rec.stores().between(i + 1, usize::MAX).len());
    t.check("memory as it was: writes undone", undone(w.first) > undone(wlast));
    let in_write = m.first_call("vfs_write", w.first, wlast, Some(w.task.addr)).unwrap_or(0);
    let comm = m.view(|v| v.task_fields(w.task.addr, in_write)).into_iter().find(|c| c.name == "comm").and_then(|c| c.value);
    t.check("the task, as it was", comm.as_deref() == Some("\"prog\""));
    let rq = m.at_moment(in_write, |v, mem| v.tables.types.globals.get("runqueues").map(|&(a, ty)| v.tables.types.object(mem, "runqueues", a, ty, 1)));
    t.check("a global", rq.is_some_and(|o| child(&o, "nr_running").is_some()));

}
