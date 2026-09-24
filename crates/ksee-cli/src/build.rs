//! `ksee build` and `ksee test`: the build, in order. Every step asks the
//! tool that knows its own dependencies (kbuild, cargo), or checks whether
//! its output is older than its inputs.
//!
//!   plugin/ksee-trace.so   the LLVM pass that instruments the kernel
//!   linux/vmlinux          the kernel, instrumented (kbuild)
//!   web/pkg/               the runtime, and wasm-bindgen's glue (cargo)
//!   web/vmlinux, web/src/  the kernel for the page (pack)
//!   web/initramfs.cpio     /init (user/init.c)
//!
//! $LLVM is the LLVM version suffix (default -23); $KSEE_TRACE and
//! $KSEE_CURRENT override what the plugin instruments.
use std::path::Path;
use std::process::Command;

/// What the plugin instruments (see plugin/ksee-trace.cpp): every basic
/// block, and the calls that give blocks context.
const TRACE: &str = "*:block,wasm_syscall:enter+exit,context_switch:enter+exit,\
                     __schedule,try_to_wake_up,wake_up_new_task,kernel_clone,do_exit";
/// The kernel variable that holds the current task.
const CURRENT: &str = "__current_task";

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

fn run(cmd: &mut Command) {
    let status = cmd.status().unwrap_or_else(|e| crate::fail(&format!("{:?}: {e}", cmd.get_program())));
    if !status.success() {
        crate::fail(&format!("{:?} failed", cmd.get_program()));
    }
}

/// Whether `out` is missing or older than any of `inputs`.
fn stale(out: &str, inputs: &[&str]) -> bool {
    let time = |p: &str| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let Some(t) = time(out) else { return true };
    inputs.iter().any(|i| time(i).is_none_or(|ti| ti > t))
}

pub fn build() {
    let llvm = env("LLVM", "-23");
    let (trace, current) = (env("KSEE_TRACE", TRACE), env("KSEE_CURRENT", CURRENT));

    if stale("plugin/ksee-trace.so", &["plugin/ksee-trace.cpp"]) {
        eprintln!("  PLUGIN  plugin/ksee-trace.so");
        let flags = Command::new(format!("llvm-config{llvm}")).arg("--cxxflags").output()
            .unwrap_or_else(|e| crate::fail(&format!("llvm-config{llvm}: {e}"))).stdout;
        run(Command::new(format!("clang++{llvm}"))
            .args(["-shared", "-fPIC", "-O2", "plugin/ksee-trace.cpp", "-o", "plugin/ksee-trace.so"])
            .args(String::from_utf8_lossy(&flags).split_whitespace()));
    }

    // A hash of the plugin and its settings in the flags makes kbuild
    // rebuild the kernel when either changes.
    let mut id = std::fs::read("plugin/ksee-trace.so").unwrap_or_default();
    id.extend(format!("{trace} {current}").bytes());
    let id = format!("{:016x}", fnv64(&id));
    let plugin = Path::new("plugin/ksee-trace.so").canonicalize().unwrap();
    let kmake = |args: &[&str]| {
        let mut c = Command::new("make");
        c.args(["-s", "-C", "linux", "ARCH=wasm", &format!("LLVM={llvm}"),
                &format!("KCFLAGS=-g -fpass-plugin={} -DKSEE_ID={id}", plugin.display())])
            .args(args).env("KSEE_TRACE", &trace).env("KSEE_CURRENT", &current);
        c
    };
    if !Path::new("linux/.config").exists() {
        run(&mut kmake(&["defconfig"]));
    }
    eprintln!("  KERNEL  linux/vmlinux");
    run(&mut kmake(&[&format!("-j{}", std::thread::available_parallelism().map_or(4, |n| n.get()))]));

    eprintln!("  RUNTIME web/pkg");
    run(Command::new("cargo").args(["build", "-q", "--release", "-p", "ksee-runtime",
        "--target", "wasm32-unknown-unknown", "-Zbuild-std=std,panic_abort"]));
    run(Command::new("wasm-bindgen").args(["--target", "web", "--out-dir", "web/pkg",
        "target/wasm32-unknown-unknown/release/ksee_runtime.wasm"]));

    if stale("web/vmlinux", &["linux/vmlinux"]) {
        crate::pack::pack(Path::new("linux"), Path::new("web"));
    }
    if stale("web/initramfs.cpio", &["user/init.c", "user/initramfs.list", "linux/usr/gen_init_cpio"]) {
        eprintln!("  INIT    web/initramfs.cpio");
        run(Command::new(format!("clang{llvm}")).args(["--target=wasm32", "-O2", "-ffreestanding", "-nostdlib",
            "-Wl,--no-entry", "-Wl,--export=_start", "-o", "web/init.wasm", "user/init.c"]));
        let cpio = Command::new("linux/usr/gen_init_cpio").arg("../user/initramfs.list").current_dir("web")
            .output().unwrap_or_else(|e| crate::fail(&format!("gen_init_cpio: {e}")));
        std::fs::write("web/initramfs.cpio", cpio.stdout).unwrap_or_else(|e| crate::fail(&format!("web/initramfs.cpio: {e}")));
    }
}

/// Build, then: core's unit tests and tests/web.js (under Deno); with
/// `ui`, also tests/ui.js (the page in headless Chromium, $CHROME).
pub fn test(ui: bool) {
    build();
    run(Command::new("cargo").args(["test", "-q", "-p", "ksee-core"]));
    run(Command::new("deno").args(["run", "--allow-read", "tests/web.js"]));
    if ui {
        run(Command::new("deno").args(["run", "-A", "tests/ui.js", "web/ui.png"]));
    }
}

fn fnv64(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}
