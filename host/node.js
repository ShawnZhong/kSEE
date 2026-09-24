#!/usr/bin/env node
// Boot a kSEE wasm kernel under Node.
//   node host/node.js [--mem MiB] [--timeout s] [--initrd cpio]
//                     [--events out.jsonl] [--trace] vmlinux [cmdline...]
import { openSync, readFileSync } from "node:fs";
import { Worker } from "node:worker_threads";
import { parseArgs } from "node:util";

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    mem: { type: "string", default: "64" },
    timeout: { type: "string", default: "0" },
    trace: { type: "boolean", default: false },
    initrd: { type: "string" },
    events: { type: "string" },
  },
});
const [image, ...cmd] = positionals;
const module = await WebAssembly.compile(readFileSync(image));
const pages = (Number(values.mem) << 20) >> 16;
const memory = new WebAssembly.Memory({ initial: pages, maximum: pages, shared: true });

// Instantiate once on the main thread: this runs the module's start
// function, which initializes kernel memory, so no Worker has to wait.
const stub = new Proxy({}, { get: (_, name) => () => { throw new Error(name); } });
const main = new WebAssembly.Instance(module, { env: { memory }, kernel: stub, ksee: stub });
const exports = {
  wasm_irq_pending: main.exports.wasm_irq_pending.value,
  wasm_timer_deadline: main.exports.wasm_timer_deadline.value,
  __current_task: main.exports.__current_task.value,
};

// The initramfs goes at the top of memory; the kernel reserves it.
if (values.initrd) {
  const cpio = readFileSync(values.initrd);
  const addr = (memory.buffer.byteLength - cpio.length) & ~0xfff;
  new Uint8Array(memory.buffer).set(cpio, addr);
  cmd.push(`initrd=0x${addr.toString(16)},${cpio.length}`);
}

const core = await WebAssembly.compile(readFileSync(
  new URL("./ksee/target/wasm32-unknown-unknown/release/ksee.wasm", import.meta.url)));

const shared = {
  module, core, memory, exports,
  epoch: performance.timeOrigin,
  cmdline: cmd.join(" "),
  // Workers append kSEE events here; only the Worker holding the CPU token
  // runs kernel code, so writes are already in execution order.
  eventsFd: values.events ? openSync(values.events, "w") : -1,
};
const workers = new Set();

function spawn(role, extra = {}) {
  const w = new Worker(new URL("./worker.js", import.meta.url), {
    workerData: { ...shared, role, ...extra },
  });
  workers.add(w);
  const name = extra.comm ?? role;
  if (values.trace) console.error(`[host] spawn ${name}`);
  w.on("message", (m) => {
    if (m.type === "spawn") spawn("task", m);
    else if (m.type === "halt") halt(m.code);
  });
  w.on("exit", () => {
    workers.delete(w);
    if (values.trace) console.error(`[host] exit ${name}`);
  });
  w.on("error", (e) => { console.error(e); halt(1); });
  return w;
}

function halt(code) {
  for (const w of workers) w.terminate();
  process.exit(code);
}

if (Number(values.timeout))
  setTimeout(() => { console.error("\n[host] timeout"); halt(124); },
             Number(values.timeout) * 1000);

spawn("timer");
spawn("boot", { comm: "swapper" });
