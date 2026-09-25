// The kSEE page: wires the panels to a booted Machine (crates/ksee-runtime).
//
// The kernel is live: it runs on its own, and the history (the trace ring)
// grows. The page is in one of three states:
//   LIVE     running; the history follows now
//   PAUSED   the debugger stopped the kernel (Pause, a breakpoint, Step
//            into); now is the stop, and objects are exact
//   REPLAY   the cursor is in the history (a picked moment, or playing a
//            syscall back); the kernel goes on meanwhile
// One cursor: the history shows it among the tasks and the calls; the call
// stack and the source show the code there. Play moves it line by line:
// through the recorded trace in REPLAY, or by running the kernel, a line at
// a time, when PAUSED.
import { $, el, taskName } from "./dom.js";
import { process } from "./ksee.js";
import { cards } from "./cards.js";
import { syscalls } from "./syscalls.js";
import { source } from "./source.js";
import { objects } from "./objects.js";
import { timeline } from "./timeline.js";
import { callstack } from "./callstack.js";
import { tours } from "./tours.js";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function app(m) {
  const a = { m, proc: process(m) };
  // A syscall's identity across snapshots (indexes shift as the ring wraps).
  a.key = (s) => `${s.task.addr}:${s.start_ns}`;
  const list = syscalls(a), src = source(a), objs = objects(a), time = timeline(a), stack = callstack(a);
  a.showFunction = src.showFunction;
  a.inspect = (...args) => {
    tab("objects");
    objs.inspect(...args);
  };
  a.showBytes = (name, object) => {
    tab("objects");
    objs.inspect(name, null, 0, true, object);
  };

  // ---- The state ----
  let halted = null;
  const state = () => (halted != null ? "halted" : time.cursor() != null ? "replay" : m.stopped() ? "paused" : "live");
  function badge() {
    const st = state(), at = m.stopped();
    $("badge").className = `badge ${st}`;
    $("badge").textContent = { live: "● LIVE", paused: "❚❚ PAUSED", replay: "↺ REPLAY", halted: "■ HALTED" }[st];
    $("pause").disabled = st === "halted" || !!at;
    $("cont").disabled = !at;
    $("over").disabled = !(at && st === "paused");
    $("mem-note").textContent = st === "replay" ? "live memory, not at the cursor" : "live memory";
    if (st === "paused") status(`in ${at.func} (${at.file}:${at.line}) on ${taskName(at.task)}`);
    if (st === "halted") status(`the kernel halted (${halted})`);
    if (st === "replay") status(at ? "the recorded past; the kernel is paused at now" : "the recorded past; the kernel runs on");
  }
  const status = (text) => ($("status").textContent = text);

  // A new snapshot of the trace, everywhere (the cursor back at now).
  const refresh = () => {
    const all = list.refresh();
    time.refresh(all);
    return all;
  };
  $("show-control").onchange = refresh;
  a.refresh = refresh;

  // ---- Selecting and moving the cursor ----

  // Select syscall `s`: its span, its calls; the cursor at its first line
  // past the entry (unless `stay`: the kernel is stopped in it).
  a.select = (s, stay = false) => {
    list.mark(a.key(s));
    nb.mark(a.key(s));
    time.select(s);
    if (stay) return;
    let i = s.first;
    for (let k = 0; k < 4; k++) {
      const at = m.at(i);
      if (at?.kind === "block" && at.func !== "wasm_syscall" && !at.func.startsWith("__wasm_sys_")) break;
      i = m.seek(i, 1, "function");
    }
    time.go(i);
  };
  // Select the run of program `pid` (Machine.program): its calls; the
  // cursor at its start.
  a.selectProgram = (pid, title) => {
    const where = () => {
      const p = m.program(pid);
      return p && { a: p.first, b: p.last, task: p.task };
    };
    const w = where();
    if (!w) return;
    time.selectWindow(where, title);
    list.mark(null);
    nb.mark(null);
    time.go(m.seek(w.a, 1, "line"));
  };
  // The first syscall of task `task` after record `i`.
  a.syscallAfter = (task, i) => list.all().find((s) => s.task.addr === task && s.first > i);
  a.selectKey = (key) => {
    const s = list.byKey(key) ?? (refresh(), list.byKey(key));
    if (s) a.select(s);
    else $("where").textContent = "that syscall is no longer in the trace ring";
  };
  // A card made syscall `name`: the snapshot with it, selected.
  a.madeSyscall = (name) => {
    if (m.stopped()) return null;
    const s = refresh().findLast((s) => !s.control && s.name === name && s.task.pid === 1);
    if (s) a.select(s);
    return s;
  };
  // The cursor moved to `at` (see Machine.at).
  a.showAt = (at) => {
    L.pos = null;
    src.view.cursor = at;
    const s = at.syscall != null ? list.byIndex(at.syscall) : null;
    if (at.kind === "block") src.showFunction(at.func, s?.index, at.line, at.file);
    stack.show(at);
    tour?.follow(at);
    note(null);
    $("where").textContent = `record ${at.i + 1} of ${time.records()} · ${taskName(at.task)} · ${at.func} ${at.file ?? ""}:${at.line ?? ""}` +
      (s ? ` · in ${s.name}` : "");
    badge();
  };
  // Back at now: the stop, or the kernel running.
  a.showNow = () => {
    L.pos = null;
    src.view.cursor = null;
    const at = m.stopped();
    if (at) showStop(at);
    else stack.show(null);
    $("where").textContent = "";
    badge();
  };
  const nb = cards(a);
  let tour = null;
  tours(a).then((t) => (tour = t));

  // A tour's note for a stop (null: none).
  function note(stop, k, n) {
    $("note").hidden = !stop;
    if (stop) $("note").replaceChildren(el("span", { className: "dim", textContent: `Stop ${k + 1} of ${n} · ` }),
      el("b", { textContent: stop.func }), `: ${stop.note} `,
      el("span", { className: "dim", textContent: "(Continue: to the next stop; ▶| or Play: line by line)" }));
  }

  // ---- Stopping ----
  let oneShot = null; // the handler "Step into" breaks at
  a.stepInto = (name) => {
    oneShot = `__wasm_sys_${name}`;
    m.break_function(oneShot, true);
  };
  function showStop(at) {
    src.view.stop = at;
    src.showFunction(at.func, undefined, at.line, at.file);
    stack.show(null, at);
  }
  // A new stop (`quiet`: while stepping, which shows the line itself).
  function onStop(at, quiet = false) {
    if (oneShot) {
      m.break_function(oneShot, false);
      oneShot = null;
    }
    // A tour's stops are for its program's syscalls (not, say, its exec,
    // which reads the program's file), unless one says any task; and only
    // on the program's lines the tour marks.
    const stops = a.tour?.stops ?? [];
    const k = stops.findIndex((s) => s.func === at.func);
    if (k >= 0) {
      // (The program may stop before the page knows its pid: not init's, then.)
      const pid = a.tour.pid ?? (at.task.pid > 1 ? at.task.pid : null);
      const inSyscall = m.call_stack().some((f) => f.func === "wasm_syscall");
      let skip = !stops[k].any && (pid == null || at.task.pid !== pid || !inSyscall);
      if (!skip && a.tour.lines?.length) {
        refresh();
        const line = pid != null ? m.program(pid)?.lines.at(-1)?.line : null;
        skip = !a.tour.lines.includes(line);
      }
      if (skip) {
        m.resume();
        return;
      }
    }
    const s = refresh().findLast((s) => s.task.addr === at.task.addr && s.duration_ns == null);
    if (s) a.select(s, true);
    stack.stopped();
    objs.stopped(at.task);
    note(k >= 0 ? stops[k] : null, k, stops.length);
    tour?.follow(null, at);
    if (!quiet) {
      showStop(at);
      // The stop's first line, among the syscall's.
      const st = steps(), first = st.findIndex((x) => x.i >= time.records() - 1);
      if (first >= 0) showStep(first);
    }
    badge();
  }
  $("pause").onclick = () => m.pause();
  $("cont").onclick = () => {
    note(null);
    stopPlay();
    src.view.stop = null;
    m.resume();
  };

  // ---- Lines: the line steps of the selected syscall or program ----
  // (Machine.line_steps: every source line it ran, within blocks too.)
  const L = { key: null, steps: [], pos: null };
  function steps() {
    const w = time.home();
    if (!w) return (L.steps = []);
    // (A new snapshot moves the indexes: the newest record's id tells it.)
    const key = `${w.a}:${w.b}:${w.task}:${m.record_id(time.records() - 1)}`;
    if (key !== L.key) {
      L.key = key;
      L.steps = m.line_steps(w.a, w.b ?? time.records() - 1, w.task);
      L.pos = null;
    }
    return L.steps;
  }
  // Where in the steps the cursor is (the first step at or after it).
  function position() {
    const st = steps();
    if (L.pos != null && L.pos < st.length) return L.pos;
    const cur = time.cursor() ?? time.records() - 1;
    const k = st.findIndex((x) => x.i >= cur);
    return (L.pos = k < 0 ? st.length - 1 : k);
  }
  // Show step `k`: the cursor at its record, its line in the source.
  function showStep(k) {
    const st = steps(), step = st[k];
    if (!step) return false;
    L.pos = k;
    time.mark(step.i);
    const at = m.at(step.i), stop = m.stopped();
    src.view.cursor = time.cursor() == null ? null : { file: step.file, line: step.line };
    src.showFunction(step.func, time.selected()?.index, step.line, step.file);
    if (time.cursor() == null && stop) stack.show(null, stop, step);
    else stack.show(at, null, step);
    tour?.follow(at);
    $("where").textContent = `line ${k + 1} of ${st.length} · ${step.inlined ? `${step.inlined} in ` : ""}${step.func} ${step.file}:${step.line}` +
      (at ? ` · record ${step.i + 1} · ${taskName(at.task)}` : "");
    badge();
    return true;
  }

  // Run the paused kernel on its task until there is a line more (or, with
  // `over`, to the next block of the same function). False if it went on
  // running instead (the syscall returned, and the task sleeps).
  let stepping = false;
  async function runStep(over = false) {
    let at = m.stopped();
    if (!at || stepping) return false;
    stepping = true;
    // (The last line's record, by id: indexes move with each snapshot.)
    const before = steps().length ? m.record_id(steps().at(-1).i) : null;
    try {
      for (let k = 0; k < 2000; k++) {
        const last = at.stop;
        over ? m.next() : m.step_task();
        at = null;
        for (let w = 0; w < 200 && !(at = m.stopped()); w++) await sleep(5);
        if (!at || at.stop === last) {
          m.resume(); // gone on (it would stop in the next syscall)
          return false;
        }
        lastStop = at.stop;
        onStop(at, true);
        const after = before == null ? -1 : m.index_of(before);
        const st = steps(), next = st.findIndex((x) => x.i > after);
        if (next >= 0) return showStep(next);
      }
      return false;
    } finally {
      stepping = false;
    }
  }

  // ---- The transport: line by line, and Play ----
  async function forward() {
    const st = steps();
    if (!st.length) return m.stopped() ? runStep() : false;
    const k = position();
    if (k + 1 < st.length) return showStep(k + 1);
    // The last line recorded: go on in the kernel if it is paused there.
    return m.stopped() ? runStep() : false;
  }
  function back() {
    const st = steps();
    if (!st.length) return time.go(m.seek(time.cursor() ?? time.records() - 1, -1, "line"));
    showStep(Math.max(0, position() - 1));
  }
  let playing = null;
  function stopPlay() {
    clearTimeout(playing);
    playing = null;
    $("play").textContent = "▶ Play";
  }
  function play() {
    if (playing) return stopPlay();
    if (state() === "live") {
      if (!time.home()) return status("pick a syscall to play (a card's result, a bar in the history, a tour's run), or pause the kernel");
      showStep(0);
    }
    $("play").textContent = "❚❚ Pause";
    const tickPlay = async () => {
      if (!playing) return;
      if (!(await forward())) return stopPlay();
      playing = setTimeout(tickPlay, Number($("speed").value));
    };
    playing = setTimeout(tickPlay, 0);
  }
  $("play").onclick = play;
  $("fwd-line").onclick = () => forward();
  $("back-line").onclick = back;
  $("over").onclick = () => runStep(true);
  $("to-start").onclick = () => steps().length && showStep(0);
  $("now").onclick = () => {
    stopPlay();
    time.go(null);
  };
  addEventListener("keydown", (e) => {
    if (e.target.closest("input, select, textarea")) return;
    if (e.key === "ArrowRight") forward();
    else if (e.key === "ArrowLeft") back();
    else if (e.key === " ") play();
    else return;
    e.preventDefault();
  });

  // ---- The state column's tabs ----
  let current = "stack";
  function tab(name) {
    current = name;
    for (const t of ["stack", "objects", "syscalls", "console"]) {
      $(`tab-${t}`).classList.toggle("on", t === name);
      $(`pane-${t}`).hidden = t !== name;
    }
    if (name === "console") $("console-new").textContent = "";
  }
  for (const t of ["stack", "objects", "syscalls", "console"]) $(`tab-${t}`).onclick = () => tab(t);
  for (const t of ["tours", "play"])
    $(`tab-${t}`).onclick = () => ["tours", "play"].forEach((u) => {
      $(`tab-${u}`).classList.toggle("on", u === t);
      $(`pane-${u}`).hidden = u !== t;
    });

  // ---- The loop: console, stops, and (live) the history growing ----
  let lastStop = null, lastNow = 0, lastRefresh = 0;
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
      src.view.stop = null;
      refresh();
      badge();
      return;
    }
    const at = m.stopped();
    // (A new stop, also after a step: each has its own number.)
    if (!stepping) {
      if (at && at.stop !== lastStop) onStop(at);
      if (!at && lastStop != null) src.view.stop = null;
      lastStop = at?.stop ?? null;
    }
    const st = state(), t = performance.now();
    if (st === "live" && t - lastNow > 250) {
      lastNow = t;
      const now = m.now();
      if (now) status(`now ${taskName(now.task)} in ${now.func} (${now.file}:${now.line})`);
      // The history grows (not while a card waits for its result).
      if (t - lastRefresh > 1500 && !document.hidden) {
        lastRefresh = t;
        refresh();
      }
    }
    badge();
    setTimeout(tick, 30);
  }
  refresh();
  stack.show(null);
  tick();
  a.state = state;
  return a;
}
