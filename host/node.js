#!/usr/bin/env node
// Boot a kSEE wasm kernel under Node.
//   node host/node.js [--mem MiB] [--timeout s] [--initrd cpio] [--trace]
//                     [--dump FILE] vmlinux [cmdline...]
// --dump saves the trace ring when the machine stops; read it with the
// ksee command (crates/ksee-cli).
import { openSync, readFileSync } from "node:fs";
import { Worker } from "node:worker_threads";
import { parseArgs } from "node:util";
import { platformImports } from "./platform.js";

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    mem: { type: "string", default: "64" },
    timeout: { type: "string", default: "0" },
    trace: { type: "boolean", default: false },
    initrd: { type: "string" },
    dump: { type: "string" },
  },
});
const [image, ...cmd] = positionals;
const module = await WebAssembly.compile(readFileSync(image));
const host = await WebAssembly.compile(readFileSync(new URL(
  "../target/wasm32-unknown-unknown/release/ksee_runtime.wasm", import.meta.url)));
const pages = (Number(values.mem) << 20) >> 16;
const memory = new WebAssembly.Memory({ initial: pages, maximum: pages, shared: true });
const epoch = performance.timeOrigin;

// Instantiate both modules once here: their start functions initialize
// their data in the shared memory, so no Worker has to wait for that.
// The host (Rust) owns the memory below the kernel image.
const outputs = {};
const h = new WebAssembly.Instance(host, platformImports(memory, epoch, outputs));
const stub = new Proxy({}, { get: (_, name) => () => { throw new Error(name); } });
const k = new WebAssembly.Instance(module, { env: { memory }, kernel: stub, ksee: stub });
const exports = {
  wasm_irq_pending: k.exports.wasm_irq_pending.value,
  wasm_timer_deadline: k.exports.wasm_timer_deadline.value,
};

// Hand the kernel's self-description (plugin/ksee-trace.cpp) to the host.
function section(name) {
  const parts = WebAssembly.Module.customSections(module, name).map((b) => new Uint8Array(b));
  const len = parts.reduce((n, b) => n + b.length, 0);
  const p = h.exports.alloc(len);
  let off = p;
  for (const b of parts) {
    new Uint8Array(memory.buffer).set(b, off);
    off += b.length;
  }
  return [p, len];
}
{
  const probes = section("ksee.probes"), types = section("ksee.types");
  h.exports.init(...probes, ...types);
  h.exports.free(...probes);
  h.exports.free(...types);
}

// The initramfs goes at the top of memory; the kernel reserves it.
if (values.initrd) {
  const cpio = readFileSync(values.initrd);
  const addr = (memory.buffer.byteLength - cpio.length) & ~0xfff;
  new Uint8Array(memory.buffer).set(cpio, addr);
  cmd.push(`initrd=0x${addr.toString(16)},${cpio.length}`);
}

const shared = { module, host, memory, epoch, exports, cmdline: cmd.join(" ") };
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

// Stop every Worker, then save the trace ring the kernel left behind.
async function halt(code) {
  await Promise.all([...workers].map((w) => w.terminate()));
  if (values.dump) {
    outputs[0] = openSync(values.dump, "w");
    h.exports.dump(k.exports.ksee_trace());
  }
  process.exit(code);
}

if (Number(values.timeout))
  setTimeout(() => { console.error("\n[host] timeout"); halt(124); },
             Number(values.timeout) * 1000);

spawn("timer");
spawn("boot", { comm: "swapper" });
