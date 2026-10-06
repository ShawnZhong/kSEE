use super::harness::{sleep, until, KernelHarness};
use crate::state::state;

pub(super) async fn run(t: &mut KernelHarness) {
    let m = t.machine.clone();
    // A fresh kernel, in place: nothing of the calls before (/tmp is gone).
    t.line("mkdirat(AT_FDCWD, \"/tmp\", 0755)").await;
    let again = t.line("mkdirat(AT_FDCWD, \"/tmp\", 0755)").await.ret;
    t.check("/tmp is there before", again == Some(-17));
    // (Restarted while a call is under way, its quiet not come: the calls after it still finish.)
    let _ = m.make(None, "getpid()");
    // (Its quiet waited for, as the restart comes: the restart rejects it.)
    let waiting = m.clone();
    wasm_bindgen_futures::spawn_local(async move { let _ = waiting.quiet().await; });
    sleep(0).await;
    let _ = m.restart().await;
    let after = t.line("getpid()").await.ret;
    t.check(format!("a call after a restart cut one short finishes ({after:?})"), after == state(|s| s.session.main).map(|p| p as i64));
    let pid = t.line("getpid()").await.ret;
    t.check("restarted: the program ready again, its names gone", state(|s| s.session.main.is_some_and(|p| s.session.runners.contains_key(&p) && pid == Some(p as i64)) && s.session.names.before(usize::MAX).is_empty()));
    let made = t.line("mkdirat(AT_FDCWD, \"/tmp\", 0755)").await.ret;
    t.check("... on a fresh kernel: /tmp is not there", made == Some(0));
    // Deterministic: the same calls on a fresh kernel make the same recording, record for record.
    // (A file made; a thread waiting on a futex, woken.)
    let made: &[(Option<&str>, &str)] = &[(None, "int fd = openat(AT_FDCWD, \"/new.txt\", O_CREAT | O_WRONLY, 0644)")];
    let futex: &[(Option<&str>, &str)] = &[
        (None, "void *mem = mmap2(NULL, 65536, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0)"),
        (None, "pid_t thread = clone(CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD | CLONE_SYSVSEM, mem + 65536, NULL, NULL, 0)"),
        (Some("thread"), "futex(mem, FUTEX_WAIT_PRIVATE, 1, NULL)"),
        (Some("thread"), "futex(mem, FUTEX_WAIT_PRIVATE, 0, NULL)"),
        (None, "futex(mem, FUTEX_WAKE_PRIVATE, 1, NULL)"),
    ];
    for (name, program) in [("a file made", made), ("a futex", futex)] {
        let mut runs = Vec::new();
        for _ in 0..2 {
            let ids = t.program(program.iter().copied()).await;
            let seen: Vec<_> = ids.iter().flatten().map(|&id| t.syscall(id).map(|s| (s.first, s.last, t.finished(id)))).collect();
            runs.push((m.view(|v| v.rec.len()), seen));
        }
        t.check(format!("deterministic: the same calls, the same records ({name}: {} records)", runs[0].0), runs[0] == runs[1]);
    }
    // An execve with its argv (a vfork child runs the program again): it finishes as an exec.
    {
        let ids = t.program([(None, "pid_t child = clone(CLONE_VM | CLONE_VFORK | SIGCHLD, 0, NULL, NULL, 0)"),
                             (Some("child"), "execve(\"/bin/prog\", {\"prog\", \"-x\", NULL}, NULL)")]).await;
        let ended = ids[1].and_then(|id| t.finished(id));
        t.check(format!("an execve with argv runs the program, no return ({ended:?})"), ended == Some(None) && !t.console_output.contains("NULL argv"));
    }
    // LINUX_REBOOT_CMD_POWER_OFF (no result: the machine halts).
    let _ = m.restart().await;
    let _ = m.make(None, "reboot(0xfee1dead, 672274793, 0x4321fedc, 0)");
    until(|| state(|s| s.halted).is_some(), 10, 1000).await;
    t.check("powered off", state(|s| s.halted) == Some(0));
    t.console();
    t.check("no warning from the kernel (an execve has its argv)", !t.console_output.contains("NULL argv"));
}
