// The kSEE page: wires the panels to a booted Machine (crates/ksee-runtime).
//
// An example's program makes calls, each recorded; explore the recording
// at one cursor: the timeline shows it among the tasks and their calls;
// the source and the Stack show the code and the values there (memory as
// it was). ◀ ▶| and Play move it line by line.
import { $, taskName } from "./dom.js";
import { source } from "./source.js";
import { timeline } from "./timeline.js";
import { callstack } from "./callstack.js";
import { examples } from "./examples.js";
import { trace } from "./trace.js";
import { records } from "./records.js";


export function app(m) {
  const a = { m };
  const src = source(a), time = timeline(a), stack = callstack(a);
  // (The panels, for each other and for tests.)
  Object.assign(a, { src, time, stack });

  // ---- The state: a recording to explore, or none yet ----
  let halted = null;
  const state = () => (halted != null ? "halted" : time.records() ? "replay" : "idle");
  // (The status, in the transport: only what the panels do not say.)
  function badge() {
    const st = state();
    $("status").classList.toggle("halted", st === "halted");
    $("status").textContent = {
      idle: "Nothing recorded yet: an example's calls are, as it opens.",
      replay: "",
      halted: `the kernel halted (${halted})`,
    }[st];
  }

  // A new snapshot of the recording, everywhere.
  let calls = [];
  const refresh = () => {
    m.snapshot();
    calls = m.syscalls();
    time.refresh(calls);
    a.outlined?.();
    return calls;
  };
  a.refresh = refresh;

  // ---- Memory: as it was at the cursor ----
  let memAt;
  function memoryAt(i) {
    if (i === memAt) return;
    memAt = i;
    m.memory_at(i ?? undefined);
  }

  // ---- Selecting, and moving the cursor ----

  // The place shown, in the transport: `task` · `name` `file`:`line` · in `sc`.
  const where = ({ task, name, file, line, sc }) => ($("where").textContent =
    [task && taskName(task), `${name} ${file ?? ""}:${line ?? ""}`, sc && `in ${sc}`].filter(Boolean).join(" · "));

  // Select syscall `s`: its records; the cursor where its own code starts
  // (its handler's first line, past the syscall entry).
  a.select = (s) => {
    time.select(s);
    time.go(m.seek(s.first, 1, "function"));
    a.outlined?.();
  };
  // Select call `f` (a Stack frame: see Machine.call_stack) at the cursor:
  // the Stack marks it, the source shows where it is, the timeline outlines it.
  a.selectFrame = (f) => {
    stack.select(f.first, f.inlined ? f.func : null);
    src.view.cursor = { file: f.file, line: f.line };
    const at = time.cursor() != null ? m.at(time.cursor()) : null, s = at?.syscall != null ? calls.find((c) => c.index === at.syscall) : null;
    // (Inlined code: in the function it is inlined into, at its own line.)
    const owner = f.inlined ? stack.owner(f) : f;
    src.showFunction(owner?.func ?? f.func, at?.syscall ?? undefined, f.line, f.file);
    time.selected(f.first);
    where({ task: at?.task, name: f.func, file: f.file, line: f.line, sc: s?.name });
  };
  // Select the call on the stack at the cursor whose first record is
  // `first` (or code inlined into it, from function `inlined`).
  a.selectCall = (first, inlined) => {
    const f = stack.select(first, inlined);
    if (f) a.selectFrame(f);
  };
  a.calls = () => calls;
  // Show the moment `at` (see Machine.at): the source there, the Stack,
  // memory as it was, where it is. `frame` ({ first, inlined }, a call on
  // the stack then): that call selected on the Stack, the source at the
  // line it was at (the one calling what is above it), as a click on it
  // shows.
  function view(at, frame = null) {
    memoryAt(at.i);
    const s = at.syscall != null ? calls.find((c) => c.index === at.syscall) : null;
    stack.show(at.i, null, at.task);
    const f = frame && stack.select(frame.first, frame.inlined);
    if (f) document.querySelector("#stack tr.frame.on")?.scrollIntoView({ block: "nearest" });
    const here = f ? { func: f.func, file: f.file, line: f.line, inlined: null, owner: (f.inlined ? stack.owner(f) : f)?.func } : at;
    src.view.cursor = here;
    if (here.file) src.showFunction(here.owner ?? here.func, s?.index, here.line, here.file);
    where({ task: at.task, name: here.inlined ?? here.func, file: here.file, line: here.line, sc: s?.name });
  }
  // The cursor moved to `at`.
  a.showAt = (at) => {
    L.pos = null;
    view(at);
    a.trace?.mark();
    stack.objects.mark();
    a.records?.mark();
    badge();
  };
  // The view follows the pointer over the timeline: record `i` shown as if
  // the cursor were there, as call `frame` saw it if over one (once a
  // frame, and only as the moment or the call changes); null: back to the
  // cursor's. A click fixes the cursor (time.go).
  let peek = { raf: 0, key: null };
  a.peek = (i, frame = null) => {
    cancelAnimationFrame(peek.raf);
    peek.raf = requestAnimationFrame(() => {
      const to = i ?? time.cursor(), key = to == null ? null : `${to}:${frame?.first ?? ""}:${frame?.inlined ?? ""}`;
      if (key === peek.key) return;
      peek.key = i == null ? null : key;
      const at = to != null ? m.at(to) : null;
      if (at) view(at, frame);
    });
  };
  a.trace = trace(a);
  a.records = records(a);
  a.outlined = () => (a.trace.draw(), stack.objects.draw(), a.records.draw());
  // (The Records follow the cursor, drawn when the tab is looked at.)
  document.querySelector('h2.tabs a[data-tab="records"]').addEventListener("click", () => a.records.draw());
  examples(a);

  // ---- Lines: the line steps of the selection (a syscall) ----
  // (Machine.line_steps: every source line it ran, within blocks too.)
  const L = { key: null, steps: [], pos: null };
  function steps() {
    const w = time.home();
    if (!w) return (L.steps = []);
    // (A selection of any task, as the boot: the cursor's task's lines.)
    const task = w.task ?? m.at(time.cursor() ?? w.a)?.task.addr ?? 0;
    // (A new snapshot moves the indexes: the newest record's id tells it.)
    const key = `${w.a}:${w.b}:${task}:${m.record_id(time.records() - 1)}`;
    if (key !== L.key) {
      L.key = key;
      L.steps = m.line_steps(w.a, w.b, task);
      L.pos = null;
    }
    return L.steps;
  }
  // Where in the steps the cursor is (the first step at or after it).
  function position() {
    const st = steps();
    if (L.pos != null && L.pos < st.length) return L.pos;
    const k = st.findIndex((x) => x.i >= (time.cursor() ?? 0));
    return (L.pos = k < 0 ? st.length - 1 : k);
  }
  // Show step `k`: the cursor at its record, its line in the source.
  function showStep(k) {
    const st = steps(), step = st[k];
    if (!step) return false;
    L.pos = k;
    showLine(step);
    return true;
  }
  // Go to the call from record `first`: its call site, as a debugger stops
  // before stepping in: the cursor at the call's first record, the
  // caller's frame selected (the source shows the calling line, the Stack
  // has the callee with its arguments on top); → or ↑ then enters it.
  // (Inlined code, no call: the cursor at its first record.)
  a.goCall = (first) => {
    time.go(first);
    const frames = stack.frames(), k = frames.findIndex((f) => f.first === first && !f.inlined);
    const caller = k >= 0 ? frames[k + 1] : null;
    if (caller) a.selectFrame(caller);
  };
  // Show line `step` (see Machine.line_steps): the cursor at its record.
  function showLine(step) {
    time.mark(step.i);
    a.trace?.mark();
    stack.objects.mark();
    a.records?.mark();
    memoryAt(step.i);
    const at = m.at(step.i);
    src.view.cursor = { file: step.file, line: step.line };
    src.showFunction(step.func, undefined, step.line, step.file);
    // (The stack as it runs: see Machine.call_stack's nth.)
    stack.show(step.i, step, at?.task);
    where({ task: at?.task, name: step.inlined ?? step.func, file: step.file, line: step.line });
  }
  // Step: by a `line` (into calls), `over` calls (the next line at this
  // level or out of it), or `out` (the next line out of it), `dir` 1
  // forward, -1 back. The level: the Stack's selected call's, else the
  // line's (calls deep, then inlined deep: inlined code is stepped over
  // too). False if there is no such line.
  function step(kind, dir) {
    const st = steps();
    if (!st.length) return false;
    const k = position(), { depth: d, inl: i } = stack.level() ?? st[k];
    // (A line's level against this one: -1 shallower, 0 the same, 1 deeper.)
    const cmp = (s) => Math.sign(s.depth - d || s.inl - i);
    const stop = { line: () => true, over: (s) => cmp(s) <= 0, out: (s) => cmp(s) < 0 }[kind];
    for (let j = k + dir; j >= 0 && j < st.length; j += dir) if (stop(st[j])) return showStep(j);
    return false;
  }
  // Run to line `line` of `file`: the next (dir 1) or the last (-1) time it
  // runs, from the cursor; its syscall selected (line steps go on from it).
  a.runTo = (file, line, dir) => {
    const i = m.run_of(file, line, time.cursor() ?? 0, dir);
    if (i == null) return ($("where").textContent = `${file}:${line} does not run ${dir > 0 ? "again" : "before"} in the recording`);
    const at = m.at(i), s = at?.syscall != null ? calls.find((c) => c.index === at.syscall) : null;
    if (s) time.select(s);
    time.mark(i);
    const st = steps(), k = st.findIndex((x) => x.i === i && x.line === line && x.file === file);
    return k >= 0 ? showStep(k) : time.go(i);
  };
  let playing = null;
  function stopPlay() {
    clearTimeout(playing);
    playing = null;
    $("play").textContent = "▶ Play";
  }
  function play() {
    if (playing) return stopPlay();
    if (!steps().length) return;
    $("play").textContent = "❚❚ Pause";
    const by = $("step-by").value;
    const tickPlay = () => {
      if (!playing) return;
      if (!step(by, 1)) return stopPlay();
      playing = setTimeout(tickPlay, Number($("speed").value));
    };
    playing = setTimeout(tickPlay, 0);
  }
  $("play").onclick = play;
  $("step-back").onclick = () => step($("step-by").value, -1);
  $("step-fwd").onclick = () => step($("step-by").value, 1);
  // (← →: a line; with Shift: over; with Alt: out. F10, F11, Shift+F11 as
  // in a debugger; ↑ ↓: the row above or below on the Stack (the callee,
  // the caller: the innermost is on top; steps go on at that call's
  // level); Home: the first line; /: find a function.)
  addEventListener("keydown", (e) => {
    if (e.target.closest?.("input, select, textarea, .cm-editor")) return;
    const arrow = { ArrowRight: 1, ArrowLeft: -1 }[e.key], updown = { ArrowUp: -1, ArrowDown: 1 }[e.key];
    if (arrow) step(e.altKey ? "out" : e.shiftKey ? "over" : "line", arrow);
    else if (updown) {
      const f = stack.neighbour(updown);
      if (f) (a.selectFrame(f), document.querySelector("#stack tr.frame.on")?.scrollIntoView({ block: "nearest" }));
    }
    else if (e.key === "Home") steps().length && showStep(0);
    else if (e.key === "/") time.focusFind();
    else if (e.key === "F10") step("over", 1);
    else if (e.key === "F11") step(e.shiftKey ? "out" : "line", 1);
    else if (e.key === " ") play();
    else return;
    e.preventDefault();
  });

  // ---- The console: all of it, or (folded) its last line ----
  let consoleText = "";
  function showConsole() {
    $("console").textContent = sizes.consoleOpen ? consoleText : consoleText.trimEnd().split("\n").at(-1) ?? "";
    $("pane-console").scrollTop = $("pane-console").scrollHeight;
  }

  // ---- Resizing: the columns, and the timeline's height ----
  const sizes = { left: 340, right: 380, chart: 190, console: 140, consoleOpen: false };
  try { Object.assign(sizes, JSON.parse(localStorage.getItem("ksee.sizes") ?? "{}")); } catch { /* none kept */ }
  const layout = () => {
    $("app").style.gridTemplateColumns = `${sizes.left}px 5px minmax(0, 1fr)`;
    $("main").style.gridTemplateColumns = `minmax(300px, 1fr) 5px ${sizes.right}px`;
    $("chartbox").style.height = `${sizes.chart}px`;
    // (Folded: its last line only.)
    $("pane-console").classList.toggle("folded", !sizes.consoleOpen);
    $("pane-console").style.flexBasis = `${sizes.console}px`;
    $("console-tw").textContent = sizes.consoleOpen ? "▾" : "▸";
    showConsole();
    try { localStorage.setItem("ksee.sizes", JSON.stringify(sizes)); } catch { /* not kept */ }
  };
  // (`click`: a press that did not move.)
  const drag = (handle, move, click) => (handle.onpointerdown = (e) => {
    e.preventDefault();
    const start = { x: e.clientX, y: e.clientY, ...sizes };
    let moved = false;
    const onMove = (ev) => {
      moved ||= Math.abs(ev.clientX - start.x) + Math.abs(ev.clientY - start.y) > 3;
      if (!moved) return;
      move(start, ev.clientX - start.x, ev.clientY - start.y);
      layout();
    };
    addEventListener("pointermove", onMove);
    addEventListener("pointerup", () => {
      removeEventListener("pointermove", onMove);
      if (!moved) click?.();
      layout();
      dispatchEvent(new Event("resize")); // (the canvases redraw)
    }, { once: true });
  });
  for (const h of document.querySelectorAll(".vsplit")) {
    drag(h, (s, dx) => {
      if (h.dataset.col === "left") sizes.left = Math.max(200, s.left + dx);
      else sizes.right = Math.max(220, s.right - dx);
    });
  }
  drag($("hsplit"), (s, _, dy) => (sizes.chart = Math.max(60, s.chart + dy)));
  // (The console: folded to its last line, or open, as tall as dragged.)
  drag($("console-head"), (s, _, dy) => sizes.consoleOpen && (sizes.console = Math.max(40, s.console - dy)),
       () => (sizes.consoleOpen = !sizes.consoleOpen));
  layout();

  // ---- Starting again, and watching the kernel (its console, a halt) ----
  // Start the kernel again (a fresh one: the page stays); done once /init
  // is ready for calls.
  a.restart = async () => {
    halted = null;
    stopPlay();
    consoleText = "";
    showConsole();
    await m.restart();
    refresh();
    stack.show(undefined);
    badge();
  };
  function tick() {
    const out = m.console();
    if (out) {
      consoleText += out.replaceAll("\r", "");
      showConsole();
    }
    const code = m.halted();
    if (code !== undefined && halted == null) {
      halted = code;
      stopPlay();
    }
    badge();
    setTimeout(tick, 50);
  }
  refresh();
  stack.show(undefined);
  tick();
  return a;
}
