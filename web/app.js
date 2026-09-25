// The kSEE page: wires the panels to a booted Machine (crates/ksee-runtime).
//
// Make calls, each recorded (an example's, or your own); then
// explore the recording at one cursor: the timeline shows it among the
// tasks and their calls; the source and the Stack show the code and the
// values there (memory as it was). ◀ ▶| and Play move it line by line.
import { $, el, taskName } from "./dom.js";
import { source } from "./source.js";
import { timeline } from "./timeline.js";
import { callstack } from "./callstack.js";
import { examples } from "./examples.js";

export function app(m) {
  const a = { m };
  const src = source(a), time = timeline(a), stack = callstack(a);
  // (The panels, for each other and for tests.)
  Object.assign(a, { src, time, stack });

  // ---- The state: a recording to explore, or none yet ----
  let halted = null;
  const state = () => (halted != null ? "halted" : time.records() ? "replay" : "idle");
  function badge() {
    const st = state();
    $("badge").className = `badge ${st}`;
    $("badge").textContent = { idle: "○ IDLE", replay: "↺ REPLAY", halted: "■ HALTED" }[st];
    $("status").textContent = {
      idle: "Open an example and make its calls: the kernel's work for them is recorded, to explore here.",
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

  // Select syscall `s`: its records; the cursor where its own code starts
  // (its handler's first line, past the syscall entry).
  a.select = (s) => {
    time.select(s);
    time.go(m.seek(s.first, 1, "function"));
  };
  // Select call `f` (a Stack frame: see Machine.call_stack) at the cursor:
  // the Stack marks it, the source shows where it is, the timeline outlines it.
  a.selectFrame = (f) => {
    clearTimeout(peek);
    stack.select(f.first, f.inlined ? f.func : null);
    src.view.cursor = { file: f.file, line: f.line };
    const at = time.cursor() != null ? m.at(time.cursor()) : null;
    // (Inlined code: in the function it is inlined into, at its own line.)
    const owner = f.inlined ? stack.owner(f) : f;
    src.showFunction(owner?.func ?? f.func, at?.syscall ?? undefined, f.line, f.file);
    time.selected(f.first);
    $("where").textContent = `${f.func}${f.inlined ? ` (inlined into ${owner?.func})` : ""} ${f.file ?? ""}:${f.line ?? ""}` + (time.cursor() != null ? ` · record ${time.cursor() + 1}` : "");
  };
  // Preview a call in the source while it is hovered (in the timeline or
  // the Stack): `get()` says where ({ func: the function to show, file,
  // line }); null: back to what was shown. (After a pause: no flicker
  // while the pointer crosses the chart.)
  let peek = 0;
  a.preview = (get) => {
    clearTimeout(peek);
    peek = setTimeout(() => {
      const p = get?.();
      if (p) src.preview(p.func, p.file, p.line);
      else src.unpreview();
    }, 40);
  };
  // Select the call on the stack at the cursor whose first record is
  // `first` (or code inlined into it, from function `inlined`).
  a.selectCall = (first, inlined) => {
    const f = stack.select(first, inlined);
    if (f) a.selectFrame(f);
  };
  a.calls = () => calls;
  // The cursor moved to `at` (see Machine.at).
  a.showAt = (at) => {
    L.pos = null;
    memoryAt(at.i);
    src.view.cursor = at;
    const s = at.syscall != null ? calls.find((c) => c.index === at.syscall) : null;
    if (at.kind === "block") src.showFunction(at.func, s?.index, at.line, at.file);
    stack.show(at.i, null, at.task);
    note(null);
    $("where").textContent = `record ${at.i + 1} of ${time.records()} · ${taskName(at.task)} · ${at.inlined ? `${at.inlined} in ` : ""}${at.func} ${at.file ?? ""}:${at.line ?? ""}` +
      (s ? ` · in ${s.name}` : "");
    badge();
  };
  let ex = null;
  examples(a).then((e) => (a.examples = ex = e));

  // An example's stop `stop` (the `k`th of `n`) at record `i`: the cursor there, and its note.
  a.goStop = (i, stop, k, n) => {
    time.go(i);
    note(stop, k, n);
  };
  function note(stop, k, n) {
    $("note").hidden = !stop;
    if (stop) $("note").replaceChildren(el("span", { className: "dim", textContent: `Stop ${k + 1} of ${n} · ` }),
      el("b", { textContent: stop.func }), `: ${stop.note} `,
      k + 1 < n ? el("button", { className: "go", textContent: "Next stop ▸", onclick: () => ex?.next() }) : "");
  }

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
    time.mark(step.i);
    memoryAt(step.i);
    const at = m.at(step.i);
    src.view.cursor = { file: step.file, line: step.line };
    src.showFunction(step.func, undefined, step.line, step.file);
    // (Which of the lines its record runs: the stack as it runs.)
    let nth = 0;
    while (nth < k && st[k - nth - 1].i === step.i) nth++;
    stack.show(step.i, { ...step, nth }, at?.task);
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

  // ---- The loop: the console ----
  // Start the kernel again (a fresh one: the page stays); done once /init
  // is ready for calls.
  a.restart = async () => {
    halted = null;
    stopPlay();
    $("console").textContent = "";
    await m.restart();
    refresh();
    stack.show(undefined);
    badge();
  };
  function tick() {
    const out = m.console();
    if (out) {
      $("console").textContent += out.replaceAll("\r", "");
      $("pane-console").scrollTop = $("pane-console").scrollHeight;
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
  a.state = state;
  return a;
}
