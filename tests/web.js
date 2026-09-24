// Boot the kernel in the web host's runtime under Deno (the same module the
// page uses), check the console and the trace views, and save the dump.
//   deno run --allow-read --allow-write tests/web.js DUMP [cmdline]
import { start, run } from "../web/ksee.js";

const root = new URL("..", import.meta.url);
// wasm-bindgen's futures (with threads) wake through Atomics.waitAsync,
// which Deno does not count as pending work: keep the process alive.
const alive = setInterval(() => {}, 1000);
const m = await start({
  kernel: new URL("build/vmlinux", root),
  initrd: new URL("build/initramfs.cpio", root),
  cmdline: Deno.args.slice(1).join(" ") || "wasm_selftest",
});
let out = "";
const timeout = setTimeout(() => { console.log(out); console.log("FAIL (timeout)"); Deno.exit(1); }, 30000);
const code = await run(m, (s) => (out += s));
clearTimeout(timeout);
clearInterval(alive);

let failed = false;
const check = (what, ok) => {
  console.log(`${ok ? "ok" : "FAIL"}: ${what}`);
  failed ||= !ok;
};
const syscalls = m.view_syscalls();
check("halted cleanly", code === 0);
check("selftest passed", out.includes("selftest: passed"));
check("init ran", out.includes("Hello from wasm userspace! I am pid 1"));
check("write returned 36", /init\(1\) syscall 64\(0x1, 0x[0-9a-f]+, 0x24\) = 36/.test(syscalls));
check("write ran vfs_write", /^ +vfs_write +fs\/read_write\.c:\d+:\d+$/m.test(m.view_syscall(0)));
const [vfs] = m.functions("vfs_write", 0);
check("vfs_write CFG", vfs?.file === "fs/read_write.c" && vfs.blocks.some((b) => b.count > 0 && b.succs.some((e) => e.count > 0)));
const sys = m.syscalls();
check("syscalls are named", sys[0]?.name === "write" && sys[1]?.name === "getpid");
check("runs start at the entry", m.runs(0)[0]?.func === "wasm_syscall");
const init = m.task_address(1);
const task = init === undefined ? null : m.object("task_struct", init, 1);
const field = (name) => task?.children?.find((c) => c.name === name)?.value;
check("init's task_struct decodes", field("pid") === "1" && field("comm") === '"init"');
if (failed) console.log(out + "\n" + syscalls + "\n" + JSON.stringify(task)?.slice(0, 500));
await Deno.writeFile(Deno.args[0], m.dump());
console.log(failed ? "FAIL" : "PASS");
Deno.exit(failed ? 1 : 0);
