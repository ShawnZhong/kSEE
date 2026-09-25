// The page in headless Chromium (./ksee.sh test --ui): serve web/, load it, run a tour and go through it, fail on any exception, and save a screenshot. Options
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
// By default: open the read tour (it runs, recorded); go through its stops
// (the note, the Stack, a hover at each); play a little; report.
const script = opt("eval", `(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const $ = (id) => document.getElementById(id);
  const r = { badge: $("badge").textContent };
  for (let i = 0; i < 50 && !document.querySelector(".row.tour"); i++) await sleep(100);
  r.tours = document.querySelectorAll(".row.tour").length;
  [...document.querySelectorAll(".row.tour")].find((e) => e.textContent.includes("read()")).click();
  for (let i = 0; i < 100 && $("note").hidden; i++) await sleep(100);
  r.after = [$("badge").textContent, document.querySelector(".tstatus").textContent.slice(0, 30)];
  r.notes = [];
  for (let k = 0; k < 5; k++) {
    await sleep(300);
    r.notes.push($("note").querySelector("b")?.textContent);
    if (k === 0) r.stack = [...document.querySelectorAll("#stack tr.frame")].slice(0, 2).map((f) => f.innerText.trim());
    if (k === 0) r.args = [...document.querySelectorAll("#stack tr")].slice(1, 4).map((f) => f.innerText.replace(/\\s+/g, " ").trim());
    if (r.notes.at(-1) === "vfs_read") r.hover = await ksee.src.hoverAt("file->f_mode & FMODE_READ", "f_mode");
    const next = [...$("note").querySelectorAll("button")].find((b) => b.textContent.startsWith("Next"));
    if (!next) break;
    next.click();
  }
  $("speed").value = "30"; $("play").click(); await sleep(1500); $("play").click();
  r.played = [$("badge").textContent, $("mem-note").textContent, $("where").textContent.slice(0, 50)];
  r.chart = $("chart-title").textContent;
  const box = $("chart").getBoundingClientRect();
  $("chart").dispatchEvent(new PointerEvent("pointermove", { clientX: box.left + box.width / 2, clientY: box.bottom - 12 - 2 - 4 }));
  r.tip = document.querySelector(".tip:not([hidden])")?.innerText.replace(/\\s+/g, " ").slice(0, 60);
  ksee.tours.pickLine(23); await sleep(300);
  r.picked = $("where").textContent.slice(0, 40);
  return r;
})()`);
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
