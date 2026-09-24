// The page in headless Chromium: serve build/web, load it, make some calls
// with the presets, fail on any exception, and save a screenshot. Options
// look at another page or script (to check the UI while working on it).
//   deno run -A tests/ui.js OUT.png [--url=URL] [--wait=ms] [--eval=JS] [--size=WxH]
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
    { args: ["serve", new URL("../build/web", import.meta.url).pathname, "--port=8199"], stdout: "null", stderr: "null" }).spawn();
  url = "http://127.0.0.1:8199/";
  await new Promise((r) => setTimeout(r, 500));
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
await new Promise((r) => setTimeout(r, Number(opt("wait", "8000"))));
// By default: run the presets, then report what the page shows.
const script = opt("eval", `(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  for (const b of document.querySelectorAll("#presets button")) { b.click(); await sleep(800); }
  await sleep(500);
  return { syscalls: document.querySelectorAll("#syscalls .row").length, log: document.getElementById("log").innerText.split("\\n").length };
})()`);
if (script) {
  const r = await send("Runtime.evaluate", { expression: script, awaitPromise: true, returnByValue: true });
  console.log("eval:", JSON.stringify(r.result?.result?.value ?? r.result?.exceptionDetails?.text));
  await new Promise((r) => setTimeout(r, 1000));
}
const shot = await send("Page.captureScreenshot", { format: "png" });
await Deno.writeFile(out, Uint8Array.from(atob(shot.result.data), (c) => c.charCodeAt(0)));
console.log(`saved ${out}`);
chrome.kill();
server?.kill();
console.log(exceptions ? `FAIL (${exceptions} exceptions)` : "PASS");
Deno.exit(exceptions ? 1 : 0);
