// Boot the kernel under Deno with the page's own modules (web/ksee.js,
// web/calls.js), make syscalls in /init, use the debugger, and check what
// the trace shows.
//   deno run --allow-read tests/web.js
import { start, process } from "../web/ksee.js";
import { Session, SCENARIOS } from "../web/calls.js";

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
const session = new Session(m, process(m));
// Run a scenario; the last call's result.
async function scenario(name) {
  let r;
  for (const [sys, values] of SCENARIOS[name]) r = await session.call(sys, values);
  return r;
}

check("init ready", await until(() => out.includes("ready for the page's syscalls")));
const write = table.find((s) => s.name === "write");
check("syscall table has signatures",
      write.params.map((p) => `${p.type} ${p.name} ${p.kind}`).join(", ") ===
      "unsigned int fd value, const char * buf in, size_t count value");
check("getpid = 1", (await session.call("getpid")).ret === 1);
const hello = await session.call("write", { fd: "1", buf: "hello kSEE\\n", count: "auto" });
check("write, as strace shows it", hello.line === 'write(1<stdout>, "hello kSEE\\n", 11) = 11' && await until(() => out.includes("hello kSEE")));
const ls = await session.call("openat", { dfd: "AT_FDCWD", filename: "/", flags: "O_RDONLY|O_DIRECTORY" });
const dents = await session.call("getdents64", { fd: "latest", count: "auto" });
check("getdents64 lists /", ls.ret === 3 && dents.outputs[0].list.some((d) => d.startsWith("init")));
await session.call("close", { fd: "latest" });
check("write a file", (await scenario("write a file")).ret === 0);
const back = await scenario("read it back");
check("read it back", back.ret === 0 && session.fds.length === 0);
check("writev two struct iovec", (await scenario("writev")).ret === 18 && await until(() => out.includes("from writev")));
const pipe = await scenario("pipe");
check("pipe: fds by name", pipe.line === 'read(3<pipe read end>, "through a pipe", 32) = 14');
const uname = await scenario("uname");
check("uname decodes struct new_utsname", uname.outputs[0].object.children.some((c) => c.name === "machine" && c.value === '"wasm"'));
check("clock_nanosleep_time64", (await scenario("sleep 20 ms")).ret === 0);
let bad;
try { await session.call("nosuch"); } catch (e) { bad = e.message; }
check("unknown syscalls are errors", bad === "no syscall nosuch");
await session.reset();
check("reset forgets the fds", session.fds.length === 0);

// A tour's program: a real child process of init, with its source lines.
const proc = process(m);
const pid = await proc.spawn("/bin/read");
check("spawn a program", pid > 1);
check("it exits with status 0", (await proc.wait(pid)) === 0 && await until(() => out.includes("read 26 bytes")));
m.snapshot();
const prog = m.program(pid);
check("its syscalls are its own", m.syscalls().some((s) => s.task.pid === pid && s.name === "read" && s.ret === 26));
check("its source lines are in the trace", prog?.lines.some((l) => l.line === 23));

// Stop in vfs_write, look around, step, go on.
check("breakpoint set", m.break_function("vfs_write", true));
const pending = session.call("write", { fd: "1", buf: "hi\\n", count: "auto" });
const stop = await until(() => m.stopped());
check("stopped in vfs_write", stop?.func === "vfs_write" && stop.file === "fs/read_write.c");
check("stopped task is init", stop?.task.comm === "init" && stop.task.pid === 1);
const frames = m.call_stack();
check("the call stack, with arguments", frames[0]?.func === "vfs_write" && frames.some((f) => f.func === "ksys_write"
      && f.args.find((a) => a.name === "count")?.value === "3") && frames[0].args[0].target != null);
const task = stop && m.object("task_struct", stop.task.addr, 1);
check("decode the task while stopped", task?.children?.find((c) => c.name === "comm")?.value === '"init"');
check("decode a global", m.global("runqueues", 1)?.children?.some((c) => c.name === "nr_running"));
m.step_task();
check("step to the next block on the task", !!await until(() => {
  const s = m.stopped();
  return s && s.stop !== stop.stop && s.task.addr === stop.task.addr && s;
}));
m.break_function("vfs_write", false);
m.resume();
check("resumed", (await pending).ret === 3);

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
const fr = m.frames(calls.find((s) => s.name === "write" && s.ret === 11).first, calls.find((s) => s.name === "write" && s.ret === 11).last, calls[0].task.addr, 1);
check("write's calls, rebuilt", fr.some((f) => f.func === "vfs_write" && fr.some((g) => g.func === "ksys_write" && g.depth === f.depth - 1)));
check("fs/read_write.c coverage", m.file_coverage("fs/read_write.c", w).some((l) => l.count > 0));

// LINUX_REBOOT_CMD_POWER_OFF
session.call("reboot", { magic1: "0xfee1dead", magic2: "672274793", cmd: "0x4321fedc", arg: "NULL" }).catch(() => {});
check("powered off", (await until(() => m.halted() !== undefined && m.halted() + 1)) === 1);

if (failed) console.log(out);
clearInterval(alive);
console.log(failed ? "FAIL" : "PASS");
Deno.exit(failed ? 1 : 0);
