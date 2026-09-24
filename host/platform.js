// What the host runtime (host/ksee) needs from the platform.
import { writeSync } from "node:fs";

// Nanoseconds since the machine started (epoch: its performance.timeOrigin),
// identical across Workers.
export const nowNs = (epoch) =>
  BigInt(Math.round((performance.timeOrigin - epoch + performance.now()) * 1e6));

// Imports for an instance of the host runtime (crates/ksee-runtime).
// outputs maps its output numbers to file descriptors.
export function platformImports(memory, epoch, outputs = {}) {
  const text = (p, n) => new TextDecoder().decode(new Uint8Array(memory.buffer).slice(p, p + n));
  return {
    env: { memory },
    platform: {
      now_ns: () => nowNs(epoch),
      write(output, p, n) {
        if (outputs[output] !== undefined)
          writeSync(outputs[output], new Uint8Array(memory.buffer).slice(p, p + n));
      },
      panic(p, n) {
        throw new Error(`host panic: ${text(p, n)}`);
      },
    },
  };
}
