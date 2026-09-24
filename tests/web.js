// Boot the kernel in the web host's runtime under Deno (the same module the
// page uses), drive /init's syscall shell and the debugger, check the
// trace views, and save the trace dump.
//   deno run --allow-read --allow-write tests/web.js DUMP
import { start } from "../web/ksee.js";

const root = new URL("..", import.meta.url);
// wasm-bindgen's futures (with threads) wake through Atomics.waitAsync,
// which Deno does not count as pending work: keep the process alive.
const alive = setInterval(() => {}, 1000);
const m = await start({
  kernel: new URL("build/vmlinux", root),
  initrd: new URL("build/initramfs.cpio", root),
  cmdline: "wasm_selftest",
});

let out = "", failed = false;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const check = (what, ok) => {
  console.log(`${ok ? "ok" : "FAIL"}: ${what}`);
  failed ||= !ok;
  return ok;
};
// Wait for pred() (polling the console); undefined on timeout.
async function until(pred, ms = 10000) {
  for (const t0 = Date.now(); Date.now() - t0 < ms; await sleep(10)) {
    out += m.console().replaceAll("\r", ""); // the tty's ONLCR
    const v = pred();
    if (v) return v;
  }
}
// Type a shell command; its output up to the next prompt.
async function shell(line) {
  const mark = out.length;
  m.type_in(line + "\n");
  await until(() => out.slice(mark + line.length).includes("ksee$ "));
  return out.slice(mark);
}

check("selftest passed", await until(() => out.includes("selftest: passed")));
check("shell prompt", await until(() => out.includes("ksee$ ")));
check("getpid = 1", (await shell("getpid")).includes("= 1\n"));
check("write", /hello kSEE\n= 11\n/.test(await shell("write 1 hello kSEE")));
check("mkdir", (await shell("mkdir /tmp")).includes("= 0\n"));
const ls = await shell("ls /");
check("ls", /\binit\b/.test(ls) && ls.includes("tmp/"));
check("sleep", (await shell("sleep 20")).includes("= 0\n"));

// Stop in vfs_write, look around, step, go on.
check("breakpoint set", m.break_function("vfs_write", true));
const mark = out.length;
m.type_in("write 1 hi\n");
const stop = await until(() => m.stopped());
check("stopped in vfs_write", stop?.func === "vfs_write" && stop.file === "fs/read_write.c");
check("stopped task is init", stop?.task.comm === "init" && stop.task.pid === 1);
const task = stop && m.object("task_struct", stop.task.addr, 1);
check("decode the task while stopped", task?.children?.find((c) => c.name === "comm")?.value === '"init"');
m.step();
const step = await until(() => {
  const s = m.stopped();
  return s && (s.func !== stop.func || s.block !== stop.block) && s;
});
check("step to the next block", !!step);
m.break_function("vfs_write", false);
m.resume();
check("resumed", await until(() => out.slice(mark).includes("hi\n= 3\n")));

m.type_in("poweroff\n");
const code = await until(() => m.halted() !== undefined && m.halted() + 1);
check("powered off", code === 1);

m.snapshot();
const names = m.syscalls().map((s) => s.name);
for (const n of ["getpid", "write", "mkdirat", "getdents64", "clock_nanosleep", "read"])
  check(`syscall ${n} traced`, names.includes(n));
const w = m.syscalls().findIndex((s) => s.name === "write" && s.ret === 11);
check("write ran vfs_write", m.runs(w).some((r) => r.func === "vfs_write"));
const [vfs] = m.functions("vfs_write", w);
check("vfs_write CFG", vfs?.blocks.some((b) => b.count > 0 && b.succs.some((e) => e.count > 0)));

if (failed) console.log(out);
await Deno.writeFile(Deno.args[0], m.dump());
clearInterval(alive);
console.log(failed ? "FAIL" : "PASS");
Deno.exit(failed ? 1 : 0);
