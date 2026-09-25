// The objects view: a kernel object as a table of fields (nested ones
// expand in place), pointers to follow (breadcrumbs lead back), a filter,
// and the fields that changed since the last stop highlighted.
import { $, el, hex } from "./dom.js";

export function objects(app) {
  const { m } = app;
  // stack: what is shown, as breadcrumbs: [{ title, type (name or id), addr }]
  const o = { stack: [], open: new Set(), filter: "", last: new Map(), now: new Map() };

  // `object`: a value already decoded (a syscall's output, not in kernel memory).
  function inspect(title, type, addr, reset = true, object = null) {
    if (reset) o.stack = [];
    o.stack.push({ title, type, addr, object });
    o.open = new Set();
    draw();
  }
  const summary = (children) => "{ " + children.filter((c) => c.value != null).slice(0, 3)
    .map((c) => `${c.name}=${c.value}`).join(", ") + (children.length > 3 ? ", … }" : " }");

  function draw() {
    const top = o.stack.at(-1);
    if (!top) return;
    $("crumbs").replaceChildren(...o.stack.flatMap((c, i) => [i ? " › " : "", el("a", { textContent: c.title, onclick: () => {
      o.stack = o.stack.slice(0, i + 1);
      draw();
    } })]));
    const root = top.object ?? (typeof top.type === "number" ? m.object_by_id("", top.type, top.addr, 2) : m.object(top.type, top.addr, 2));
    if (!root) return $("objtable").replaceChildren(el("div", { className: "empty", textContent: `${top.type}: no such type` }));
    o.now = new Map();
    const rows = [], f = o.filter.toLowerCase();
    const walk = (children, path, depth) => {
      for (const c of children ?? []) {
        const p = `${path}.${c.name}`;
        const expandable = c.children || c.expand != null;
        const open = expandable && (o.open.has(p) || (f && depth < 3));
        let kids = c.children;
        if (open && !kids && c.expand != null && !top.object) kids = m.object_by_id(c.name, c.expand, c.addr, 1).children;
        const value = c.value ?? (kids ? summary(kids) : "{…}");
        o.now.set(p, value);
        const changed = o.last.has(p) && o.last.get(p) !== value;
        if (!f || c.name.toLowerCase().includes(f) || open)
          rows.push(el("tr", { className: changed ? "changed" : "" },
            el("td", { className: "fname", style: `padding-left:${8 + depth * 14}px` },
              el(expandable ? "a" : "span", { className: "tw", textContent: expandable ? (open ? "▾" : "▸") : "", onclick: () => {
                o.open.has(p) ? o.open.delete(p) : o.open.add(p);
                draw();
              } }), c.name),
            el("td", { className: "fval", title: value }, value, c.target != null
              ? el("a", { className: "follow", textContent: " →", title: "follow the pointer", onclick: () => inspect(`*${c.name}`, c.target, Number(c.value), false) })
              : null),
            el("td", { className: "ftype", textContent: c.ty })));
        if (open) walk(kids, p, depth + 1);
      }
    };
    walk(root.children, "", 0);
    $("objtable").replaceChildren(el("div", { className: "objhead mono", textContent: top.object ? `${root.ty}, as the syscall returned it` : `${root.ty} at ${hex(top.addr)}` }),
      el("table", { className: "objs" }, el("tbody", {}, ...rows)));
  }

  $("objfilter").oninput = () => {
    o.filter = $("objfilter").value.trim();
    draw();
  };
  $("globals").append(...m.globals().map((g) => el("option", { value: g })));
  const global = (name) => {
    const g = m.global(name, 0);
    if (g) inspect(name, g.expand ?? g.ty, g.addr);
  };
  $("globalf").onsubmit = (e) => {
    e.preventDefault();
    global($("global").value.trim());
  };
  $("decode").onsubmit = (e) => {
    e.preventDefault();
    inspect(`(${$("type").value}) ${$("addr").value}`, $("type").value.trim(), Number($("addr").value));
  };
  $("pick-task").onclick = () => {
    const t = m.stopped()?.task ?? m.now()?.task;
    if (t) inspect(`current: ${t.comm}`, "task_struct", t.addr);
  };
  $("pick-rq").onclick = () => global("runqueues");
  $("pick-init").onclick = () => global("init_task");

  // At a stop: the values shown before become the ones to compare with.
  function stopped(task) {
    o.last = o.now;
    if (o.stack.length) draw();
    else inspect(`current: ${task.comm}`, "task_struct", task.addr);
  }
  return { inspect, stopped };
}
