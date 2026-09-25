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

// Running programs: the kernel runs one (a path in the initramfs), and says
// its pid and exit status when it has exited (see Machine.run). And syscalls
// /init makes for the page: call(name, args) (see Machine.call) gives
// { ret, buffers }.
export function process(machine) {
  const tick = () => new Promise((r) => setTimeout(r, 2));
  return {
    call: async (name, args) => {
      const s = machine.syscall(name);
      if (!s) throw new Error(`no syscall ${name}`);
      while (!machine.call(s.nr, args)) await tick();
      for (;;) {
        const r = machine.call_result();
        if (r) return r;
        await tick();
      }
    },
    run: async (path) => {
      while (!machine.run(path)) await tick();
      for (;;) {
        const r = machine.result();
        if (r) return r;
        await tick();
      }
    },
  };
}
