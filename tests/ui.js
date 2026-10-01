// The page in headless Chromium, driven over CDP: serve the site, load it,
// fail on any exception, save a screenshot. --test=NAME runs one of the
// page's own tests (crates/ksee-page/src/uitest.rs: read, back, pipe,
// phone), on the site built with them (target/test/site, see ./ksee.sh
// test), each on the page and screen it needs; --eval runs a script of
// one's own instead (to look at the page as a user would).
//   deno run -A tests/ui.js OUT.png [--test=NAME | --eval=JS] [--url=URL] [--at=#HASH] [--wait=ms] [--size=WxH]
// $CHROME: a Chromium binary (e.g. Playwright's chrome-headless-shell).
const [out] = Deno.args.filter((a) => !a.startsWith("--"));
const opt = (k, d) => Deno.args.find((a) => a.startsWith(`--${k}=`))?.slice(k.length + 3) ?? d;
const test = opt("test", null);
// (Each test's page, and screen: back, the write example with its program changed in the URL.)
const back = 'int fd = openat(AT_FDCWD, "/note.txt", O_WRONLY | O_APPEND); write(fd, "changed;\\n", 9)';
const tests = { read: ["#ex=read"], back: [`#ex=write&p=${encodeURIComponent(back)}`], pipe: ["#ex=pipe"], phone: ["#ex=read", "390x844"] };
if (test && !tests[test]) {
  console.log(`no test ${test}: ${Object.keys(tests).join(", ")}`);
  Deno.exit(2);
}
const [at, size] = test ? tests[test] : [opt("at", ""), null];
const [w, h] = opt("size", size ?? "1500x900").split("x").map(Number);
const port = 9333;
let exceptions = 0;
// Serve the site (the one with the tests, for one) unless given a URL.
let server = null, url = opt("url", "");
if (!url) {
  const site = new URL(test ? "../target/test/site" : "../target/site", import.meta.url).pathname;
  server = new Deno.Command(new URL("../target/release/ksee", import.meta.url).pathname,
    { args: ["serve", site, "--port=8199"], stdout: "null", stderr: "null" }).spawn();
  url = `http://127.0.0.1:8199/${at}`;
  // (Exited: it could not serve, the port taken (another left running?).)
  const gone = await Promise.race([server.status, new Promise((r) => setTimeout(() => r(null), 500))]);
  if (gone) {
    console.log("ksee serve exited: is port 8199 taken?");
    Deno.exit(2);
  }
}
if (!Deno.env.get("CHROME")) {
  console.log("set CHROME to a (headless) Chromium binary");
  Deno.exit(2);
}
const chrome = new Deno.Command(Deno.env.get("CHROME"), {
  args: ["--no-sandbox", "--headless", `--remote-debugging-port=${port}`, `--window-size=${w},${h}`, "about:blank"],
  stdout: "null", stderr: "null",
}).spawn();

let ws;
for (let i = 0; i < 50 && !ws; i++) {
  await new Promise((r) => setTimeout(r, 200));
  try {
    const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
    ws = new WebSocket(list.find((t) => t.type === "page").webSocketDebuggerUrl);
  } catch { /* not up yet */ }
}
await new Promise((r) => (ws.onopen = r));
let id = 0;
const pending = new Map();
ws.onmessage = ({ data }) => {
  const m = JSON.parse(data);
  if (m.id) pending.get(m.id)?.(m);
  else if (m.method === "Runtime.consoleAPICalled")
    console.log(`console.${m.params.type}:`, m.params.args.map((a) => a.value ?? a.description).join(" "));
  else if (m.method === "Runtime.exceptionThrown") {
    exceptions++;
    console.log("exception:", m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text);
  }
};
const send = (method, params = {}) => new Promise((r) => {
  pending.set(++id, r);
  ws.send(JSON.stringify({ id, method, params }));
});
await send("Runtime.enable");
await send("Page.enable");
await send("Page.navigate", { url });
await new Promise((r) => setTimeout(r, Number(opt("wait", "6000"))));
// The script: the page's test (its report, or an exception saying what failed), or --eval's.
const script = test ? `import(new URL("pkg/ksee_page.js", location.href)).then((p) => p.ui_test(${JSON.stringify(test)}))` : opt("eval", null);

if (script) {
  const r = await send("Runtime.evaluate", { expression: script, awaitPromise: true, returnByValue: true });
  const failed = r.result?.exceptionDetails;
  if (failed) exceptions++;
  console.log("eval:", failed ? failed.exception?.description ?? failed.text : JSON.stringify(r.result?.result?.value));
  await new Promise((r) => setTimeout(r, 1000));
}
const shot = await send("Page.captureScreenshot", { format: "png" });
await Deno.writeFile(out, Uint8Array.from(atob(shot.result.data), (c) => c.charCodeAt(0)));
console.log(`saved ${out}`);
for (const p of [chrome, server]) try { p?.kill(); } catch { /* already gone */ }
console.log(exceptions ? `FAIL (${exceptions} exceptions)` : "PASS");
Deno.exit(exceptions ? 1 : 0);
