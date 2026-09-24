// Every Worker's script: initialize the runtime (crates/ksee-runtime) on the
// kernel's shared memory, then run what the page asked for.
import init, { worker_entry } from "./pkg/ksee_runtime.js";

self.onmessage = async ({ data }) => {
  await init({ module_or_path: data.runtime, memory: data.memory });
  worker_entry(data);
};
