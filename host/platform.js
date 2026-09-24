// What the host runtime (host/ksee) needs from the platform.
import { writeSync } from "node:fs";

// Nanoseconds since the machine started (epoch: its performance.timeOrigin),
// identical across Workers.
export const nowNs = (epoch) =>
  BigInt(Math.round((performance.timeOrigin - epoch + performance.now()) * 1e6));

// Imports for an instance of the host module. streams maps output stream
// numbers (see host/ksee/src/wasm.rs) to file descriptors.
export function platformImports(memory, epoch, streams = {}) {
  const text = (p, n) => new TextDecoder().decode(new Uint8Array(memory.buffer).slice(p, p + n));
  return {
    env: { memory },
    platform: {
      now_ns: () => nowNs(epoch),
      emit(stream, p, n) {
        if (streams[stream] !== undefined) writeSync(streams[stream], text(p, n) + "\n");
      },
      panic(p, n) {
        throw new Error(`host panic: ${text(p, n)}`);
      },
    },
  };
}
