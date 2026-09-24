// The page's entry to the runtime (crates/ksee-runtime): create the kernel's
// shared memory, initialize the runtime on it, and boot a kernel.
import init, { Machine } from "../target/web/ksee_runtime.js";

export async function start({ kernel, initrd, cmdline = "", ramMiB = 64 }) {
  const ram = ramMiB << 20;
  // The kernel uses the first `ram` bytes; the runtime's heap grows above.
  const memory = new WebAssembly.Memory({ initial: ram >> 16, maximum: 65536, shared: true });
  await init({ module_or_path: new URL("../target/web/ksee_runtime_bg.wasm", import.meta.url), memory });
  const worker = new URL("./worker.js", import.meta.url).href;
  return Machine.start({ kernel: String(kernel), initrd: initrd && String(initrd), cmdline, worker, ram });
}

// Resolves to the halt code; calls onConsole with console output meanwhile.
export function run(machine, onConsole = () => {}) {
  return new Promise((resolve) => {
    const tick = () => {
      const out = machine.console();
      if (out) onConsole(out);
      const code = machine.halted();
      if (code === undefined) setTimeout(tick, 10);
      else resolve(code);
    };
    tick();
  });
}
