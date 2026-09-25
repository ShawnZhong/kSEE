// The kSEE page: wires the panels to a booted Machine (crates/ksee-runtime).
//
// Run something, recorded (a tour's program, a program, or ● Record); then
// explore the recording at one cursor: the timeline shows it among the
// tasks and their calls; the source and the Stack show the code and the
// values there (memory as it was). ◀ ▶| and Play move it line by line.
import { $, el, taskName } from "./dom.js";
import { process } from "./ksee.js";
import { source } from "./source.js";
import { timeline } from "./timeline.js";
import { callstack } from "./callstack.js";
import { tours } from "./tours.js";

export function app(m) {
  const a = { m, proc: process(m) };
  // A syscall's identity across snapshots (indexes move as the ring goes on).
  a.key = (s) => `${s.task.addr}:${m.record_id(s.first)}`;
  const src = source(a), time = timeline(a), stack = callstack(a);
  a.showFunction = src.showFunction;
  a.src = src;
  a.time = time;
  // Expressions at the cursor (hover in the source; watches).
  a.resolve = (expr, only) => stack.resolve(expr, only);
  a.kids = (o) => stack.kids(o);
  a.watch = (expr, only) => stack.watch(expr, only);

  // ---- The state: recording, or exploring the recording ----
  let halted = null;
  const state = () => (halted != null ? "halted" : m.recording() ? "recording" : time.records() ? "replay" : "idle");
  function badge() {
    const st = state();
    $("badge").className = `badge ${st}`;
    $("badge").textContent = { idle: "○ IDLE", recording: "● RECORDING", replay: "↺ REPLAY", halted: "■ HALTED" }[st];
    $("record").textContent = m.recording() ? "■ Stop recording" : "● Record";
    $("status").textContent = {
      idle: "Run a tour or a program: the kernel's work for it is recorded, to explore here.",
      recording: `recording… ${time.records()} records`,
      replay: `a recording of ${time.records()} records: move the cursor through it`,
      halted: `the kernel halted (${halted})`,
    }[st];
  }

  // A new snapshot of the recording, everywhere.
  let calls = [];
  const refresh = () => {
    m.snapshot();
    calls = m.syscalls();
    time.refresh(calls);
    return calls;
  };
  a.refresh = refresh;
  $("record").onclick = () => {
    m.record(!m.recording());
    refresh();
    badge();
  };

  // ---- Memory: as it was at the cursor ----
  let memAt;
  function memoryAt(i) {
    if (i === memAt) return;
    memAt = i;
    const undone = m.memory_at(i ?? undefined);
    $("mem-note").textContent = i == null ? "memory now" : `memory at the cursor (${undone} writes undone)`;
    $("mem-note").className = "right " + (i != null ? "past" : "dim");
  }
  // ✎ for a value (an Object): which code last wrote it; click: go there.
  a.writer = (o) => el("a", { className: "who", textContent: " ✎", title: "who last wrote this?", onclick: (e) => {
    e.stopPropagation();
    const w = m.last_write(o.addr, o.size || 4);
    if (!w) return ($("where").textContent = `✎ ${o.name}: not written in the recording`);
    time.go(w.i);
    $("where").textContent = `✎ ${o.name} was last written here: by ${w.inlined ? `${w.inlined} in ` : ""}${w.func} (${w.file}:${w.line}), on ${taskName(w.task)}; before, it held ${w.old}`;
  } });

  // ---- Selecting, and moving the cursor ----

  // Select syscall `s`: its records; the cursor at its first line past the entry.
  a.select = (s) => {
    time.select(s);
    let i = s.first;
    for (let k = 0; k < 4; k++) {
      const at = m.at(i);
      if (at?.kind === "block" && at.func !== "wasm_syscall" && !at.func.startsWith("__wasm_sys_")) break;
      i = m.seek(i, 1, "function");
    }
    time.go(i);
  };
  // Select the run of program `pid` (Machine.program); the cursor at its start.
  a.selectProgram = (pid, title) => {
    const where = () => {
      const p = m.program(pid);
      return p && { a: p.first, b: p.last, task: p.task };
    };
    const w = where();
    if (!w) return;
    time.selectWindow(where, title);
    time.go(m.seek(w.a, 1, "line"));
  };
  // The first syscall of task `task` after record `i`.
  a.syscallAfter = (task, i) => calls.find((s) => s.task.addr === task && s.first > i);
  // The cursor moved to `at` (see Machine.at).
  a.showAt = (at) => {
    L.pos = null;
    memoryAt(at.i);
    src.view.cursor = at;
    const s = at.syscall != null ? calls.find((c) => c.index === at.syscall) : null;
    if (at.kind === "block") src.showFunction(at.func, s?.index, at.line, at.file);
    stack.show(at.i, null, at.task);
    tour?.follow(at);
    note(null);
    $("where").textContent = `record ${at.i + 1} of ${time.records()} · ${taskName(at.task)} · ${at.func} ${at.file ?? ""}:${at.line ?? ""}` +
      (s ? ` · in ${s.name}` : "");
    badge();
  };
  let tour = null;
  tours(a).then((t) => (a.tours = tour = t));

  // A tour's stop `stop` (the `k`th of `n`) at record `i`: the cursor there, and its note.
  a.goStop = (i, stop, k, n) => {
    time.go(i);
    note(stop, k, n);
  };
  function note(stop, k, n) {
    $("note").hidden = !stop;
    if (stop) $("note").replaceChildren(el("span", { className: "dim", textContent: `Stop ${k + 1} of ${n} · ` }),
      el("b", { textContent: stop.func }), `: ${stop.note} `,
      k + 1 < n ? el("button", { className: "go", textContent: "Next stop ▸", onclick: () => tour?.next() }) : null);
  }

  // ---- Lines: the line steps of the selection (a syscall or program) ----
  // (Machine.line_steps: every source line it ran, within blocks too.)
  const L = { key: null, steps: [], pos: null };
  function steps() {
    const w = time.home();
    if (!w) return (L.steps = []);
    // (A new snapshot moves the indexes: the newest record's id tells it.)
    const key = `${w.a}:${w.b}:${w.task}:${m.record_id(time.records() - 1)}`;
    if (key !== L.key) {
      L.key = key;
      L.steps = m.line_steps(w.a, w.b, w.task);
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
    time.mark(step.i);
    memoryAt(step.i);
    const at = m.at(step.i);
    src.view.cursor = { file: step.file, line: step.line };
    src.showFunction(step.func, undefined, step.line, step.file);
    stack.show(step.i, step, at?.task);
    tour?.follow(at);
    $("where").textContent = `line ${k + 1} of ${st.length} · ${step.inlined ? `${step.inlined} in ` : ""}${step.func} ${step.file}:${step.line}` +
      (at ? ` · record ${step.i + 1} · ${taskName(at.task)}` : "");
    return true;
  }
  const forward = () => steps().length > 0 && showStep(position() + 1);
  const back = () => steps().length > 0 && showStep(Math.max(0, position() - 1));
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
    const tickPlay = () => {
      if (!playing) return;
      if (!forward()) return stopPlay();
      playing = setTimeout(tickPlay, Number($("speed").value));
    };
    playing = setTimeout(tickPlay, 0);
  }
  $("play").onclick = play;
  $("fwd-line").onclick = forward;
  $("back-line").onclick = back;
  $("to-start").onclick = () => steps().length && showStep(0);
  addEventListener("keydown", (e) => {
    if (e.target.closest("input, select, textarea, .monaco-editor")) return;
    if (e.key === "ArrowRight") forward();
    else if (e.key === "ArrowLeft") back();
    else if (e.key === " ") play();
    else return;
    e.preventDefault();
  });

  // ---- The right column's tabs ----
  let current = "stack";
  function tab(name) {
    current = name;
    for (const t of ["stack", "console"]) {
      $(`tab-${t}`).classList.toggle("on", t === name);
      $(`pane-${t}`).hidden = t !== name;
    }
    if (name === "console") $("console-new").textContent = "";
  }
  for (const t of ["stack", "console"]) $(`tab-${t}`).onclick = () => tab(t);

  // ---- Resizing: the columns, and the timeline's height ----
  const sizes = { left: 340, right: 380, chart: 190 };
  try { Object.assign(sizes, JSON.parse(localStorage.getItem("ksee.sizes") ?? "{}")); } catch { /* none kept */ }
  const layout = () => {
    $("main").style.gridTemplateColumns = `${sizes.left}px 5px minmax(300px, 1fr) 5px ${sizes.right}px`;
    $("chartbox").style.height = `${sizes.chart}px`;
    try { localStorage.setItem("ksee.sizes", JSON.stringify(sizes)); } catch { /* not kept */ }
  };
  const drag = (handle, move) => (handle.onpointerdown = (e) => {
    e.preventDefault();
    const start = { x: e.clientX, y: e.clientY, ...sizes };
    const onMove = (ev) => {
      move(start, ev.clientX - start.x, ev.clientY - start.y);
      layout();
    };
    addEventListener("pointermove", onMove);
    addEventListener("pointerup", () => {
      removeEventListener("pointermove", onMove);
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
  layout();

  // ---- The loop: the console; while recording, the timeline grows ----
  let lastRefresh = 0;
  function tick() {
    const out = m.console();
    if (out) {
      $("console").textContent += out.replaceAll("\r", "");
      $("pane-console").scrollTop = $("pane-console").scrollHeight;
      if (current !== "console") $("console-new").textContent = "•";
    }
    const code = m.halted();
    if (code !== undefined) {
      halted = code;
      stopPlay();
      badge();
      return;
    }
    if (m.recording() && performance.now() - lastRefresh > 1000) {
      lastRefresh = performance.now();
      refresh();
    }
    badge();
    setTimeout(tick, 50);
  }
  refresh();
  stack.show(undefined);
  tick();
  a.state = state;
  return a;
}
