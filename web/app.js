// The kSEE page: wires the panels to a booted Machine (crates/ksee-runtime).
//
// One cursor: a position in the history (the trace ring), or now. The
// history shows it among the tasks and their syscalls; the code column
// shows the syscall there (its call path) and the source at the cursor;
// the state column shows kernel objects (always live memory). The
// notebook makes syscalls; the debugger controls stop the kernel, and a
// stop is now.
import { $, taskName } from "./dom.js";
import { process } from "./ksee.js";
import { cells } from "./cells.js";
import { syscalls } from "./syscalls.js";
import { source } from "./source.js";
import { objects } from "./objects.js";
import { timeline } from "./timeline.js";

export function app(m) {
  const a = { m, proc: process(m) };
  // A syscall's identity across snapshots (indexes shift as the ring wraps).
  a.key = (s) => `${s.task.addr}:${s.start_ns}`;
  const list = syscalls(a), src = source(a), objs = objects(a), time = timeline(a);
  a.showFunction = src.showFunction;
  a.inspect = (...args) => {
    tab("objects");
    objs.inspect(...args);
  };

  // A new snapshot of the trace, everywhere.
  const refresh = () => {
    const all = list.refresh();
    time.refresh(all);
    return all;
  };
  $("show-control").onchange = refresh;

  // Select syscall `s`: its span in the history, its call path, and the
  // run of `fn` in the source.
  a.select = (s, fn, line) => {
    list.show(s, fn, line);
    time.select(a.key(s));
    nb.mark(a.key(s));
  };
  a.selectKey = (key) => {
    const s = list.byKey(key) ?? (refresh(), list.byKey(key));
    if (s) a.select(s);
    else $("where").textContent = "that syscall is no longer in the trace ring";
  };
  // The notebook made syscall `name`: the snapshot with it, selected.
  a.madeSyscall = (name) => {
    const s = refresh().findLast((s) => !s.control && s.name === name && s.task.pid === 1);
    if (s && !m.stopped()) a.select(s);
    return s;
  };
  // The cursor moved to `at` (see Machine.at), or back to now.
  a.showAt = (at) => {
    src.view.cursor = at;
    $("mem-note").textContent = "live memory, not at the cursor";
    const s = at.syscall != null ? list.byIndex(at.syscall) : null;
    if (s) {
      list.show(s, at.func, at.line);
      time.select(a.key(s), at.i);
      nb.mark(a.key(s));
    } else src.showFunction(at.func, undefined, at.line);
  };
  a.showNow = () => {
    src.view.cursor = null;
    $("mem-note").textContent = "live memory";
    const at = m.stopped();
    if (at) showStop(at);
    else if (src.view.fn) src.showFunction(src.view.fn.name, src.view.syscall);
  };
  const nb = cells(a);

  // ---- The state column's tabs ----
  let current = "objects";
  function tab(name) {
    current = name;
    for (const t of ["objects", "syscalls", "console"]) {
      $(`tab-${t}`).classList.toggle("on", t === name);
      $(`pane-${t}`).hidden = t !== name;
    }
    if (name === "console") $("console-new").textContent = "";
  }
  for (const t of ["objects", "syscalls", "console"]) $(`tab-${t}`).onclick = () => tab(t);

  // ---- Running and stopping ----
  const buttons = { pause: $("pause"), step: $("step"), next: $("next"), cont: $("cont") };
  buttons.pause.onclick = () => m.pause();
  buttons.step.onclick = () => m.step();
  buttons.next.onclick = () => m.next();
  buttons.cont.onclick = () => m.resume();
  const status = (text, stopped = false) => {
    $("status").textContent = text;
    $("status").className = stopped ? "stopped" : "dim";
  };
  function showStop(at) {
    const s = refresh().findLast((s) => s.task.addr === at.task.addr && s.duration_ns == null);
    if (s) a.select(s, at.func, at.line);
    else src.showFunction(at.func, undefined, at.line);
  }
  function onStop(at) {
    src.view.stop = at;
    src.view.cursor = null;
    status(`stopped in ${at.func} (${at.file}:${at.line}) on ${taskName(at.task)}`, true);
    showStop(at);
    objs.stopped(at.task);
  }

  let lastStop = null, lastNow = 0;
  function tick() {
    const out = m.console();
    if (out) {
      $("console").textContent += out.replaceAll("\r", "");
      $("pane-console").scrollTop = $("pane-console").scrollHeight;
      if (current !== "console") $("console-new").textContent = "•";
    }
    const code = m.halted();
    if (code !== undefined) {
      src.view.stop = null;
      status(`halted (${code})`);
      Object.values(buttons).forEach((b) => (b.disabled = true));
      refresh();
      return;
    }
    const at = m.stopped();
    // (A new stop, also after a step: each has its own number.)
    if (at && at.stop !== lastStop) onStop(at);
    if (!at && lastStop != null) src.view.stop = null;
    // Where the kernel is now (its latest block), a few times a second.
    if (!at && performance.now() - lastNow > 250) {
      lastNow = performance.now();
      const now = m.now();
      if (now) status(`running · now ${taskName(now.task)} in ${now.func} (${now.file}:${now.line})`);
    }
    lastStop = at?.stop ?? null;
    buttons.pause.disabled = !!at;
    buttons.step.disabled = buttons.next.disabled = buttons.cont.disabled = !at;
    setTimeout(tick, 30);
  }
  refresh();
  tick();
  return a;
}
