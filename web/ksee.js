// The page's entry to the runtime (crates/ksee-runtime): create the kernel's
// shared memory, initialize the runtime on it, and boot a kernel.
import init, { Machine } from "./pkg/ksee_runtime.js";

export async function start({ kernel, initrd, cmdline = "", ramMiB = 64 }) {
  const ram = ramMiB << 20;
  // The kernel uses the first `ram` bytes; the runtime's heap grows above.
  const memory = new WebAssembly.Memory({ initial: ram >> 16, maximum: 65536, shared: true });
  await init({ module_or_path: new URL("./pkg/ksee_runtime_bg.wasm", import.meta.url), memory });
  const worker = new URL("./worker.js", import.meta.url).href;
  return Machine.start({ kernel: String(kernel), initrd: initrd && String(initrd), cmdline, worker, ram });
}

// init, which starts the page's programs, on the kernel's /dev/ksee
// channel (see user/init.c). Each operation waits for init's reply.
export function process(machine) {
  const request = async (op, a = 0, b = 0, args = [], data) => {
    while (!machine.post(op, a >>> 0, b >>> 0, args, data)) await tick();
    for (;;) {
      const r = machine.take();
      if (r) return r;
      await tick();
    }
  };
  const tick = () => new Promise((r) => setTimeout(r, 2));
  return {
    // A program (a path in the initramfs) as a child of init: its pid; then
    // its exit status, once it exits.
    spawn: async (path) => (await request(1, 0, 0, [], new TextEncoder().encode(path + "\0"))).ret,
    wait: async (pid) => (await request(2, pid)).ret,
  };
}
