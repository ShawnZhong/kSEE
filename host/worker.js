// A kernel Worker: runs one task (or the boot CPU), or the timer thread.
// Every kernel Worker instantiates the same module against the one shared
// kernel memory, so wasm globals such as __stack_pointer are per Worker.
import { parentPort, workerData } from "node:worker_threads";
import { writeSync } from "node:fs";

const { module, memory, epoch, cmdline, exports: addr } = workerData;
const u8 = () => new Uint8Array(memory.buffer);
const decoder = new TextDecoder();

// Nanoseconds since the machine started, identical across Workers.
const nowNs = () =>
  BigInt(Math.round((performance.timeOrigin - epoch + performance.now()) * 1e6));

class ExitTask extends Error {}

let kernel;        // this Worker's kernel instance
let user = null;   // the running user program (instance), if any
let loaded = null; // the program exec() has loaded, until user_enter

// Bounds-checked view of the running program's memory, or null.
function userBytes(addr, n) {
  const mem = user?.exports.memory;
  if (!mem || addr + n > mem.buffer.byteLength) return null;
  return new Uint8Array(mem.buffer, addr, n);
}

function imports() {
  return {
    env: { memory },
    kernel: {
      console_write(ptr, len) {
        writeSync(1, decoder.decode(u8().slice(ptr, ptr + len)));
      },
      get_cmdline(buf, len) {
        const bytes = new TextEncoder().encode(cmdline);
        const n = Math.min(bytes.length, len - 1);
        u8().set(bytes.subarray(0, n), buf);
        u8()[buf + n] = 0;
        return n;
      },
      time_ns: nowNs,
      spawn_task(task, sp, name, len) {
        const comm = decoder.decode(u8().slice(name, name + len));
        parentPort.postMessage({ type: "spawn", task, sp, comm });
      },
      exit_task() {
        throw new ExitTask();
      },
      halt(code) {
        parentPort.postMessage({ type: "halt", code });
        // Stop this Worker until the host terminates it.
        Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0);
      },
      print_stack() {
        writeSync(1, new Error().stack.split("\n").slice(2).join("\n") + "\n");
      },
      console_read: () => 0,
      user_load(image, len) {
        try {
          const module = new WebAssembly.Module(u8().slice(image, image + len));
          // The syscall import is the kernel export itself: a direct call.
          loaded = new WebAssembly.Instance(module,
                                            { linux: { syscall: kernel.syscall } });
          return 0;
        } catch (e) {
          writeSync(2, `[${workerData.comm}] exec: ${e.message}\n`);
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
}

function timer() {
  const MAX = 0xffffffffffffffffn;
  const deadline = new BigInt64Array(memory.buffer, addr.wasm_timer_deadline, 1);
  const pending = new Int32Array(memory.buffer, addr.wasm_irq_pending, 1);
  for (;;) {
    const d = BigInt.asUintN(64, Atomics.load(deadline, 0));
    if (d === MAX) {
      Atomics.wait(deadline, 0, BigInt.asIntN(64, MAX));
      continue;
    }
    const left = d - nowNs();
    if (left > 0n) {
      Atomics.wait(deadline, 0, BigInt.asIntN(64, d), Number(left) / 1e6);
      continue;
    }
    if (Atomics.compareExchange(deadline, 0, BigInt.asIntN(64, d),
                                BigInt.asIntN(64, MAX)) === BigInt.asIntN(64, d)) {
      Atomics.or(pending, 0, 1 << 0 /* WASM_IRQ_TIMER */);
      Atomics.notify(pending, 0);
    }
  }
}

if (workerData.role === "timer") {
  timer();
} else {
  const { exports } = new WebAssembly.Instance(module, imports());
  kernel = exports;
  try {
    if (workerData.role === "boot") {
      exports.__stack_pointer.value = exports.boot_stack();
      exports.boot();
    } else {
      exports.__stack_pointer.value = workerData.sp;
      exports.task_entry(workerData.task);
    }
  } catch (e) {
    if (!(e instanceof ExitTask)) {
      writeSync(2, `[${workerData.comm}] ${e.stack}\n`);
      parentPort.postMessage({ type: "halt", code: 1 });
    }
  }
  parentPort.postMessage({ type: "exit" });
}
