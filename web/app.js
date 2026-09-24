// The kSEE page: wires the panels to a booted Machine (crates/ksee-runtime).
import { $, taskName } from "./dom.js";
import { composer } from "./composer.js";
import { syscalls } from "./syscalls.js";
import { source } from "./source.js";
import { objects } from "./objects.js";
import { timeline } from "./timeline.js";

export function app(m) {
  const a = { m };
  const list = syscalls(a), src = source(a), objs = objects(a), time = timeline(a);
  a.showFunction = src.showFunction;
  a.inspect = objs.inspect;

  // A new snapshot of the trace, everywhere.
  const refresh = () => {
    const all = list.refresh();
    time.refresh();
    return all;
  };
  // After a call: show its syscall (the kernel may be stopped in it).
  let timer;
  a.afterCall = (name) => {
    clearTimeout(timer);
    timer = setTimeout(() => {
      if (m.stopped()) return;
      const done = refresh().filter((s) => !s.control && s.duration_ns != null);
      const s = done.findLast((s) => s.name === name) ?? done.at(-1);
      if (s) list.show(s);
    }, 50);
  };
  // The history cursor moved to `at` (see Machine.at).
  a.showAt = (at) => {
    src.view.cursor = at;
    const s = at.syscall != null ? list.byIndex(at.syscall) : null;
    if (s) list.show(s, at.func, at.line);
    else if (at.file) src.showFile(at.file, at.line);
  };
  composer(a);

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
  function onStop(at) {
    src.view.stop = at;
    src.view.cursor = null;
    status(`stopped in ${at.func} (${at.file}:${at.line}) on ${taskName(at.task)}`, true);
    const s = refresh().findLast((s) => s.task.addr === at.task.addr && s.duration_ns == null);
    if (s) list.show(s, at.func, at.line);
    else src.showFunction(at.func, undefined, at.line);
    objs.stopped(at.task);
  }

  let wasStopped = false, lastNow = 0;
  function tick() {
    const out = m.console();
    if (out) {
      $("console").textContent += out.replaceAll("\r", "");
      $("console").parentElement.scrollTop = $("console").parentElement.scrollHeight;
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
    if (at && !wasStopped) onStop(at);
    if (!at && wasStopped) src.view.stop = null;
    // Where the kernel is now (its latest block), a few times a second.
    if (!at && performance.now() - lastNow > 250) {
      lastNow = performance.now();
      const now = m.now();
      if (now) status(`running · now ${taskName(now.task)} in ${now.func} (${now.file}:${now.line})`);
    }
    wasStopped = !!at;
    buttons.pause.disabled = !!at;
    buttons.step.disabled = buttons.next.disabled = buttons.cont.disabled = !at;
    setTimeout(tick, 30);
  }
  refresh();
  tick();
}
