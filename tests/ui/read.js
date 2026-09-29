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
    // (The first, ksys_read's call site: its caller's line, its names the caller's, not a global's.)
    if (k === 0) r.site = document.querySelector(".cm-editor .values")?.textContent;
    if (k === 0 && !/count = 64/.test(r.site ?? "")) throw new Error("the call site's values: " + JSON.stringify(r.site));
    if (k === 1) r.stack = [...document.querySelectorAll("#stack tr.frame")].slice(0, 2).map((f) => f.innerText.trim());
    if (k === 1) r.args = [...document.querySelectorAll("#stack tr")].slice(1, 4).map((f) => f.innerText.replace(/\s+/g, " ").trim());
    if (a.textContent === "vfs_read") {
      // (The link lands at the call site, in ksys_read; ↑ enters vfs_read.)
      dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowUp" }));
      await sleep(300);
      r.hover = await ksee.src.hoverAt("file->f_mode & FMODE_READ", "f_mode");
      // (The cursor's line, in vfs_read, carries its values at its end.)
      r.values = document.querySelector(".cm-editor .values")?.textContent;
      if (!/\w+ = /.test(r.values ?? "")) throw new Error("no values on the cursor's line: " + JSON.stringify([r.values, document.querySelector(".cm-editor .ln-cursor")?.textContent]));
      // (A type name: its declaration's first lines.)
      r.decl = await ksee.src.hoverAt("struct kiocb kiocb", "kiocb");
    }
  }
  if (!r.decl?.startsWith("include/linux/fs.h:") || !r.decl.includes("struct kiocb {")) throw new Error("no declaration on hover: " + JSON.stringify(r.decl));
  if (r.notes.length < 3 || !r.notes.some(([n, w, top]) => n === "vfs_read" && top === "vfs_read" && w.includes("ksys_read")) || !r.hover) throw new Error("links: " + JSON.stringify(r.notes));
  // Run to a line (the source's F4): the line where the cursor is, its next run
  // (stepping on until a line that does run again), from the explanation's first link.
  document.querySelector(".explain a.place")?.click(); await sleep(300);
  let here, file, ln;
  for (let k = 0; k < 40; k++) {
    here = $("where").textContent;
    [, file, ln] = here.match(/ (\S+):(\d+)/) ?? [];
    // (A line with steps of its own later, not only an inline site: run to it.)
    const cur = ksee.time.cursor();
    if (file && ksee.m.run_of(file, Number(ln), cur, 1) != null && ksee.lineSteps().some((x) => x.file === file && x.line === Number(ln) && x.i > cur)) break;
    $("step-fwd").click();
  }
  const was = ksee.time.cursor();
  if (file) ksee.runTo(file, Number(ln), 1);
  r.ranTo = [here, $("where").textContent, was, ksee.time.cursor()];
  // (The same line, its next run; the where line may say what returned at one of them and not the other.)
  const sameLine = (x) => x.replace(/ · \w+ returned [^·]*/, "").trim();
  if (!file || !(r.ranTo[3] > was) || sameLine(r.ranTo[1]) !== sameLine(here)) throw new Error("run to: " + JSON.stringify(r.ranTo));
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
  // (Back in the caller after the call returned: the where line says what it returned.)
  r.returned = $("where").textContent;
  if (!/\w+ returned /.test(r.returned)) throw new Error("after the return: " + JSON.stringify(r.returned));
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
  const read = ksee.m.syscalls().findLast((x) => x.name === "read" && x.task.pid === ksee.m.main() && x.last != null);
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
  // (The click chose that call: the pointer wandering off, then leaving, comes back to it, not to the innermost.)
  $("chart").dispatchEvent(new PointerEvent("pointermove", { clientX: box.x, clientY: $("chart").getBoundingClientRect().top + 2 }));
  await sleep(150);
  $("chart").dispatchEvent(new PointerEvent("pointerleave"));
  await sleep(150);
  const back = [$("where").textContent.slice(0, 50), document.querySelector("#stack tr.frame.on")?.innerText.trim().slice(0, 50)];
  if (back[0] !== r.clicked[0] || back[1] !== r.clicked[1]) throw new Error("leaving the timeline: " + JSON.stringify([r.clicked, back]));
  // (A syscall's box: the syscall selected, entry to exit.)
  const home = ksee.time.home();
  if (home?.a !== read.first || home?.b !== read.last) throw new Error("the read's selection: " + JSON.stringify(home) + ", not " + read.first + ".." + read.last);
  // A call's hover says what it returned: ksys_read(…) = 26.
  const kbox = ksee.time.boxOf(ksee.m.first_call("ksys_read", read.first, read.last, read.task.addr));
  $("chart").dispatchEvent(new PointerEvent("pointermove", { clientX: kbox?.x, clientY: kbox?.y }));
  r.rets = [document.querySelector(".tip:not([hidden])")?.firstChild?.textContent];
  $("chart").dispatchEvent(new PointerEvent("pointerleave"));
  if (!/^ksys_read = \d+$/.test(r.rets[0] ?? "")) throw new Error("no return value in a call's hover: " + JSON.stringify(r.rets));
  ksee.time.go(ksee.m.first_call("vfs_read", read.first, read.last, read.task.addr));
  // The Objects (the explanation's link): what the call did, each object
  // it wrote, member by member (the kiocb's ki_pos, 0 → 18, by
  // filemap_read); a row goes to the write that left it so.
  const before = ksee.time.cursor();
  [...document.querySelectorAll(".explain a.place")].find((x) => /changed/.test(x.textContent))?.click();
  await until(() => $("right").dataset.tab === "objects" && document.querySelector("#objects .obj"));
  const kipos = [...document.querySelectorAll("#objects tr")].find((tr) => tr.cells[0]?.textContent === "ki_pos");
  r.objects = [kipos?.innerText.replace(/\s+/g, " ")];
  if (!/^ki_pos 0 → \d+ filemap_read$/.test(r.objects[0] ?? "")) throw new Error("objects: " + JSON.stringify(r.objects));
  kipos.click(); await sleep(200);
  if (ksee.time.cursor() !== Number(kipos.dataset.last) || !kipos.classList.contains("on")) throw new Error("a change's row goes to its write: " + JSON.stringify([ksee.time.cursor(), kipos.dataset.last]));
  ksee.tab("stack");
  ksee.select(read); ksee.time.go(before); // (as it was)
  // (At a store: its line in the source says what it stored, at its end.)
  const lastStore = ksee.m.stores(read.first, read.last).at(-1);
  ksee.time.go(lastStore.i);
  await sleep(300);
  r.stored = [...document.querySelectorAll(".cm-editor a.stored")].map((a) => a.textContent.trim()).slice(0, 3);
  if (!r.stored.length) throw new Error("no stored notes in the source at " + $("where").textContent);
  document.querySelector('h2.tabs a[data-tab="stack"]').click();
  r.selected = $("chart-title").textContent;
  // (Any call's box, clicked: the call in focus, from its first record to
  // its return (what the Objects say); the syscall still selected.)
  const ksys = ksee.m.call_stack(ksee.m.first_call("ksys_read", read.first, read.last, read.task.addr)).find((f) => f.func === "ksys_read");
  const kb = ksee.time.boxOf(ksys.first), readSel = ksee.time.home();
  for (const t of ["pointerdown", "pointerup"]) $("chart").dispatchEvent(new PointerEvent(t, { clientX: kb.x, clientY: kb.y }));
  const kf = ksee.time.focus();
  if (kf?.a !== ksys.first || kf?.b !== ksee.m.call_end(ksys.first) || ksee.time.home() !== readSel) throw new Error("ksys_read in focus: " + JSON.stringify([kf, ksee.time.home()]) + ", not from " + ksys.first);
  ksee.tab("objects");
  // (The Objects stay the syscall's, offering the call clicked; narrowed
  // on asking.)
  const head = () => document.querySelector("#objects .head")?.textContent;
  r.clicked = head();
  if (!/^What (read by prog\(\d+\)|filemap_read) did.* · only ksys_read$/.test(r.clicked ?? "")) throw new Error("the Objects after a click: " + r.clicked);
  [...document.querySelectorAll("#objects .head a")].find((a) => a.textContent === "only ksys_read")?.click();
  r.focused = head();
  if (!/^What ksys_read did/.test(r.focused ?? "")) throw new Error("the Objects narrowed to the call clicked: " + r.focused);
  // (Zoom: the window at the call in focus, from the chart's head.)
  const zoomTo = [...document.querySelectorAll("#chart-verbs a")].find((x) => x.textContent.startsWith("zoom"));
  r.zoom = zoomTo?.textContent;
  if (r.zoom !== "zoom to ksys_read") throw new Error("zoom to the call in focus: " + r.zoom);
  zoomTo.click();
  if (![...document.querySelectorAll("#chart-verbs a")].some((x) => x.textContent === "whole")) throw new Error("zoomed: no way back to the whole recording");
  // (And back to the whole syscall, by the link in its head.)
  document.querySelector("#objects .head a")?.click();
  r.unfocused = head();
  if (!/^What read by prog\(\d+\) did/.test(r.unfocused ?? "")) throw new Error("the whole syscall again: " + r.unfocused);
  ksee.tab("stack");
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
  await until(() => document.querySelectorAll("ol.steps > li.done").length === lis().length && document.querySelector(".explain a.place.on"));
  r.switched = [location.hash, window.stayed, line(lis()[1])];
  // (Its first line declares its buffer; its first call is mkdirat.)
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
  // The console, folded to its last line; the current task, a link to the
  // Tasks: there, open, its key fields; each task, where it is.
  r.console = [$("pane-console").classList.contains("folded"), $("console").textContent];
  const cur = document.querySelector("#stack tr.task a");
  r.current = [cur?.textContent];
  cur?.click(); await sleep(200);
  r.current.push($("right").dataset.tab, ...[...document.querySelectorAll("#tasks tr.on, #tasks tr.on ~ tr")].slice(0, 14).map((tr) => tr.innerText.replace(/\s+/g, " ").trim()));
  r.tasks = [...document.querySelectorAll("#tasks tr.task")].map((tr) => tr.innerText.replace(/\s+/g, " ").trim());
  if (!r.console[0] || !r.console[1] || r.console[1].includes("\n") || r.current[0] !== `current task: prog(${ksee.m.main()})` || r.current[1] !== "tasks"
      || !r.current[2]?.includes(`prog(${ksee.m.main()}) in `) || !r.current.slice(3).some((x) => x.startsWith("pid")) || !r.tasks.length) throw new Error("console, current: " + JSON.stringify(r));
  ksee.tab("stack");
  return r;
})()
