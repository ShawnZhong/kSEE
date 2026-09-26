// The page in headless Chromium (./ksee.sh test --ui): serve the site (target/site), load it,
// go through examples, fail on any exception (or if the script throws), and
// save a screenshot. Options look at another page, or run another script
// (--eval; --back: going back, see below).
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
  // (By default the read example: see the script below.)
  url = `http://127.0.0.1:8199/${opt("at", Deno.args.some((a) => a.startsWith("--eval") || a === "--back") ? ""
    : Deno.args.includes("--pipe") ? "#ex=pipe" : "#ex=read")}`;
  await new Promise((r) => setTimeout(r, 500));
}
// --back: the page opened with a program changed (examples.js, the URL's
// p=): the life of a file, its first call, then another instead of its
// second; the tutorial at its second stop.
const back = 'openat(AT_FDCWD, "/mnt/note.txt", O_CREAT | O_WRONLY, 0644); write($1, "changed;\\n", 9)';
if (Deno.args.includes("--back")) url += `#ex=file&p=${encodeURIComponent(back)}&stop=2`;
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
// By default: the read example (#ex=read): its calls are made by
// themselves; go through its tutorial's stops (the note, the Stack, a hover
// at each), play a little, hover and click the chart, add a call; report.
// (A program's line: its call, in its editor, then its result.)
const lineOf = `const line = (li) => (li.querySelector("textarea")?.value ?? "") + " " + li.querySelector(".line").innerText.replace(/\\s+/g, " ").trim();`;
const backScript = `(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  ${lineOf}
  for (let i = 0; i < 100 && document.querySelectorAll("ol.steps > li.done").length < 2; i++) await sleep(100);
  const done = [...document.querySelectorAll("ol.steps > li.done")].map(line);
  const stop = document.querySelector(".tour .tnav span")?.textContent;
  if (done.length !== 2 || !done[1].includes("= 9") || !stop?.startsWith("Stop 2 of") || !location.hash.includes("stop=2"))
    throw new Error("not back: " + JSON.stringify([done, stop, location.hash]));
  return { done, stop, url: location.hash };
})()`;
// --pipe: the pipe example (two processes), all its calls made: the ones
// that blocked finished as later ones woke them; the child's are its.
const pipeScript = `(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  ${lineOf}
  const lis = () => [...document.querySelectorAll("ol.steps > li:not(.add)")];
  for (let i = 0; i < 100 && (document.querySelectorAll("ol.steps > li.done").length < 10 || !document.querySelector(".tour li.on")); i++) await sleep(100);
  const r = { calls: lis().map(line), stop: document.querySelector(".tour").innerText.replace(/\\s+/g, " ") };
  const bad = (k, want) => !r.calls[k]?.includes(want);
  if (document.querySelector("ol.steps > li.blocked") || bad(1, "= 19") || bad(2, "tid ") || bad(2, "new program")
      || bad(5, "= 6") || bad(8, "= 0") || bad(9, "= 0") || !r.stop.includes("Stop 1 of"))
    throw new Error("the pipe: " + JSON.stringify(r));
  return r;
})()`;
const script = Deno.args.includes("--back") ? backScript : Deno.args.includes("--pipe") ? pipeScript : opt("eval", `(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const $ = (id) => document.getElementById(id);
  const until = async (f, n = 100) => { for (let i = 0; i < n && !f(); i++) await sleep(100); };
  ${lineOf}
  const lis = () => [...document.querySelectorAll("ol.steps > li:not(.add)")];
  const r = { badge: $("badge").textContent };
  // (All its calls made, the tutorial at its first stop.)
  await until(() => document.querySelectorAll("ol.steps > li.done").length === lis().length && document.querySelector(".tour li.on"));
  const pick = document.querySelector("#ex-head select");
  r.first = [pick.selectedOptions[0]?.textContent, line(lis()[0])];
  r.examples = pick.options.length;
  r.calls = lis().map(line).slice(4, 6);
  r.notes = [];
  for (let k = 0; k < 5; k++) {
    await sleep(300);
    r.notes.push(document.querySelector(".tour li.on b")?.textContent);
    if (k === 0) r.stack = [...document.querySelectorAll("#stack tr.frame")].slice(0, 2).map((f) => f.innerText.trim());
    if (k === 0) r.args = [...document.querySelectorAll("#stack tr")].slice(1, 4).map((f) => f.innerText.replace(/\\s+/g, " ").trim());
    if (r.notes.at(-1) === "vfs_read") r.hover = await ksee.src.hoverAt("file->f_mode & FMODE_READ", "f_mode");
    const next = document.querySelector(".tour button.go");
    if (!next || next.disabled) break;
    next.click();
  }
  $("speed").value = "30"; $("play").click(); await sleep(1500); $("play").click();
  r.played = [$("badge").textContent, $("where").textContent.slice(0, 50)];
  // Stepping: into a call, over the next, out of it (the Stack's calls, deep).
  const depthNow = () => document.querySelectorAll("#stack tr.frame").length;
  const press = (kind) => { $("step-by").value = kind; $("step-fwd").click(); };
  const d0 = depthNow();
  for (let k = 0; k < 200 && depthNow() <= d0; k++) press("line");
  const d1 = depthNow();
  press("over");
  const d2 = depthNow();
  press("out");
  const d3 = depthNow();
  r.stepped = [d0, d1, d2, d3];
  if (!(d1 > d0 && d2 <= d1 && d3 <= d2 && d3 < d1)) throw new Error("stepping: " + JSON.stringify(r.stepped));
  r.chart = $("chart-title").textContent;
  // (The read's own box: its syscall entry's.)
  const read = ksee.m.syscalls().findLast((x) => x.name === "read" && x.task.pid === 1 && x.last != null);
  const box = ksee.time.boxOf(read.first), at = { clientX: box?.x, clientY: box?.y };
  $("chart").dispatchEvent(new PointerEvent("pointermove", at));
  r.tip = document.querySelector(".tip:not([hidden])")?.innerText.replace(/\\s+/g, " ").slice(0, 60);
  // (Above the stack, at the top of the chart: the call on top then.)
  $("chart").dispatchEvent(new PointerEvent("pointermove", { clientX: box.x, clientY: $("chart").getBoundingClientRect().top + 2 }));
  r.top = document.querySelector(".tip:not([hidden])")?.firstChild?.textContent;
  if (!r.top) throw new Error("no call on top above the stack");
  $("chart").dispatchEvent(new PointerEvent("pointermove", at));
  await sleep(400);
  r.preview = [$("fn-title").textContent.slice(0, 40), document.querySelector(".tip .here")?.textContent, $("stack").classList.contains("preview")];
  // (Moving on in the same call: the tip keeps where it is.)
  $("chart").dispatchEvent(new PointerEvent("pointermove", { ...at, clientX: at.clientX + 1 }));
  if (!document.querySelector(".tip .here")?.textContent) throw new Error("the tip lost where the call is");
  $("chart").dispatchEvent(new PointerEvent("pointerleave"));
  await sleep(300);
  r.unpreviewed = !$("fn-title").classList.contains("preview") && !$("stack").classList.contains("preview");
  if (!r.preview[2] || !r.unpreviewed) throw new Error("the Stack's preview: " + JSON.stringify([r.preview, r.unpreviewed]));
  for (const type of ["pointerdown", "pointerup"])
    $("chart").dispatchEvent(new PointerEvent(type, at));
  r.clicked = [$("where").textContent.slice(0, 50), document.querySelector("#stack tr.frame.on")?.innerText.trim().slice(0, 50)];
  // (A syscall's box: the syscall selected, entry to exit.)
  const home = ksee.time.home();
  if (home?.a !== read.first || home?.b !== read.last) throw new Error("the read's selection: " + JSON.stringify(home) + ", not " + read.first + ".." + read.last);
  r.selected = $("chart-title").textContent;
  // (Any call's box, double-clicked: the call, from its first record to its return.)
  const ksys = ksee.m.call_stack(ksee.m.first_call("ksys_read", read.first, read.last, read.task.addr)).find((f) => f.func === "ksys_read");
  const kb = ksee.time.boxOf(ksys.first);
  $("chart").dispatchEvent(new MouseEvent("dblclick", { clientX: kb.x, clientY: kb.y }));
  const kh = ksee.time.home();
  if (kh?.a !== ksys.first || kh?.b !== ksee.m.call_end(ksys.first)) throw new Error("ksys_read's selection: " + JSON.stringify(kh) + ", not from " + ksys.first);
  r.dblclicked = $("chart-title").textContent;
  // Your own call: type it, Tab completes the name, Enter makes it.
  const input = document.querySelector(".callform textarea"), key = (k) => input.dispatchEvent(new KeyboardEvent("keydown", { key: k }));
  input.value = "mkdira"; key("Tab");
  input.value += "AT_FDCWD, "; input.dispatchEvent(new Event("input"));
  r.typed = [input.value, document.querySelector(".callform .hint").textContent];
  input.value = 'openat(AT_FDCWD, "/mnt/mine", O_CREAT | O_WRONLY, 0644)'; key("Enter");
  await until(() => document.querySelectorAll("ol.steps > li.done").length >= 8);
  r.mine = line(lis().at(-1));
  // Another example, from the selector: on a fresh kernel, the page staying.
  window.stayed = true;
  const sel = document.querySelector("#ex-head select");
  sel.selectedIndex = [...sel.options].findIndex((o) => o.textContent.includes("Directories"));
  sel.dispatchEvent(new Event("change"));
  await until(() => document.querySelectorAll("ol.steps > li.done").length === lis().length && document.querySelector(".tour li.on"));
  r.switched = [location.hash, window.stayed, line(lis()[0])];
  if (!location.hash.startsWith("#ex=dirs") || !window.stayed || !r.switched[2]?.startsWith("mkdirat")) throw new Error("not switched: " + JSON.stringify(r.switched));
  // Typing a call: constants that fit the parameter first, Tab takes one; the parameter in bold.
  const inp = document.querySelector(".callform textarea");
  inp.focus();
  await sleep(100);
  // (Nothing written: the syscalls the examples make, to start from.)
  r.offered = [...document.querySelectorAll(".callform ul.complete li")].map((li) => li.innerText.split(" ")[0]).slice(0, 4);
  if (!r.offered.includes("openat")) throw new Error("nothing offered: " + JSON.stringify(r.offered));
  inp.value = 'openat(AT_FDCWD, "/x", O_CR'; inp.dispatchEvent(new Event("input"));
  r.complete = [...document.querySelectorAll(".callform ul.complete li")].map((li) => li.innerText).slice(0, 3);
  inp.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab" }));
  r.completed = [inp.value, document.querySelector(".callform .hint b")?.textContent];
  if (!r.completed[0].endsWith("O_CREAT") || r.completed[1] !== "int flags") throw new Error("completion: " + JSON.stringify(r));
  inp.value = ""; inp.dispatchEvent(new Event("input")); inp.blur();
  // Changing a call, where it is (typing in it): the program runs again, on a fresh kernel, with it.
  const n = lis().length, e3 = lis()[3].querySelector("textarea");
  e3.focus();
  e3.value = 'mkdirat(AT_FDCWD, "/mnt/other", 0700)';
  e3.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
  await until(() => location.hash.includes("&p=") && document.querySelectorAll("ol.steps > li.done").length === n && line(lis()[3]).includes("= 0"));
  r.edited = [lis().length, line(lis()[3])];
  if (r.edited[0] !== n || !r.edited[1].includes("/mnt/other")) throw new Error("editing: " + JSON.stringify(r));
  // The console, folded to its last line; the current task, open to its key fields.
  r.console = [$("pane-console").classList.contains("folded"), $("console").textContent];
  r.current = [...document.querySelectorAll("#stack tr.task, #stack tr.task ~ tr")].slice(0, 3).map((tr) => tr.innerText.replace(/\\s+/g, " ").trim());
  if (!r.console[0] || !r.console[1] || r.console[1].includes("\\n") || !r.current[0]?.startsWith("current task: init(1)") || !r.current[1]?.startsWith("pid")) throw new Error("console, current: " + JSON.stringify(r));
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
