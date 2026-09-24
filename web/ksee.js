// kSEE web host: boots the wasm kernel in the browser (or Deno).
//
// This one module is both the page's API (start) and the script of every
// Worker. Each kernel Worker instantiates the kernel and the host runtime
// (crates/ksee-runtime) against the one shared memory; the kernel's
// frequent imports are runtime exports (direct wasm calls), and only what
// needs the platform is JS here: Workers, instantiating modules, and user
// programs' memories.

const url = import.meta.url;
const decoder = new TextDecoder();

// Wall-clock ms, the same in every Worker (the runtime's only clock).
const nowMs = () => performance.timeOrigin + performance.now();

function runtimeImports(memory, outputs = {}) {
  return {
    env: { memory },
    platform: {
      now_ms: nowMs,
      write(output, p, n) {
        outputs[output]?.(new Uint8Array(memory.buffer).slice(p, p + n));
      },
      panic(p, n) {
        throw new Error("ksee runtime: " + decoder.decode(new Uint8Array(memory.buffer).slice(p, p + n)));
      },
    },
  };
}

// ---- The page -------------------------------------------------------------

/**
 * Boot a kernel. Returns { runtime, kernel, halted } where halted resolves
 * to the halt code, and runtime/kernel are the page's instances (their
 * exports read the machine: see view() and console()).
 */
export async function start({ kernel: kernelUrl, runtime: runtimeUrl, initrd,
                              cmdline = "", memoryMiB = 64, onConsole = () => {} }) {
  const [kmod, rmod] = await Promise.all(
    [kernelUrl, runtimeUrl].map((u) => WebAssembly.compileStreaming(fetch(u))));
  const pages = (memoryMiB << 20) >> 16;
  const memory = new WebAssembly.Memory({ initial: pages, maximum: pages, shared: true });

  // Instantiating once here runs both modules' start functions, which
  // initialize their data in the shared memory; the runtime owns the memory
  // below the kernel image.
  const outputs = {};
  const rt = new WebAssembly.Instance(rmod, runtimeImports(memory, outputs)).exports;
  const stub = new Proxy({}, { get: (_, n) => () => { throw new Error(`${n} on the page`); } });
  const k = new WebAssembly.Instance(kmod, { env: { memory }, kernel: stub, ksee: stub }).exports;

  const copyIn = (bytes) => {
    const p = rt.alloc(bytes.length);
    new Uint8Array(memory.buffer).set(bytes, p);
    return [p, bytes.length];
  };
  const section = (name) => {
    const parts = WebAssembly.Module.customSections(kmod, name).map((b) => new Uint8Array(b));
    const all = new Uint8Array(parts.reduce((n, b) => n + b.length, 0));
    parts.reduce((off, b) => (all.set(b, off), off + b.length), 0);
    return copyIn(all);
  };
  const probes = section("ksee.probes"), types = section("ksee.types");
  rt.init(...probes, ...types);
  rt.free(...probes);
  rt.free(...types);

  // The initramfs goes at the top of memory; the kernel reserves it.
  if (initrd) {
    const cpio = new Uint8Array(await (await fetch(initrd)).arrayBuffer());
    const addr = (memory.buffer.byteLength - cpio.length) & ~0xfff;
    new Uint8Array(memory.buffer).set(cpio, addr);
    cmdline += ` initrd=0x${addr.toString(16)},${cpio.length}`;
  }
  const cmd = copyIn(new TextEncoder().encode(cmdline.trim()));
  rt.set_cmdline(...cmd);

  const addrs = {
    deadline: k.wasm_timer_deadline.value,
    pending: k.wasm_irq_pending.value,
  };
  const workers = new Set();
  const spawn = (msg) => {
    const w = new Worker(url, { type: "module" });
    w.onmessage = ({ data }) => data.spawn && spawn(data.spawn);
    w.onerror = (e) => console.error("ksee worker:", e.message ?? e);
    w.postMessage({ kmod, rmod, memory, addrs, ...msg });
    workers.add(w);
  };
  spawn({ role: "timer" });
  spawn({ role: "boot" });

  // Drain the console, and stop every Worker once the kernel halts.
  const buf = rt.alloc(4096);
  const halted = new Promise((resolve) => {
    const tick = () => {
      let n;
      while ((n = rt.console_take(buf, 4096)))
        onConsole(decoder.decode(new Uint8Array(memory.buffer).slice(buf, buf + n)));
      const h = rt.halted();
      if (!h) return setTimeout(tick, 10);
      for (const w of workers) w.terminate();
      resolve(h - 1);
    };
    tick();
  });

  // A text view from the runtime (see view_* in crates/ksee-runtime).
  const view = (name, ...args) => {
    let text = "";
    outputs[1] = (bytes) => (text += decoder.decode(bytes));
    rt[`view_${name}`](k.ksee_trace(), ...args);
    delete outputs[1];
    return text;
  };
  const cfg = (fn, syscall = 0xffffffff) => {
    const name = copyIn(new TextEncoder().encode(fn));
    const text = view("cfg", ...name, syscall);
    rt.free(...name);
    return text;
  };
  return { memory, runtime: rt, kernel: k, halted, view, cfg };
}

