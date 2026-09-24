// Boot the web host under Deno (the same web/ksee.js the page uses), check
// the console and the trace views, and save the trace dump.
//   deno run --allow-read --allow-write tests/web.js DUMP [cmdline]
import { start } from "../web/ksee.js";

const root = new URL("..", import.meta.url);
let out = "";
const m = await start({
  kernel: new URL("linux/vmlinux", root),
  runtime: new URL("target/wasm32-unknown-unknown/release/ksee_runtime.wasm", root),
  initrd: new URL("build/initramfs.cpio", root),
  cmdline: Deno.args.slice(1).join(" ") || "wasm_selftest",
  onConsole: (s) => (out += s),
});
const timeout = setTimeout(() => { console.log(out); console.log("FAIL (timeout)"); Deno.exit(1); }, 30000);
const code = await m.halted;
clearTimeout(timeout);

let failed = false;
const check = (what, ok) => {
  console.log(`${ok ? "ok" : "FAIL"}: ${what}`);
  failed ||= !ok;
};
const syscalls = m.view("syscalls");
check("halted cleanly", code === 0);
check("selftest passed", out.includes("selftest: passed"));
check("init ran", out.includes("Hello from wasm userspace! I am pid 1"));
check("write returned 36", /init\(1\) syscall 64\(0x1, 0x[0-9a-f]+, 0x24\) = 36/.test(syscalls));
check("write ran vfs_write", /^ +vfs_write +fs\/read_write\.c:\d+:\d+$/m.test(m.view("syscall", 0)));
check("vfs_write CFG", /^vfs_write fs\/read_write\.c:\d+ +\(\d+ blocks, \d+ ran\)/.test(m.cfg("vfs_write", 0)));
if (failed) console.log(out + "\n" + syscalls);
await Deno.writeFile(Deno.args[0], new Uint8Array(await m.dump().arrayBuffer()));
console.log(failed ? "FAIL" : "PASS");
Deno.exit(failed ? 1 : 0);
