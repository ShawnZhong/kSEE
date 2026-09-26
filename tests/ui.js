// The page in headless Chromium (./ksee.sh test --ui): serve the site (target/site), load it,
// go through examples, fail on any exception (or if the script throws), and
// save a screenshot. Options look at another page, or run another script
// (--eval; --back, --pipe: tests/ui/back.js, pipe.js; by default read.js).
//   deno run -A tests/ui.js OUT.png [--url=URL] [--at=PATH] [--wait=ms] [--eval=JS | --back | --pipe] [--size=WxH]
// $CHROME: a Chromium binary (e.g. Playwright's chrome-headless-shell).
const [out] = Deno.args.filter((a) => !a.startsWith("--"));
const opt = (k, d) => Deno.args.find((a) => a.startsWith(`--${k}=`))?.slice(k.length + 3) ?? d;
const [w, h] = opt("size", "1500x900").split("x").map(Number);
const port = 9333;
let exceptions = 0;
// Serve the site unless given a URL.
let server = null, url = opt("url", "");
if (!url) {
  server = new Deno.Command(new URL("../target/release/ksee", import.meta.url).pathname,
    { args: ["serve", new URL("../target/site", import.meta.url).pathname, "--port=8199"], stdout: "null", stderr: "null" }).spawn();
  // (By default the read example: see tests/ui/read.js.)
  url = `http://127.0.0.1:8199/${opt("at", Deno.args.some((a) => a.startsWith("--eval") || a === "--back") ? ""
    : Deno.args.includes("--pipe") ? "#ex=pipe" : "#ex=read")}`;
  await new Promise((r) => setTimeout(r, 500));
}
// --back: the page opened with a program changed (examples.js, the URL's
// p=): the write example, its first call, then another instead of its
// second; the tutorial at its second stop.
const back = 'openat(AT_FDCWD, "/note.txt", O_WRONLY | O_APPEND); write($1, "changed;\\n", 9)';
if (Deno.args.includes("--back")) url += `#ex=write&p=${encodeURIComponent(back)}&stop=2`;
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
// The script to run in the page: tests/ui/NAME.js (read, the default;
// back; pipe), after tests/ui/lib.js; or --eval's.
const page = (name) => Deno.readTextFileSync(new URL(`ui/${name}.js`, import.meta.url));
const script = opt("eval", null) ?? page("lib") + page(Deno.args.includes("--back") ? "back" : Deno.args.includes("--pipe") ? "pipe" : "read");

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
