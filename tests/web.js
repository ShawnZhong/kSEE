// Boot the kernel under Deno with the page's own module (web/ksee.js), run
// programs in it (user/examples/, user/tests/), recorded, and
// check what the recording shows.
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
  const r = await proc.run(path);
  m.record(false);
  return r;
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
  && m.first_call(func, s.first, s.last, s.task.addr) != null);
check("write ran vfs_write", ran("write", "vfs_write"));
check("getpid ran __task_pid_nr_ns", ran("getpid", "__task_pid_nr_ns"));
check("mkdirat ran vfs_mkdir", ran("mkdirat", "vfs_mkdir"));
// (The tasks of a stack chart's columns: see Machine.chart.)
const chartTasks = (flat) => {
  const out = [];
  for (let j = 0; j < flat.length; j += 2 + 3 * flat[j + 1]) out.push(flat[j]);
  return out;
};
const nap = calls.find((s) => s.name === "clock_nanosleep_time64");
check("sleeping switched to the idle task and back", ran("clock_nanosleep_time64", "__schedule")
      && chartTasks(m.chart(nap.first, nap.last, 50)).some((a) => m.task(a, nap.first).comm === "swapper/0"));
const w = calls.find((s) => s.name === "write" && s.ret === 11);
const [vfs] = m.functions("vfs_write", w.index);
check("vfs_write block counts", vfs?.blocks.some((b) => b.count > 0) && vfs.blocks.some((b) => b.count === 0));
check("fs/read_write.c coverage", m.file_coverage("fs/read_write.c", w.index).some((l) => l.count > 0));
const chart = m.chart(w.first, w.last, 200);
check("a stack chart of write's records", chart.length > 600 && chart[0] === w.task.addr);
const steps = m.line_steps(w.first, w.last, w.task.addr);
check("write, line by line (inlined lines too)", steps.length > 50 && steps.some((x) => x.inlined));
const k = steps.findIndex((x) => x.inlined);
let nth = 0;
while (nth < k && steps[k - nth - 1].i === steps[k].i) nth++;
const inl = m.call_stack(steps[k].i, nth);
check(`inlined code is a frame of its own (${steps[k].inlined} in ${steps[k].func})`,
      inl[0]?.inlined && inl[0].func === steps[k].inlined && inl[0].line === steps[k].line && inl.some((f) => f.func === steps[k].func && !f.inlined));
const flat = m.chart(w.first, w.last, 200);
let inlinedRows = 0;
for (let j = 0; j < flat.length; j += 2 + 3 * flat[j + 1]) for (let d = 0; d < flat[j + 1]; d++) if (flat[j + 4 + 3 * d]) inlinedRows++;
check("and a row of its own in the chart", inlinedRows > 0);
const mid = steps.find((x) => x.func === "vfs_write").i;
const stack = m.call_stack(mid);
const kw = stack.find((f) => f.func === "ksys_write"), kwDef = m.functions("ksys_write")[0];
check("an outer call is where it calls the next one in", kw?.file === "fs/read_write.c" && kw.line > kwDef.line);
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

// A program's write, in its recording: the call stack there, and the task.
const hi = await run("/bin/hello");
check("hello ran", hi.status === 0 && await until(() => out.includes("hello\n")));
m.snapshot();
const hw = m.syscalls().find((s) => s.task.pid === hi.pid && s.name === "write");
const inWrite = m.first_call("vfs_write", hw.first, hw.last, hw.task.addr);
const live = m.call_stack(inWrite);
check("the call stack in vfs_write, with arguments", live[0]?.func === "vfs_write" && live.some((f) => f.func === "ksys_write"
      && f.args?.find((a) => a.name === "count")?.value === "6"));
m.memory_at(inWrite);
const task = m.object("task_struct", hw.task.addr, 1);
check("the task, as it was", task?.children?.find((c) => c.name === "comm")?.value === '"hello"');
check("a global", m.global("runqueues", 1)?.children?.some((c) => c.name === "nr_running"));
m.memory_at(null);

// ext4 (on the loop device, mounted by /init): each example's stops are in its recording.
for (const [name, funcs] of [["ext4-write", ["ext4_sync_file", "ext4_mb_new_blocks", "jbd2_journal_commit_transaction", "submit_bio"]],
                             ["ext4-mkdir", ["ext4_mkdir", "__ext4_new_inode", "ext4_init_new_dir", "add_dirent_to_buf"]],
                             ["ext4-delete", ["ext4_delete_entry", "ext4_orphan_add", "ext4_evict_inode", "ext4_free_blocks", "ext4_free_inode"]]]) {
  const e = await run(`/bin/${name}`);
  m.snapshot();
  const p = m.program(e.pid);
  const missing = funcs.filter((f) => m.first_call(f, p.first, m.records() - 1) == null);
  check(`${name}: ${funcs.join(", ")}`, e.status === 0 && !missing.length || console.log("  missing:", missing));
}
// A char * shows its string: mkdir's struct filename, its name.
{
  const p = m.program(m.syscalls().findLast((s) => s.name === "mkdirat").task.pid), i = m.first_call("filename_create", p.first, m.records() - 1);
  const arg = m.call_stack(i).find((f) => f.func === "filename_create")?.args?.find((a) => a.ty === "struct filename *");
  m.memory_at(i); // (as it was: the name is freed after the syscall)
  // (7.2's struct filename: the name pointer is in an anonymous struct, with the name in iname.)
  const name = arg && m.object_by_id("name", arg.target, Number(arg.value.split(" ")[0]), 2)?.children?.[0]?.children?.find((c) => c.name === "name");
  m.memory_at(null);
  check(`a char * with its string: filename->name = ${name?.value}`, name?.value?.endsWith('"/mnt/notes"'));
}

// LINUX_REBOOT_CMD_POWER_OFF
proc.run("/bin/poweroff");
check("powered off", (await until(() => m.halted() !== undefined && m.halted() + 1)) === 1);

if (failed) console.log(out);
clearInterval(alive);
console.log(failed ? "FAIL" : "PASS");
Deno.exit(failed ? 1 : 0);
