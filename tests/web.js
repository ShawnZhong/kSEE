// Boot the kernel in the web host's runtime under Deno (the same module the
// page uses), make syscalls through /init (the agent), use the debugger,
// check the trace views, and save the trace dump.
//   deno run --allow-read --allow-write tests/web.js DUMP
import { start, call } from "../web/ksee.js";

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
async function until(pred, ms = 10000) {
  for (const t0 = Date.now(); Date.now() - t0 < ms; await sleep(10)) {
    out += m.console().replaceAll("\r", "");
    const v = pred();
    if (v) return v;
  }
}
const enc = (s) => [...new TextEncoder().encode(s)];
const table = m.syscall_table();
const nr = (name) => table.find((s) => s.name === name).nr;

check("selftest passed", await until(() => out.includes("selftest: passed")));
check("agent ready", await until(() => out.includes("syscall agent ready")));
const write = table.find((s) => s.name === "write");
check("syscall table has signatures",
      write.params.map((p) => `${p.type} ${p.name} ${p.kind}`).join(", ") ===
      "unsigned int fd value, const char * buf in, size_t count value");
check("getpid = 1", (await call(m, { nr: nr("getpid") })).ret === 1);
const hello = enc("hello kSEE\n");
check("write", (await call(m, { nr: nr("write"), args: [1, 0, hello.length], buffers: [{ arg: 1, data: hello }] })).ret === 11
      && await until(() => out.includes("hello kSEE")));
const AT_FDCWD = -100;
check("mkdirat", (await call(m, { nr: nr("mkdirat"), args: [AT_FDCWD, 0, 0o755], buffers: [{ arg: 1, data: enc("/tmp\0") }] })).ret === 0);
const fd = (await call(m, { nr: nr("openat"), args: [AT_FDCWD, 0, 0o200000, 0], buffers: [{ arg: 1, data: enc("/\0") }] })).ret;
const dents = await call(m, { nr: nr("getdents64"), args: [fd, 0, 1024], buffers: [{ arg: 1, len: 1024 }] });
const names = new TextDecoder().decode(dents.outputs[0].slice(0, Math.max(0, dents.ret)));
check("getdents64 lists /", dents.ret > 0 && names.includes("init") && names.includes("tmp"));
check("clock_nanosleep_time64", (await call(m, { nr: nr("clock_nanosleep_time64"), args: [1, 0, 0, 0],
      buffers: [{ arg: 2, data: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0x2d, 0x31, 1, 0, 0, 0, 0] }] })).ret === 0); // 20 ms

// Stop in vfs_write, look around, step, go on.
check("breakpoint set", m.break_function("vfs_write", true));
const pending = call(m, { nr: nr("write"), args: [1, 0, 3], buffers: [{ arg: 1, data: enc("hi\n") }] });
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
check("resumed", (await pending).ret === 3);

m.snapshot();
const calls = m.syscalls().filter((s) => !s.control);
const called = calls.map((s) => s.name);
for (const n of ["getpid", "write", "mkdirat", "openat", "getdents64", "clock_nanosleep_time64"])
  check(`syscall ${n} traced`, called.includes(n));
check("agent traffic is marked", m.syscalls().some((s) => s.control && s.name === "read"));
const w = calls.find((s) => s.name === "write" && s.ret === 11).index;
check("write ran vfs_write", m.runs(w).some((r) => r.func === "vfs_write"));
const [vfs] = m.functions("vfs_write", w);
check("vfs_write CFG", vfs?.blocks.some((b) => b.count > 0 && b.succs.some((e) => e.count > 0)));

// LINUX_REBOOT_CMD_POWER_OFF
call(m, { nr: nr("reboot"), args: [0xfee1dead, 672274793, 0x4321fedc, 0] });
check("powered off", (await until(() => m.halted() !== undefined && m.halted() + 1)) === 1);

if (failed) console.log(out);
await Deno.writeFile(Deno.args[0], m.dump());
clearInterval(alive);
console.log(failed ? "FAIL" : "PASS");
Deno.exit(failed ? 1 : 0);
