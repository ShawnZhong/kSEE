// The page's entry to the runtime (crates/ksee-runtime): create the kernel's
// shared memory, initialize the runtime on it, and boot a kernel.
import init, { Machine } from "./pkg/ksee_runtime.js";

export async function start({ kernel, initrd, cmdline = "", ramMiB = 64 }) {
  const ram = ramMiB << 20;
  // The kernel uses the first `ram` bytes; the runtime's heap grows above.
  const memory = new WebAssembly.Memory({ initial: ram >> 16, maximum: 65536, shared: true });
  await init({ module_or_path: new URL("./pkg/ksee_runtime_bg.wasm", import.meta.url), memory });
  const worker = new URL("./worker.js", import.meta.url).href;
  [kernel, initrd] = await Promise.all([kernel, initrd].map((u) => u && unpacked(String(u))));
  return Machine.start({ kernel, initrd, cmdline, worker, ram });
}

// A .gz file (the build gzips the kernel and the initramfs), unpacked, as a
// URL for the runtime to fetch; any other URL as it is.
async function unpacked(url) {
  if (!url.endsWith(".gz")) return url;
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  return URL.createObjectURL(await new Response(r.body.pipeThrough(new DecompressionStream("gzip"))).blob());
}

// Syscalls /init makes for the page: call(name, args) (see Machine.call)
// gives { ret, buffers }.
// cancel(): calls under way give up (the kernel starts again).
export function process(machine) {
  const tick = () => new Promise((r) => setTimeout(r, 2));
  let gen = 0;
  return {
    call: async (name, args) => {
      const s = machine.syscall(name), mine = gen;
      if (!s) throw new Error(`no syscall ${name}`);
      while (!machine.call(s.nr, args)) {
        await tick();
        if (gen !== mine) throw new Error("the kernel started again");
      }
      for (;;) {
        if (gen !== mine) throw new Error("the kernel started again");
        const r = machine.call_result();
        if (r) return r;
        await tick();
      }
    },
    cancel: () => gen++,
  };
}
