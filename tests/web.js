// Boot the kernel under Deno with the page's own module (web/ksee.js), run
// programs in it (user/tests/, user/tours/) while recording, use the
// debugger, and check what the trace shows.
//   deno run --allow-read tests/web.js
import { start, process } from "../web/ksee.js";

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
const proc = process(m);
// Run program `path`, recording; its pid and exit status.
async function run(path) {
  m.record(true);
  const pid = await proc.spawn(path);
  const status = await proc.wait(pid);
  m.record(false);
  return { pid, status };
}

check("init ready", await until(() => out.includes("ready for the page's programs")));
check("not recording at first", !m.recording() && (m.snapshot(), m.records() === 0));

// Syscalls, traced.
const t = await run("/bin/syscalls");
check("a program runs, and exits 0", t.pid > 1 && t.status === 0);
check("its output", await until(() => out.includes("hello kSEE") && out.includes("ls /: init found") &&
      out.includes("hello from writev") && out.includes("pipe: through a pipe") && out.includes("uname: Linux wasm") && out.includes("slept")));
m.snapshot();
const calls = m.syscalls().filter((s) => s.task.pid === t.pid);
const called = calls.map((s) => s.name);
for (const n of ["getpid", "write", "mkdirat", "openat", "getdents64", "writev", "pipe2", "read", "uname", "clock_nanosleep_time64"])
  check(`syscall ${n} traced`, called.includes(n));
const ran = (name, func) => calls.some((s) => s.name === name && s.last != null
  && m.frames(s.first, s.last, s.task.addr, 1).some((f) => f.func === func));
check("write ran vfs_write", ran("write", "vfs_write"));
check("getpid ran __task_pid_nr_ns", ran("getpid", "__task_pid_nr_ns"));
check("mkdirat ran vfs_mkdir", ran("mkdirat", "vfs_mkdir"));
check("sleeping switched to the idle task and back", ran("clock_nanosleep_time64", "__schedule")
      && m.timeline(4000).some((b) => b.comm === "swapper/0"));
const w = calls.find((s) => s.name === "write" && s.ret === 11);
const [vfs] = m.functions("vfs_write", w.index);
check("vfs_write block counts", vfs?.blocks.some((b) => b.count > 0) && vfs.blocks.some((b) => b.count === 0));
check("fs/read_write.c coverage", m.file_coverage("fs/read_write.c", w.index).some((l) => l.count > 0));
const frames = m.frames(w.first, w.last, w.task.addr, 1);
check("write's calls, rebuilt", frames.some((f) => f.func === "vfs_write" && frames.some((g) => g.func === "ksys_write" && g.depth === f.depth - 1)));
const steps = m.line_steps(w.first, w.last, w.task.addr);
check("write, line by line (more lines than blocks)", steps.length > w.blocks && steps.some((x) => x.inlined));
const mid = steps.find((x) => x.func === "vfs_write").i;
const stack = m.call_stack(mid);
check("a past call stack, with arguments", stack[0]?.func === "vfs_write"
      && stack.find((f) => f.func === "ksys_write")?.args?.find((a) => a.name === "count")?.value === "11");
check("memory as it was: writes undone", m.memory_at(w.first) > m.memory_at(w.last));
m.memory_at(null);
const at = m.at(m.seek(calls.find((s) => s.name === "getpid").first, 1, "block"));
check("history: a record in getpid", at?.syscall != null && m.syscalls()[at.syscall]?.name === "getpid");

// A tour's program: its source lines, and f_pos over time.
const r = await run("/bin/read");
check("the read tour's program", r.status === 0 && await until(() => out.includes("read 26 bytes")));
m.snapshot();
const prog = m.program(r.pid);
check("its source lines are in the trace", prog?.lines.some((l) => l.line === 23));
const read = m.syscalls().find((s) => s.task.pid === r.pid && s.name === "read");
const inRead = m.line_steps(read.first, read.last, read.task.addr).find((x) => x.func === "vfs_read").i;
const file = m.call_stack(inRead)[0].args.find((a) => a.name === "file");
const fpos = (i) => (m.memory_at(i), m.object_by_id("file", file.target, Number(file.value), 1)?.children?.find((c) => c.name === "f_pos"));
check("f_pos before the read, and after", fpos(read.first)?.value === "0" && fpos(read.last)?.value === "26");
const who = (m.memory_at(read.last), m.last_write(fpos(read.last).addr, 8));
check("who wrote f_pos: ksys_read", who?.func === "ksys_read");
const readSteps = m.line_steps(read.first, read.last, read.task.addr);
const pos = (i) => (m.memory_at(i), m.call_stack(i).find((c) => c.func === "ksys_read")?.locals?.find((l) => l.name === "pos")?.value);
check("a local on the stack, as it was: ksys_read's pos, 0 then 26",
      pos(inRead) === "0" && pos(readSteps.findLast((x) => x.func === "ksys_read").i) === "26");
m.memory_at(null);

// Stop in vfs_write (in hi's write), look around, step, go on.
check("breakpoint set", m.break_function("vfs_write", true));
m.record(true);
const hi = await proc.spawn("/bin/hi");
const waiting = proc.wait(hi);
const stop = await until(() => m.stopped());
check("stopped in vfs_write", stop?.func === "vfs_write" && stop.file === "fs/read_write.c");
check("stopped task is hi", stop?.task.comm === "hi" && stop.task.pid === hi);
m.snapshot();
const live = m.call_stack();
check("the call stack, with arguments", live[0]?.func === "vfs_write" && live.some((f) => f.func === "ksys_write"
      && f.args?.find((a) => a.name === "count")?.value === "3"));
const task = stop && m.object("task_struct", stop.task.addr, 1);
check("decode the task while stopped", task?.children?.find((c) => c.name === "comm")?.value === '"hi"');
check("decode a global", m.global("runqueues", 1)?.children?.some((c) => c.name === "nr_running"));
m.step_task();
check("step to the next block on the task", !!await until(() => {
  const s = m.stopped();
  return s && s.stop !== stop.stop && s.task.addr === stop.task.addr && s;
}));
m.break_function("vfs_write", false);
m.resume();
check("resumed", (await waiting) === 0 && await until(() => out.includes("hi\n")));
m.record(false);

// LINUX_REBOOT_CMD_POWER_OFF
proc.spawn("/bin/poweroff");
check("powered off", (await until(() => m.halted() !== undefined && m.halted() + 1)) === 1);

if (failed) console.log(out);
clearInterval(alive);
console.log(failed ? "FAIL" : "PASS");
Deno.exit(failed ? 1 : 0);
