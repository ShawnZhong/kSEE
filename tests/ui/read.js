// The default: the read example (#ex=read): its calls made by themselves; its explanation's links (the
// Call stack, a hover at each), stepping, play, the chart (hover, click), typing and changing calls,
// another example; returns a report.
(async () => {
    const r = {};
  // (All its calls made, the cursor at the explanation's first link.)
  await until(() => document.querySelectorAll("ol.steps > li.done").length === lis().length && document.querySelector(".explain a.place.on"));
  const pick = document.querySelector("#ex-head select");
  r.first = [pick.selectedOptions[0]?.textContent, line(lis()[0])];
  r.examples = pick.options.length;
  r.calls = lis().map(line);
  // The explanation's links, each gone to: the function's call, the Call stack there.
  r.notes = [];
  const places = [...document.querySelectorAll(".explain a.place")];
  for (const [k, a] of places.entries()) {
    a.click();
    await sleep(300);
    r.notes.push([a.textContent, $("where").textContent.slice(0, 60)]);
    if (k === 1) r.stack = [...document.querySelectorAll("#stack tr.frame")].slice(0, 2).map((f) => f.innerText.trim());
    if (k === 1) r.args = [...document.querySelectorAll("#stack tr")].slice(1, 4).map((f) => f.innerText.replace(/\s+/g, " ").trim());
    if (a.textContent === "vfs_read") r.hover = await ksee.src.hoverAt("file->f_mode & FMODE_READ", "f_mode");
  }
  if (r.notes.length < 3 || !r.notes.some(([n, w]) => n === "vfs_read" && w.includes("vfs_read")) || !r.hover) throw new Error("links: " + JSON.stringify(r.notes));
  // Watch a value: click one in the Call stack (one in memory: a local's, a task's field); the Watch shows it,
  // its writes listed (a local's in its frame's lifetime), ▶ goes to the next.
  const v = document.querySelector("#stack td.fval.mem");
  v?.click();
  r.history = [v?.textContent, document.querySelectorAll("#watch li").length, $("watch").innerText.slice(0, 120)];
  if (!/writes (while \w+'s frame lives|in the recording)/.test(r.history[2] ?? "")) throw new Error("no watch: " + JSON.stringify(r.history));
  document.querySelector("#watch .whead button:not([disabled])")?.click();
  await sleep(200);
  document.querySelector("#watch .rm")?.click();
  if ($("watch").children.length) throw new Error("the watch stays");
  $("speed").value = "30"; $("play").click(); await sleep(1500); $("play").click();
  r.played = $("where").textContent.slice(0, 50);
  // Run to a line (the source's F4): the line where the cursor is, its next run.
  const here = $("where").textContent;
  const [, file, ln] = here.match(/ (\S+):(\d+)/) ?? [];
  const was = ksee.time.cursor();
  if (file) ksee.runTo(file, Number(ln), 1);
  r.ranTo = [here, $("where").textContent, was, ksee.time.cursor()];
  if (!file || !(r.ranTo[3] > was) || r.ranTo[1] !== here) throw new Error("run to: " + JSON.stringify(r.ranTo));
  // Stepping: into a call, over the next, out of it (the Call stack's calls, deep), from the
  // explanation's first link (a known place: ksys_read's entry), so that the run is the same every time.
  document.querySelector(".explain a.place").click();
  await sleep(300);
  const depthNow = () => document.querySelectorAll("#stack tr.frame").length;
  const press = (kind) => { $("step-by").value = kind; $("step-fwd").click(); };
  const frames = () => [...document.querySelectorAll("#stack tr.frame")].map((f) => f.innerText.trim().split("\n")[0].slice(0, 28));
  const d0 = depthNow(), trail = [[$("where").textContent, frames()]];
  for (let k = 0; k < 200 && depthNow() <= d0; k++) press("line");
  const d1 = depthNow();
  trail.push([$("where").textContent, frames()]);
  press("over");
  const d2 = depthNow();
  trail.push([$("where").textContent, frames()]);
  press("out");
  const d3 = depthNow();
  trail.push([$("where").textContent, frames()]);
  r.stepped = [d0, d1, d2, d3];
  if (!(d1 > d0 && d2 <= d1 && d3 <= d2 && d3 < d1)) throw new Error("stepping: " + JSON.stringify(r.stepped) + " " + JSON.stringify(trail));
  r.chart = $("chart-title").textContent;
  // (The read's own box: its syscall entry's.)
  const read = ksee.m.syscalls().findLast((x) => x.name === "read" && x.task.pid === 1 && x.last != null);
  ksee.select(read); // (its box in the window: the tutorial may have ended in another call)
  const box = ksee.time.boxOf(read.first), at = { clientX: box?.x, clientY: box?.y };
  $("chart").dispatchEvent(new PointerEvent("pointermove", at));
  r.tip = document.querySelector(".tip:not([hidden])")?.innerText.replace(/\s+/g, " ").slice(0, 60);
  // (Above the stack, at the top of the chart: the call on top then.)
  $("chart").dispatchEvent(new PointerEvent("pointermove", { clientX: box.x, clientY: $("chart").getBoundingClientRect().top + 2 }));
  r.top = document.querySelector(".tip:not([hidden])")?.firstChild?.textContent;
  if (!r.top) throw new Error("no call on top above the stack");
  $("chart").dispatchEvent(new PointerEvent("pointermove", at));
  await sleep(400);
  r.here = document.querySelector(".tip .here")?.textContent;
  // (Moving on in the same call: the tip keeps where it is; the source does not follow a hover.)
  const title = $("fn-title").textContent;
  $("chart").dispatchEvent(new PointerEvent("pointermove", { ...at, clientX: at.clientX + 1 }));
  if (!document.querySelector(".tip .here")?.textContent) throw new Error("the tip lost where the call is");
  $("chart").dispatchEvent(new PointerEvent("pointerleave"));
  await sleep(300);
  if ($("fn-title").textContent !== title || !r.here?.includes("now at")) throw new Error("hover: " + JSON.stringify([r.here, title, $("fn-title").textContent]));
  for (const type of ["pointerdown", "pointerup"])
    $("chart").dispatchEvent(new PointerEvent(type, at));
  r.clicked = [$("where").textContent.slice(0, 50), document.querySelector("#stack tr.frame.on")?.innerText.trim().slice(0, 50)];
  // (A syscall's box: the syscall selected, entry to exit.)
  const home = ksee.time.home();
  if (home?.a !== read.first || home?.b !== read.last) throw new Error("the read's selection: " + JSON.stringify(home) + ", not " + read.first + ".." + read.last);
  // The Changes tab: what the read changed, by object (the struct file's f_pos among them); an object watched.
  document.querySelector('h2.tabs a[data-tab="changes"]').click();
  r.changes = [$("changes-note").textContent, [...document.querySelectorAll("#changes .whead a")].map((a) => a.textContent).slice(0, 6)];
  const fileChange = [...document.querySelectorAll("#changes .changed")].find((d) => d.innerText.includes("f_pos: 0 → "));
  if (!fileChange) throw new Error("the read's changes: " + JSON.stringify(r.changes));
  fileChange.querySelector(".whead a").click();
  document.querySelector('h2.tabs a[data-tab="stack"]').click();
  if (!$("watch").innerText.includes("f_pos")) throw new Error("watching the file: " + $("watch").innerText.slice(0, 100));
  // (The file said as what it is: its fd and name.)
  if (!/fd 3 note\.txt/.test($("watch").innerText)) throw new Error("the file's name: " + $("watch").innerText.slice(0, 100));
  // The Records tab: the raw trace around the cursor, the cursor's record marked.
  document.querySelector('h2.tabs a[data-tab="records"]').click();
  r.records = [$("records-note").textContent, document.querySelector("#records tr.on")?.innerText];
  if (!document.querySelector("#records tr.on") || !document.querySelector("#records tr.store")) throw new Error("the records: " + JSON.stringify(r.records));
  document.querySelector('h2.tabs a[data-tab="stack"]').click();
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
  const had = lis().length;
  input.value = 'openat(AT_FDCWD, "/mnt/mine", O_CREAT | O_WRONLY, 0644)'; key("Enter");
  await until(() => document.querySelectorAll("ol.steps > li.done").length > had);
  r.mine = line(lis().at(-1));
  // Another example, from the selector: on a fresh kernel, the page staying.
  window.stayed = true;
  const sel = document.querySelector("#ex-head select");
  sel.selectedIndex = [...sel.options].findIndex((o) => o.textContent.includes("directory"));
  sel.dispatchEvent(new Event("change"));
  await until(() => document.querySelectorAll("ol.steps > li.done").length === lis().length && document.querySelector(".tour li.on"));
  r.switched = [location.hash, window.stayed, line(lis()[0])];
  if (!location.hash.startsWith("#ex=dir&") && location.hash !== "#ex=dir" || !window.stayed || !r.switched[2]?.startsWith("mkdirat")) throw new Error("not switched: " + JSON.stringify(r.switched));
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
  // Removing a call (✕): the program runs again without it; ↺: the example's own program back.
  lis()[3].querySelector(".rm").click();
  await until(() => document.querySelectorAll("ol.steps > li.done").length === n - 1 && !location.hash.includes("/mnt/other"));
  r.removed = lis().length;
  document.querySelector(".ptitle a").click();
  await until(() => !location.hash.includes("&p=") && document.querySelectorAll("ol.steps > li.done").length === n);
  r.reset = [lis().length, line(lis()[3])];
  if (r.removed !== n - 1 || r.reset[0] !== n || r.reset[1].includes("/mnt/other")) throw new Error("remove, reset: " + JSON.stringify(r));
  // Code navigation: the lens over the shown function (who called it), F12 to a function, and to a field.
  ksee.src.showFunction("vfs_mkdir");
  await sleep(1200);
  r.lens = [...document.querySelectorAll(".cm-editor .lens")].map((e) => e.innerText.replace(/\s+/g, " ")).slice(0, 2);
  // (F12 on a name: ksee.src.definitionAt(line, column), as the key does.)
  const goto = async (needle, off) => {
    const at = ksee.src.find(needle);
    if (!at) throw new Error(`no "${needle}" in ${$("fn-title").textContent}`);
    ksee.src.definitionAt(at.line, at.column + off);
    await sleep(1000);
  };
  await goto("may_create_dentry(idmap", 1);
  r.def = $("fn-title").textContent;
  await goto("dir->i_sb", 5);
  r.field = [$("fn-title").textContent, ksee.src.text().split("\n")[ksee.src.lineAt() - 1].trim()];
  if (!/called \d+×/.test(r.lens[0] ?? "") || !r.def.includes("may_create_dentry") || !r.field[0].includes("fs.h") || !r.field[1].includes("i_sb"))
    throw new Error("navigation: " + JSON.stringify(r));
  // Find a function in the recording: Enter, its next call after the cursor; Shift+Enter, the one before.
  const fbox = document.querySelector("input.find"), fkey = (k, shift = false) => fbox.dispatchEvent(new KeyboardEvent("keydown", { key: k, shiftKey: shift }));
  // (Its one call may be before the cursor: then Shift+Enter finds it.)
  fbox.value = "vfs_mkdir"; fkey("Enter"); await sleep(300);
  r.found = [document.querySelector(".findhint").textContent];
  if (r.found[0].startsWith("no call")) { fkey("Enter", true); await sleep(300); r.found.push(document.querySelector(".findhint").textContent); }
  r.found.push($("where").textContent.includes("vfs_mkdir"));
  fbox.value = "nope_fn"; fkey("Enter");
  r.noFn = document.querySelector(".findhint").textContent;
  if (!r.found.at(-2).match(/^\d+ of \d+$/) || !r.found.at(-1) || r.noFn !== "no function nope_fn") throw new Error("find: " + JSON.stringify(r));
  // The console, folded to its last line; the current task, open to its key fields.
  r.console = [$("pane-console").classList.contains("folded"), $("console").textContent];
  r.current = [...document.querySelectorAll("#stack tr.task, #stack tr.task ~ tr")].slice(0, 3).map((tr) => tr.innerText.replace(/\s+/g, " ").trim());
  if (!r.console[0] || !r.console[1] || r.console[1].includes("\n") || !r.current[0]?.startsWith("current task: init(1)") || !r.current[1]?.startsWith("pid")) throw new Error("console, current: " + JSON.stringify(r));
  return r;
})()
