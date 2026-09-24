// Boot the kernel under Deno with the page's own modules (web/ksee.js,
// web/notebook.js), make syscalls in /init from notebook lines, use the
// debugger, and check what the trace shows.
//   deno run --allow-read tests/web.js
import { start, process } from "../web/ksee.js";
import { Notebook, EXAMPLES } from "../web/notebook.js";

const root = new URL("..", import.meta.url);
// wasm-bindgen's futures (with threads) wake through Atomics.waitAsync,
// which Deno does not count as pending work: keep the process alive.
const alive = setInterval(() => {}, 1000);
const m = await start({
  kernel: new URL("web/vmlinux", root),
  initrd: new URL("web/initramfs.cpio", root),
});

let out = "", failed = false;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const check = (what, ok) => {
  console.log(`${ok ? "ok" : "FAIL"}: ${what}`);
  failed ||= !ok;
  return ok;
};
async function until(pred, ms = 10000) {
  for (const t0 = Date.now(); Date.now() - t0 < ms; await sleep(10)) {
    out += m.console().replaceAll("\r", "");
    const v = pred();
    if (v) return v;
  }
}
const table = m.syscall_table();
const nb = new Notebook(m, process(m));
// Run notebook lines; the last one's result.
async function run(text) {
  let r;
  for (const line of text.split("\n")) r = await nb.run(line);
  return r;
}

check("init ready", await until(() => out.includes("ready for the page's syscalls")));
const write = table.find((s) => s.name === "write");
check("syscall table has signatures",
      write.params.map((p) => `${p.type} ${p.name} ${p.kind}`).join(", ") ===
      "unsigned int fd value, const char * buf in, size_t count value");
check("getpid = 1", (await run("getpid()")).text === "= 1");
check("write", (await run('write(1, "hello kSEE\\n", 11)')).text === "= 11" && await until(() => out.includes("hello kSEE")));
const files = await run(EXAMPLES.files.split("\n").slice(0, 3).join("\n"));
check("getdents64 lists /", files.text.startsWith("= ") && files.shown[0].includes("init") && files.shown[0].includes("dev"));
check("files example", (await run(EXAMPLES.files)).text === "= 0");
check("read it back", (await run(EXAMPLES["read back"])).text === "= 0" && (await nb.show("buf")) === 'buf: "hello kSEE\\n"');
check("writev a struct iovec[2]", (await run(EXAMPLES.writev)).text === "= 18" && await until(() => out.includes("from writev")));
const pipe = await run(EXAMPLES.pipe);
check("pipe: indexing an int[2]", pipe.text === "= 14" && pipe.shown[0] === 'buf: "through a pipe"');
const uname = await run(EXAMPLES.uname);
check("uname decodes struct new_utsname", uname.shown[0]?.includes('machine = "wasm"'));
check("clock_nanosleep_time64", (await run(EXAMPLES.sleep)).text === "= 0");
let bad;
try { await nb.run("nosuch(1)"); } catch (e) { bad = e.message; }
check("unknown syscalls are errors", bad === "no syscall nosuch");
await nb.reset();
check("reset forgets values", await nb.run("buf").then(() => false, () => true));

// Stop in vfs_write, look around, step, go on.
check("breakpoint set", m.break_function("vfs_write", true));
const pending = nb.run('write(1, "hi\\n", 3)');
const stop = await until(() => m.stopped());
check("stopped in vfs_write", stop?.func === "vfs_write" && stop.file === "fs/read_write.c");
check("stopped task is init", stop?.task.comm === "init" && stop.task.pid === 1);
const task = stop && m.object("task_struct", stop.task.addr, 1);
check("decode the task while stopped", task?.children?.find((c) => c.name === "comm")?.value === '"init"');
check("decode a global", m.global("runqueues", 1)?.children?.some((c) => c.name === "nr_running"));
m.step();
check("step to the next block", !!await until(() => {
  const s = m.stopped();
  return s && (s.func !== stop.func || s.block !== stop.block) && s;
}));
m.break_function("vfs_write", false);
m.resume();
check("resumed", (await pending).text === "= 3");

m.snapshot();
const calls = m.syscalls().filter((s) => !s.control);
const called = calls.map((s) => s.name);
for (const n of ["getpid", "write", "mkdirat", "openat", "getdents64", "writev", "pipe2", "uname", "clock_nanosleep_time64"])
  check(`syscall ${n} traced`, called.includes(n));
check("init's channel traffic is marked", m.syscalls().some((s) => s.control && s.name === "read"));
const ran = (name, func) => calls.some((s) => s.name === name && m.runs(s.index).some((r) => r.func === func));
const w = calls.find((s) => s.name === "write" && s.ret === 11).index;
check("write ran vfs_write", ran("write", "vfs_write"));
check("getpid ran __task_pid_nr_ns", ran("getpid", "__task_pid_nr_ns"));
check("mkdirat ran vfs_mkdir", ran("mkdirat", "vfs_mkdir"));
check("sleeping switched to the idle task and back", ran("clock_nanosleep_time64", "__schedule")
      && m.timeline(4000).some((b) => b.comm === "swapper/0") && m.timeline(4000).some((b) => b.comm === "init"));
const at = m.at(m.seek(calls.find((s) => s.name === "getpid").first, 1, "block"));
check("history: a record in getpid", at?.syscall != null && m.syscalls()[at.syscall]?.name === "getpid");
const [vfs] = m.functions("vfs_write", w);
check("vfs_write block counts", vfs?.blocks.some((b) => b.count > 0) && vfs.blocks.some((b) => b.count === 0));
check("fs/read_write.c coverage", m.file_coverage("fs/read_write.c", w).some((l) => l.count > 0));

// LINUX_REBOOT_CMD_POWER_OFF
nb.run("reboot(0xfee1dead, 672274793, 0x4321fedc, 0)");
check("powered off", (await until(() => m.halted() !== undefined && m.halted() + 1)) === 1);

if (failed) console.log(out);
clearInterval(alive);
console.log(failed ? "FAIL" : "PASS");
Deno.exit(failed ? 1 : 0);
