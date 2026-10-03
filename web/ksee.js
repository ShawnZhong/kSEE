// The page's entry to the runtime (crates/ksee-runtime): initialize it, and
// boot a kernel (on this thread: its tasks are JSPI stacks, see sched.rs).
import init, { Machine } from "./pkg/ksee_page.js";

// The module, loaded (once: start's too).
export const load = () => init({ module_or_path: new URL("./pkg/ksee_page_bg.wasm", import.meta.url) });

export async function start({ kernel, initrd }) {
  if (!WebAssembly.Suspending) throw new Error("this browser has no JSPI (WebAssembly.Suspending): kSEE needs Chrome 137, Firefox 153, Safari 27 or later");
  if (!kernel || !initrd) throw new Error("kSEE needs a kernel and initramfs");
  await load();
  [kernel, initrd] = await Promise.all([kernel, initrd].map((u) => unpacked(String(u))));
  return Machine.start(kernel, initrd);
}

// A .gz file (the build gzips the kernel and the initramfs), unpacked, as a
// URL for the runtime to fetch; any other URL as it is.
async function unpacked(url) {
  if (!url.endsWith(".gz")) return url;
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  return URL.createObjectURL(await new Response(r.body.pipeThrough(new DecompressionStream("gzip"))).blob());
}
