// The page in headless Chromium (./ksee.sh test --ui): serve web/, load it, run the scenarios,
// step into a syscall and play it, fail on any exception, and save a screenshot. Options
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
    { args: ["serve", new URL("../web", import.meta.url).pathname, "--port=8199"], stdout: "null", stderr: "null" }).spawn();
  url = "http://127.0.0.1:8199/";
  await new Promise((r) => setTimeout(r, 500));
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
// By default: run the scenarios; step into a write, play some lines while
// paused, continue; replay the write; report what the page shows.
const script = opt("eval", `(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const $ = (id) => document.getElementById(id);
  const idle = async () => { for (let i = 0; i < 200 && document.querySelector(".card.running"); i++) await sleep(50); };
  const r = {};
  for (const o of [...$("scenarios").options].slice(1)) {
    $("scenarios").value = o.value;
    $("scenarios").dispatchEvent(new Event("change"));
    await sleep(100);
    await idle();
  }
  r.cards = document.querySelectorAll(".card.ok").length;
  r.failed = [...document.querySelectorAll(".card.err")].map((c) => c.innerText);
  // Step into a new write.
  $("add").value = "write";
  $("add").dispatchEvent(new Event("change"));
  const card = [...document.querySelectorAll(".card")].at(-1);
  card.querySelector('.acts button[title^="Step into"]').click();
  for (let i = 0; i < 100 && ksee.state() !== "paused"; i++) await sleep(50);
  r.paused = $("badge").textContent + " " + $("status").textContent;
  r.stack = [...document.querySelectorAll("#stack .frame")].map((f) => f.innerText).slice(0, 3);
  for (let k = 0; k < 3; k++) { $("fwd-line").click(); await sleep(400); }
  r.stepped = $("status").textContent;
  $("cont").click();
  await idle();
  r.card = card.innerText;
  // Replay it: the write is selected, the cursor at its start.
  r.replay = $("badge").textContent;
  $("play").click();
  await sleep(1500);
  $("play").click();
  r.where = $("where").textContent;
  r.flame = !$("flamebox").hidden;
  return r;
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