// ---- A Worker ---------------------------------------------------------------

class ExitTask extends Error {}

function worker({ kmod, rmod, memory, addrs, role, task, sp }) {
  const rt = new WebAssembly.Instance(rmod, runtimeImports(memory)).exports;
  const stack = rt.stack();
  if (!stack) throw new Error("ksee: out of runtime stacks");
  rt.__stack_pointer.value = stack;
  if (role === "timer") return rt.timer(addrs.deadline, addrs.pending);

  const u8 = () => new Uint8Array(memory.buffer);
  let kernel, user = null, loaded = null;
  const userBytes = (p, n) => {
    const m = user?.exports.memory;
    return m && p + n <= m.buffer.byteLength ? new Uint8Array(m.buffer, p, n) : null;
  };
  const imports = {
    env: { memory },
    ksee: { now: rt.time_ns },
    kernel: {
      // Frequent: the runtime's exports.
      console_write: rt.console_write,
      console_read: rt.console_read,
      get_cmdline: rt.get_cmdline,
      time_ns: rt.time_ns,
      halt: rt.halt,
      // Rare: the platform.
      spawn_task: (task, sp) => self.postMessage({ spawn: { role: "task", task, sp } }),
      exit_task: () => { throw new ExitTask(); },
      print_stack: () => console.log(new Error().stack),
      user_load(image, len) {
        try {
          const m = new WebAssembly.Module(u8().slice(image, image + len));
          // The user program's syscall import is the kernel's export.
          loaded = new WebAssembly.Instance(m, { linux: { syscall: kernel.syscall } });
          return 0;
        } catch (e) {
          console.error("exec:", e.message);
          return 1;
        }
      },
      user_enter() {
        user = loaded;
        loaded = null;
        user.exports._start();
      },
      // Both return the number of bytes not copied.
      user_read(to, from, n) {
        const src = userBytes(from, n);
        if (!src) return n;
        u8().set(src, to);
        return 0;
      },
      user_write(to, from, n) {
        const dst = userBytes(to, n);
        if (!dst) return n;
        dst.set(u8().subarray(from, from + n));
        return 0;
      },
    },
  };
  kernel = new WebAssembly.Instance(kmod, imports).exports;
  try {
    if (role === "boot") {
      kernel.__stack_pointer.value = kernel.boot_stack();
      kernel.boot();
    } else {
      kernel.__stack_pointer.value = sp;
      kernel.task_entry(task);
    }
  } catch (e) {
    // A dead task's Worker, or the halt (whose runtime call traps).
    if (!(e instanceof ExitTask) && !rt.halted()) throw e;
  }
  rt.unstack(stack);
  self.close();
}

if (typeof WorkerGlobalScope !== "undefined" && self instanceof WorkerGlobalScope)
  self.onmessage = ({ data }) => worker(data);
