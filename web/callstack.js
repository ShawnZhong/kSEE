// The call stack at the cursor, innermost first. Stopped at now, it is the
// kernel's own (the plugin's frames), with each call's arguments, decoded
// by their types (→ opens a pointer's object); in the history, it is
// rebuilt from the trace (names only). Click a call for its source.
import { $, el } from "./dom.js";

export function callstack(app) {
  const { m } = app;
  const row = (func, args, onclick, on) => el("div", { className: "row frame" + (on ? " on" : ""), onclick },
    el("span", { className: "mono", textContent: func }),
    args ? el("span", { className: "args mono" }, "(", ...args.flatMap((a, k) => [k ? ", " : "",
      el("span", { className: "dim", textContent: `${a.name}=` }),
      a.target != null && a.value !== "NULL"
        ? el("a", { textContent: `${a.value}→`, title: `${a.ty}: open it`, onclick: (e) => {
            e.stopPropagation();
            app.inspect(`${func}: ${a.name}`, a.target, Number(a.value));
          } })
        : el("span", { textContent: a.value ?? "{…}", title: a.ty })]), ")") : null);

  // At a stop (the kernel's frames) or at record `at` (rebuilt).
  function show(at, stopped) {
    if (stopped) {
      const frames = m.call_stack();
      $("stack").replaceChildren(...frames.map((f, k) => row(f.func, f.args, () =>
        k ? app.showFunction(f.func) : app.showFunction(f.func, undefined, stopped.line), k === 0)));
      $("stack-note").textContent = "the kernel's, with arguments";
      return;
    }
    if (at?.i == null) {
      $("stack").replaceChildren(el("div", { className: "empty", textContent: "Pause the kernel, or pick a moment in the history." }));
      $("stack-note").textContent = "";
      return;
    }
    const names = m.stack_at(at.i).reverse();
    $("stack").replaceChildren(...names.map((f, k) => row(f, null, () => app.showFunction(f, undefined, k ? undefined : at.line), k === 0)));
    $("stack-note").textContent = "from the trace";
  }
  return { show };
}
