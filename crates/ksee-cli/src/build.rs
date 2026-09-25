//! `ksee build` and `ksee test`: the build, in order. Every step asks the
//! tool that knows its own dependencies (kbuild, cargo), or checks whether
//! its output is older than its inputs.
//!
//!   plugin/ksee-trace.so   the LLVM pass that instruments the kernel
//!   linux/vmlinux          the kernel, instrumented (kbuild)
//!   web/pkg/               the runtime, and wasm-bindgen's glue (cargo)
//!   web/vmlinux, web/src/  the kernel for the page (pack)
//!   web/initramfs.cpio     /init (user/init.c), and an ext4 disk image
//!                          (/disk.img, mkfs.ext4)
//!   web/examples/          index.json: the examples (user/examples/*.json)
//!   web/vendor/monaco/     the Monaco editor (downloaded once from npm)
//!
//! $LLVM is the LLVM version suffix (default -23).
use std::path::Path;
use std::process::Command;

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

    if stale("plugin/ksee-trace.so", &["plugin/ksee-trace.cpp"]) {
        eprintln!("  PLUGIN  plugin/ksee-trace.so");
        let flags = Command::new(format!("llvm-config{llvm}")).arg("--cxxflags").output()
            .unwrap_or_else(|e| crate::fail(&format!("llvm-config{llvm}: {e}"))).stdout;
        run(Command::new(format!("clang++{llvm}"))
            .args(["-shared", "-fPIC", "-O2", "plugin/ksee-trace.cpp", "-o", "plugin/ksee-trace.so"])
            .args(String::from_utf8_lossy(&flags).split_whitespace()));
    }

    // A hash of the plugin in the flags makes kbuild rebuild the kernel
    // when it changes.
    let id = format!("{:016x}", fnv64(&std::fs::read("plugin/ksee-trace.so").unwrap_or_default()));
    let plugin = Path::new("plugin/ksee-trace.so").canonicalize().unwrap();
    let kmake = |args: &[&str]| {
        let mut c = Command::new("make");
        c.args(["-s", "-C", "linux", "ARCH=wasm", &format!("LLVM={llvm}"),
                &format!("KCFLAGS=-g -fpass-plugin={} -DKSEE_ID={id}", plugin.display())])
            .args(args);
        c
    };
    if stale("linux/.config", &["linux/arch/wasm/configs/defconfig"]) {
        run(&mut kmake(&["defconfig"]));
    }
    eprintln!("  KERNEL  linux/vmlinux");
    run(&mut kmake(&[&format!("-j{}", std::thread::available_parallelism().map_or(4, |n| n.get()))]));

    eprintln!("  RUNTIME web/pkg");
    run(Command::new("cargo").args(["build", "-q", "--release", "-p", "ksee-runtime",
        "--target", "wasm32-unknown-unknown", "-Zbuild-std=std,panic_abort"]));
    run(Command::new("wasm-bindgen").args(["--target", "web", "--out-dir", "web/pkg",
        "target/wasm32-unknown-unknown/release/ksee_runtime.wasm"]));

    vendor();
    if stale("web/vmlinux", &["linux/vmlinux"]) {
        crate::pack::pack(Path::new("linux"), Path::new("web"));
    }
    // /init uses nolibc, with the kernel's UAPI headers.
    run(&mut kmake(&["headers"]));
    let inputs = ["user/init.c", "user/initramfs.list", "linux/usr/gen_init_cpio",
                  "linux/tools/include/nolibc/arch-wasm.h"];
    if stale("web/initramfs.cpio", &inputs) {
        eprintln!("  INIT    web/initramfs.cpio");
        run(Command::new(format!("clang{llvm}"))
            .args(["--target=wasm32", "-O2", "-g0", "-ffreestanding", "-nostdlib", "-Wl,--no-entry", "-Wl,--export=_start",
                   "-nostdinc", "-isystem", "linux/tools/include/nolibc", "-isystem", "linux/usr/include",
                   "-o", "web/init.wasm", "user/init.c"]));
        // The ext4 disk /init mounts (a file, through the loop device).
        let _ = std::fs::remove_file("web/disk.img");
        run(Command::new("mkfs.ext4").args(["-q", "-F", "-b", "1024", "-L", "ksee", "-N", "32", "web/disk.img", "2M"]));
        // (The list's paths are relative to web/.)
        let tool = Path::new("linux/usr/gen_init_cpio").canonicalize()
            .unwrap_or_else(|e| crate::fail(&format!("linux/usr/gen_init_cpio: {e}")));
        let cpio = Command::new(tool).arg(Path::new("user/initramfs.list").canonicalize().unwrap()).current_dir("web").output()
            .unwrap_or_else(|e| crate::fail(&format!("gen_init_cpio: {e}")));
        if !cpio.status.success() {
            crate::fail(&format!("gen_init_cpio: {}", String::from_utf8_lossy(&cpio.stderr)));
        }
        std::fs::write("web/initramfs.cpio", cpio.stdout).unwrap_or_else(|e| crate::fail(&format!("web/initramfs.cpio: {e}")));
    }
    examples();
}

/// The examples' order on the page, from the simplest (others after them).
const EXAMPLES: &[&str] = &["hello", "read", "open-fails", "sleep", "file", "dirs", "yours"];

/// web/examples/index.json: the examples (user/examples/*.json: each a list
/// of syscalls for /init to make, one at a time), with their names.
fn examples() {
    let mut index: Vec<(usize, String)> = std::fs::read_dir("user/examples").into_iter().flatten().flatten()
        .map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            let text = String::from_utf8_lossy(&crate::read(&p)).into_owned();
            let entry = format!("  {{ \"name\": \"{name}\", {}", text.trim().strip_prefix('{').unwrap_or("}").trim_start());
            (EXAMPLES.iter().position(|&e| e == name).unwrap_or(EXAMPLES.len()), entry)
        }).collect();
    index.sort();
    let index: Vec<String> = index.into_iter().map(|(_, e)| e).collect();
    std::fs::create_dir_all("web/examples")
        .and_then(|_| std::fs::write("web/examples/index.json", format!("[\n{}\n]\n", index.join(",\n"))))
        .unwrap_or_else(|e| crate::fail(&format!("web/examples/index.json: {e}")));
}

/// The Monaco editor (the page's source view), its prebuilt AMD build
/// (package/min/vs), in web/vendor/monaco/vs.
const MONACO: &str = "0.57.0";

fn vendor() {
    let dir = format!("web/vendor/monaco-{MONACO}");
    if Path::new(&format!("{dir}/vs/loader.js")).exists() {
        return;
    }
    eprintln!("  VENDOR  {dir} (npm: monaco-editor {MONACO})");
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| crate::fail(&format!("{dir}: {e}")));
    let url = format!("https://registry.npmjs.org/monaco-editor/-/monaco-editor-{MONACO}.tgz");
    run(Command::new("sh").arg("-c").arg(format!(
        "curl -fsSL '{url}' | tar xz -C '{dir}' --strip-components=2 package/min/vs")));
    // (A fixed name for the page.)
    let _ = std::fs::remove_file("web/vendor/monaco");
    std::os::unix::fs::symlink(format!("monaco-{MONACO}"), "web/vendor/monaco")
        .unwrap_or_else(|e| crate::fail(&format!("web/vendor/monaco: {e}")));
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
