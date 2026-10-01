// The kernel's tests (crates/ksee-page/src/selftest.rs: the page's module
// built with them, in target/test/): boot the kernel under Deno as the page
// does, and run them; each check logged, "ok: …" or "FAIL: …".
//   deno run --allow-read tests/web.js   (after ./ksee.sh test has built target/test/)
import { start } from "../target/test/ksee.js";
import { self_test } from "../target/test/pkg/ksee_page.js";

const root = new URL("..", import.meta.url);
const m = await start({
  kernel: new URL("target/site/vmlinux.gz", root),
  initrd: new URL("target/site/initramfs.cpio.gz", root),
});
const examples = JSON.parse(await Deno.readTextFile(new URL("target/site/examples.json", root)));
const ok = await self_test(m, examples);
console.log(ok ? "PASS" : "FAIL");
Deno.exit(ok ? 0 : 1);
