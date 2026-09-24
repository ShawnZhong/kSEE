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

// Make a syscall through /init (the agent on /dev/ksee); resolves to
// { ret, outputs }. See Machine.call for the request.
export async function call(machine, request) {
  while (!machine.call(request)) await new Promise((r) => setTimeout(r, 5));
  for (;;) {
    const r = machine.call_result();
    if (r) return r;
    await new Promise((r) => setTimeout(r, 5));
  }
}
