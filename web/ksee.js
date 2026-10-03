// The page's entry to the host (crates/ksee-host): initialize it, and
// boot a kernel (on this thread: its tasks are JSPI stacks, see sched.rs).
import init, { Machine } from "./pkg/ksee_page.js";

// The module, loaded (once: start's too).
export const load = () => init({ module_or_path: new URL("./pkg/ksee_page_bg.wasm", import.meta.url) });

export async function start({ kernel, initrd }) {
  if (!WebAssembly.Suspending) throw new Error("this browser has no JSPI (WebAssembly.Suspending): kSEE needs Chrome 137, Firefox 153, Safari 27 or later");
  if (!kernel || !initrd) throw new Error("kSEE needs a kernel and initramfs");
  await load();
  [kernel, initrd] = await Promise.all([kernel, initrd].map((u) => bytes(String(u))));
  return Machine.start(kernel, initrd);
}

// Fetch an asset and return its bytes, decompressing the build's .gz files.
async function bytes(url) {
  let response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: ${response.status}`);
  if (url.endsWith(".gz")) {
    response = new Response(response.body.pipeThrough(new DecompressionStream("gzip")));
  }
  return new Uint8Array(await response.arrayBuffer());
}
