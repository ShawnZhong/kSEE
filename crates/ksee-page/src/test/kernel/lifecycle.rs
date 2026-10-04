use crate::lesson::Step;

use super::harness::{sleep, KernelHarness};

pub(super) async fn run(t: &mut KernelHarness, examples: &[crate::lesson::Example]) {
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
    t.check(format!("a call after a restart cut one short finishes ({after:?})"), after == m.main().map(|p| p as i64));
    let pid = t.line("getpid()").await.ret;
    t.check("restarted: the program ready again, its names gone", m.runners().contains(&m.main().unwrap_or(0)) && pid == m.main().map(|p| p as i64) && m.names_before(usize::MAX).is_empty());
    let made = t.line("mkdirat(AT_FDCWD, \"/tmp\", 0755)").await.ret;
    t.check("... on a fresh kernel: /tmp is not there", made == Some(0));
    // Deterministic: the same calls on a fresh kernel make the same recording, record for record.
    for name in ["create", "futex"] {
        let Some(ex) = examples.iter().find(|e| e.name == name) else { continue };
        let mut runs = Vec::new();
        for _ in 0..2 {
            let ids = t.program(&ex.steps).await;
            let seen: Vec<_> = ids.iter().flatten().map(|&id| t.syscall(id).map(|s| (s.first, s.last, t.finished.get(&id).map(|f| f.ret)))).collect();
            runs.push((m.records(), seen));
        }
        t.check(format!("deterministic: the same calls, the same records ({name}: {} records)", runs[0].0), runs[0] == runs[1]);
    }
    // An execve with its argv (a vfork child runs the program again): it finishes as an exec.
    {
        let lines = [Step::Main("pid_t child = clone(CLONE_VM | CLONE_VFORK | SIGCHLD, 0, NULL, NULL, 0)".into()),
                     Step::Task { task: "child".into(), text: "execve(\"/bin/prog\", {\"prog\", \"-x\", NULL}, NULL)".into() }];
        let ids = t.program(&lines).await;
        let how = ids[1].and_then(|id| t.finished.get(&id)).map(|f| f.how.to_string());
        t.check(format!("an execve with argv runs the program ({how:?})"), how.as_deref() == Some("exec") && !t.console_output.contains("NULL argv"));
    }
    // LINUX_REBOOT_CMD_POWER_OFF (no result: the machine halts).
    let _ = m.restart().await;
    let _ = m.make(None, "reboot(0xfee1dead, 672274793, 0x4321fedc, 0)");
    for _ in 0..1000 {
        if m.halted().is_some() {
            break;
        }
        sleep(10).await;
    }
    t.check("powered off", m.halted() == Some(0));
    t.console();
    t.check("no warning from the kernel (an execve has its argv)", !t.console_output.contains("NULL argv"));
}
