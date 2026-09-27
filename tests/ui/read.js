// The default: the read example (#ex=read): its calls made by themselves; its explanation's links (the
// Stack, a hover at each), stepping, the chart (hover, click), typing and changing calls,
// another example; returns a report.
(async () => {
    const r = {};
  // (All its calls made, the cursor at the explanation's first link.)
  await until(() => document.querySelectorAll("ol.steps > li.done").length === lis().length && document.querySelector(".explain a.place.on"));
  const pick = document.querySelector("#ex-head select");
  r.first = [pick.selectedOptions[0]?.textContent, line(lis()[0])];
  r.examples = pick.options.length;
  r.calls = lis().map(line);
  // The explanation's links, each gone to: the function's call site (the caller's line; the call on top of the Stack).
  r.notes = [];
  const places = [...document.querySelectorAll(".explain a.place")];
  for (const [k, a] of places.entries()) {
    a.click();
    await sleep(300);
    r.notes.push([a.textContent, $("where").textContent.slice(0, 60), document.querySelector("#stack tr.frame")?.innerText.trim().split(/\s/)[0]]);
    if (k === 1) r.stack = [...document.querySelectorAll("#stack tr.frame")].slice(0, 2).map((f) => f.innerText.trim());
    if (k === 1) r.args = [...document.querySelectorAll("#stack tr")].slice(1, 4).map((f) => f.innerText.replace(/\s+/g, " ").trim());
    if (a.textContent === "vfs_read") {
      // (The link lands at the call site, in ksys_read; ↑ enters vfs_read.)
      dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp" }));
      await sleep(300);
      r.hover = await ksee.src.hoverAt("file->f_mode & FMODE_READ", "f_mode");
      // (A type name: its declaration's first lines.)
      r.decl = await ksee.src.hoverAt("struct kiocb kiocb", "kiocb");
    }
  }
  if (!r.decl?.startsWith("include/linux/fs.h:") || !r.decl.includes("struct kiocb {")) throw new Error("no declaration on hover: " + JSON.stringify(r.decl));
  if (r.notes.length < 3 || !r.notes.some(([n, w, top]) => n === "vfs_read" && top === "vfs_read" && w.includes("ksys_read")) || !r.hover) throw new Error("links: " + JSON.stringify(r.notes));
  // Pin a value: click one in the Stack (one in memory: a local's, a task's field); the Objects show it pinned,
  // its writes listed (a local's in its frame's lifetime), ▶ goes to the next; 📌 unpins.
  const v = document.querySelector("#stack td.fval.mem");
  v?.click();
  r.history = [v?.textContent, document.querySelectorAll("#objects .pinned li").length, $("objects").innerText.slice(0, 160)];
  if (!/changes? (while \w+'s frame lives|in the recording)/.test(r.history[2] ?? "") || $("right").dataset.tab !== "objects" || !document.querySelector("#objects .pinned")) throw new Error("no pin: " + JSON.stringify(r.history));
  document.querySelector("#objects .pinned .whead button:not([disabled])")?.click();
  await sleep(200);
  document.querySelector("#objects .pinned .whead a.pin").click();
  // (The example's own pin stays: the read's struct file, pinned as it opened.)
  if (!$("objects").innerText.includes("fd 3 note.txt") || document.querySelectorAll("#objects .pinned").length !== 1) throw new Error("the example's pin: " + $("objects").innerText.slice(0, 160));
  document.querySelector('h2.tabs a[data-tab="stack"]').click();
  // Run to a line (the source's F4): the line where the cursor is, its next run
  // (stepping on until a line that does run again).
  let here, file, ln;
  for (let k = 0; k < 40; k++) {
    here = $("where").textContent;
    [, file, ln] = here.match(/ (\S+):(\d+)/) ?? [];
    if (file && ksee.m.run_of(file, Number(ln), ksee.time.cursor(), 1) != null) break;
    $("step-fwd").click();
  }
  const was = ksee.time.cursor();
  if (file) ksee.runTo(file, Number(ln), 1);
  r.ranTo = [here, $("where").textContent, was, ksee.time.cursor()];
  if (!file || !(r.ranTo[3] > was) || r.ranTo[1] !== here) throw new Error("run to: " + JSON.stringify(r.ranTo));
  // Stepping: → with "into calls" on by any line until a call is entered, → within that call, ↓ then → out of it
  // (the Stack's calls, deep), from the explanation's first link (a known place: ksys_read's call
  // site), so that the run is the same every time.
  document.querySelector(".explain a.place").click();
  await sleep(300);
  const depthNow = () => document.querySelectorAll("#stack tr.frame").length;
  const press = (key) => dispatchEvent(new KeyboardEvent("keydown", { key }));
  const frames = () => [...document.querySelectorAll("#stack tr.frame")].map((f) => f.innerText.trim().split("\n")[0].slice(0, 28));
  press("ArrowUp"); // (into ksys_read: the innermost, selected)
  const d0 = depthNow(), trail = [[$("where").textContent, frames()]];
  $("into").checked = true;
  for (let k = 0; k < 200 && depthNow() <= d0; k++) press("ArrowRight");
  $("into").checked = false;
  const d1 = depthNow();
  trail.push([$("where").textContent, frames()]);
  press("ArrowRight");
  const d2 = depthNow();
  trail.push([$("where").textContent, frames()]);
  $("step-down").click(); // (the button is the key)
  press("ArrowRight");
  // (↓: the row below, the caller, selected on the Stack; ↑: back up.)
  dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown" }));
  const onRow = () => [...document.querySelectorAll("#stack tr.frame")].findIndex((f) => f.classList.contains("on"));
  if (onRow() !== 1) throw new Error("↓ did not select the caller: row " + onRow());
  dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp" }));
  if (onRow() !== 0) throw new Error("↑ did not select the callee: row " + onRow());
  const d3 = depthNow();
  trail.push([$("where").textContent, frames()]);
  r.stepped = [d0, d1, d2, d3];
  if (!(d1 > d0 && d2 <= d1 && d3 <= d2 && d3 < d1)) throw new Error("stepping: " + JSON.stringify(r.stepped) + " " + JSON.stringify(trail));
  r.chart = $("chart-title").textContent;
  // (The read's own box: its syscall entry's.)
  const read = ksee.m.syscalls().findLast((x) => x.name === "read" && x.task.pid === 1 && x.last != null);
  ksee.select(read); // (its box in the window: the tutorial may have ended in another call)
  const box = ksee.time.boxOf(read.first), at = { clientX: box?.x, clientY: box?.y };
  const cursorTitle = $("where").textContent; // (the cursor's: the view comes back to it)
  $("chart").dispatchEvent(new PointerEvent("pointermove", at));
  r.tip = document.querySelector(".tip:not([hidden])")?.innerText.replace(/\s+/g, " ").slice(0, 60);
  // (Above the stack, at the top of the chart: the call on top then.)
  $("chart").dispatchEvent(new PointerEvent("pointermove", { clientX: box.x, clientY: $("chart").getBoundingClientRect().top + 2 }));
  r.top = document.querySelector(".tip:not([hidden])")?.firstChild?.textContent;
  if (!r.top) throw new Error("no call on top above the stack");
  $("chart").dispatchEvent(new PointerEvent("pointermove", at));
  await sleep(400);
  // (The view follows the pointer: the hovered moment's file; back to the cursor's on leaving.)
  r.hovered = [$("fn-title").textContent, $("where").textContent];
  $("chart").dispatchEvent(new PointerEvent("pointerleave"));
  await sleep(300);
  if (r.hovered[1] === cursorTitle || $("where").textContent !== cursorTitle) throw new Error("hover: " + JSON.stringify([r.hovered, cursorTitle, $("where").textContent]));
  for (const type of ["pointerdown", "pointerup"])
    $("chart").dispatchEvent(new PointerEvent(type, at));
  r.clicked = [$("where").textContent.slice(0, 50), document.querySelector("#stack tr.frame.on")?.innerText.trim().slice(0, 50)];
  // (A syscall's box: the syscall selected, entry to exit.)
  const home = ksee.time.home();
  if (home?.a !== read.first || home?.b !== read.last) throw new Error("the read's selection: " + JSON.stringify(home) + ", not " + read.first + ".." + read.last);
  // The Calls tab: the read as a tree of every call, in order, foldable; the call the cursor is in marked.
  document.querySelector('h2.tabs a[data-tab="trace"]').click();
  await sleep(100);
  r.trace = [$("trace-note").textContent, [...document.querySelectorAll("#trace li a.mono")].map((a) => a.textContent).slice(0, 6), $("trace").querySelector(".head")?.textContent];
  // (The spine opens by itself: ksys_read, then its calls, vfs_read among them, inlined code too; ▸ folds.)
  const inner = [...document.querySelectorAll("#trace li ul li > a.mono")].map((a) => a.textContent);
  if (!/^\d+ calls/.test(r.trace[2] ?? "") || !inner.includes("vfs_read") || !document.querySelector("#trace a.inl")) throw new Error("the calls: " + JSON.stringify([r.trace, inner]));
  const ksysLi = () => [...document.querySelectorAll("#trace li")].find((li) => li.querySelector(":scope > a.mono")?.textContent === "ksys_read");
  ksysLi().querySelector(":scope > a.tw").click();
  if (ksysLi().querySelector("ul")) throw new Error("folding ksys_read left its calls shown");
  ksysLi().querySelector(":scope > a.tw").click();
  [...document.querySelectorAll("#trace li")].find((li) => li.querySelector(":scope > a.mono")?.textContent === "vfs_read").querySelector(":scope > a.tw").click();
  ksee.time.go(ksee.m.first_call("vfs_read", read.first, read.last, read.task.addr));
  if (document.querySelector("#trace li.on > a.mono")?.textContent !== "vfs_read") throw new Error("the cursor's call not marked: " + document.querySelector("#trace li.on")?.textContent);
  document.querySelector('h2.tabs a[data-tab="stack"]').click();
  // The Objects tab: what the read changed, by object (the struct file's f_pos among them: pinned, as the example says;
  // the kiocb, the inode; no registers or frames); ▸ opens a value; "all" shows the rest.
  document.querySelector('h2.tabs a[data-tab="objects"]').click();
  await sleep(100);
  const fileObj = [...document.querySelectorAll("#objects .changed")].find((d) => d.innerText.includes("f_pos: 0 → "));
  if (!fileObj || !fileObj.classList.contains("pinned") || $("objects").innerText.includes("pt_regs") || $("objects").innerText.includes("'s frame")) throw new Error("the read's objects: " + $("objects").innerText.slice(0, 400));
  fileObj.querySelector(".whead a.tw").click();
  if (!document.querySelector("#objects .pinned table.objs")) throw new Error("the file's value: " + $("objects").innerText.slice(0, 200));
  document.querySelector("#objects .head a").click(); // (all: the registers and frames too)
  if (!$("objects").innerText.includes("pt_regs")) throw new Error("all objects: " + $("objects").innerText.slice(0, 200));
  document.querySelector("#objects .head a").click();
  // The Records tab: the raw trace around the cursor, the cursor's marked.
  document.querySelector('h2.tabs a[data-tab="records"]').click();
  await sleep(100);
  if (!document.querySelector("#records .row.on") || !document.querySelector("#records .row.store")) throw new Error("the records: " + $("records").innerText.slice(0, 200));
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
  // Code navigation: F12 to a function, and to a field.
  ksee.src.showFunction("vfs_mkdir");
  await sleep(1200);
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
  if (!r.def.includes("may_create_dentry") || !r.field[0].includes("fs.h") || !r.field[1].includes("i_sb"))
    throw new Error("navigation: " + JSON.stringify(r));
  // Find a function in the recording: Enter, its next call after the cursor; Shift+Enter, the one before.
  const fbox = document.querySelector("input.find"), fkey = (k, shift = false) => fbox.dispatchEvent(new KeyboardEvent("keydown", { key: k, shiftKey: shift }));
  // (Its one call may be before the cursor: then Shift+Enter finds it.)
  fbox.value = "vfs_mkdir"; fkey("Enter"); await sleep(300);
  r.found = [document.querySelector(".findhint").textContent];
  if (r.found[0].startsWith("no call")) { fkey("Enter", true); await sleep(300); r.found.push(document.querySelector(".findhint").textContent); }
  r.found.push(document.querySelector("#stack tr.frame")?.innerText.includes("vfs_mkdir")); // (at its call site: the call on top)
  fbox.value = "nope_fn"; fkey("Enter");
  r.noFn = document.querySelector(".findhint").textContent;
  if (!r.found.at(-2).match(/^\d+ of \d+$/) || !r.found.at(-1) || r.noFn !== "no function nope_fn") throw new Error("find: " + JSON.stringify(r));
  // The console, folded to its last line; the current task, open to its key fields.
  r.console = [$("pane-console").classList.contains("folded"), $("console").textContent];
  r.current = [...document.querySelectorAll("#stack tr.task, #stack tr.task ~ tr")].slice(0, 3).map((tr) => tr.innerText.replace(/\s+/g, " ").trim());
  if (!r.console[0] || !r.console[1] || r.console[1].includes("\n") || !r.current[0]?.startsWith("current task: init(1)") || !r.current[1]?.startsWith("pid")) throw new Error("console, current: " + JSON.stringify(r));
  return r;
})()
