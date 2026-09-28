// The timeline of the recording. On top, an overview of all of it: which
// task ran when, where each syscall starts, the window shown below, the
// cursor. Below, the window as a
// stack chart (a flame graph, Machine.chart): each task's call stack over
// time, outermost at the bottom, each call a bar colored by where its
// function is (a syscall's entry named for the syscall); under it, a strip
// of which task runs. Each depth keeps its row: the rows fit the box, as
// thin as the deepest stack of the selection needs (so they do not move
// while zooming), and the box scrolls when deeper still.
//
// Scroll (or pinch, or W/S) to zoom; drag to pan, in time and up and down
// through deep stacks; Shift+scroll, sideways scroll or A/D to pan in time. Hover a call
// for what it was, and where it is, and the source and the Stack
// show that moment while the pointer is there; click to put the
// cursor there and select it (a syscall: its records, from entry to exit);
// double-click a call to select its records, from its first to its return. Click or
// drag the overview to move the window. Find a function (the box in the
// header, or /): Enter goes to its next call after the cursor, Shift+Enter
// to the one before; its calls are marked on the overview.
import { $, el, taskName } from "./dom.js";

const ROW = 15, MIN_ROW = 11, STRIP = 12, OVER = 18;
const css = (v) => getComputedStyle(document.body).getPropertyValue(v).trim();
// A task's color, by its pid (the idle task, 0, pale), as the strip, the
// overview and the program's lanes have it.
export const hue = (pid) => ((pid ?? 0) * 137) % 360;
const taskColor = (pid) => `hsl(${hue(pid)} 50% ${pid === 0 ? 85 : 62}%)`;
// Calls are colored by where the function is.
const AREAS = [["fs/", 210, "fs"], ["mm/", 135, "mm"], ["kernel/sched/", 28, "sched"], ["kernel/", 48, "kernel"],
               ["drivers/", 280, "drivers"], ["lib/", 175, "lib"], ["include/", 330, "include"], ["arch/", -1, "arch"]];
export const color = (file, on) => {
  const h = (AREAS.find(([p]) => file?.startsWith(p)) ?? [null, -1])[1], l = on ? 50 : 74;
  return h < 0 ? `hsl(0 0% ${l + 5}%)` : `hsl(${h} ${on ? 65 : 55}% ${l}%)`;
};

export function timeline(app) {
  const { m } = app;
  const over = $("overview"), chart = $("chart"), strip = $("strip");
  // n: records; calls: syscalls; win: the window shown { a, b }; home: the
  // selection (a syscall's records, and its task); cols: the
  // chart's columns [{ task, frames: [[func, first, inlined]] }] (see
// Machine.chart: inlined, code inlined into the call below it).
  const t = { n: 0, calls: [], win: null, home: null, where: null, title: "", cursor: null, cols: [], buckets: [],
              depth: 1, row: ROW, // (depth: the deepest stack of the selection seen; row: the rows' height)
              lit: null, sel: null, syscallAt: new Map(),
              found: { name: "", at: [] } }; // (the function found: its calls' records) // (lit, sel: the calls outlined, their first records: hovered here or in the Stack; selected)
  const { fn, iname } = app, tasks = new Map();
  const who = (addr) => (tasks.has(addr) ? tasks.get(addr) : tasks.set(addr, m.task(addr)).get(addr));

  function canvas(c, h) {
    const dpr = devicePixelRatio;
    c.style.height = `${h}px`;
    c.width = c.clientWidth * dpr;
    c.height = h * dpr;
    const ctx = c.getContext("2d");
    ctx.scale(dpr, dpr);
    ctx.font = `11px ${css("--mono")}`;
    ctx.textBaseline = "middle";
    return ctx;
  }
  const W = () => chart.clientWidth;
  // (The axis is steps, blocks run (Machine.steps; step_of, record_of
  // convert): a store takes no room, so a memset is no longer than a line,
  // and the stores between calls are not there. The window is kept in
  // both: records a, b (a selection is records), steps sa, sb.)
  const span = () => t.win.sb - t.win.sa + 1;
  const x = (i) => ((m.step_of(i) - t.win.sa) / span()) * W();
  const at = (px) => Math.max(t.win.a, Math.min(t.win.b, m.record_of(Math.round(t.win.sa + (px / W()) * span()))));

  // ---- Drawing ----
  function drawOverview() {
    const ctx = canvas(over, OVER), w = over.clientWidth;
    if (!t.n) {
      ctx.fillStyle = css("--dim");
      ctx.fillText("Nothing recorded yet: open an example and make its calls. What the kernel runs appears here.", 4, OVER / 2);
      return;
    }
    t.buckets.forEach((b, k) => {
      ctx.fillStyle = b.pid == null ? css("--line") : taskColor(b.pid);
      ctx.fillRect(k, 3, 1, OVER - 6);
    });
    const ox = (i) => (m.step_of(i) / Math.max(1, m.steps() - 1)) * w;
    // (Each syscall's start: a mark, to find the program's calls; the calls
    // of the function found: marks on top.)
    ctx.fillStyle = css("--accent");
    for (const c of t.calls) ctx.fillRect(Math.round(ox(c.first)), OVER - 4, 1, 4);
    ctx.fillStyle = css("--miss");
    for (const i of t.found.at) ctx.fillRect(Math.round(ox(i)), 0, 1, 4);
    if (t.win) {
      ctx.strokeStyle = css("--accent");
      ctx.lineWidth = 1.5;
      ctx.strokeRect(ox(t.win.a), 1, Math.max(3, ox(t.win.b) - ox(t.win.a)), OVER - 2);
    }
    if (t.cursor != null) {
      ctx.fillStyle = css("--fg");
      ctx.fillRect(ox(t.cursor) - 1, 0, 2, OVER);
    }
  }
  // A call's name; a syscall's entry (from its first record) with the
  // syscall after it, as the Stack has it: "wasm_syscall · read".
  const nameOf = (id, first) => {
    const s = t.syscallAt.get(first), name = fn(id)?.name ?? "?";
    return s ? `${name} · ${s.name ?? s.nr}` : name;
  };
  // A flame graph: the outermost calls at the bottom, over the task strip
  // (as in the Stack, the innermost is on top). Rows get thinner to fit the
  // box, down to where names still fit; deeper, the box scrolls (its bottom
  // in view at first). Row `d` (d calls deep) is at rowY(d); rowAt(y) is
  // the row at y.
  const rowY = (d) => (t.depth - 1 - d) * t.row;
  const rowAt = (y) => t.depth - 1 - Math.floor(y / t.row);
  function drawChart() {
    // (Rows only get thinner as deeper stacks show up, not while zooming.)
    const depth = (t.depth = Math.max(t.depth, ...t.cols.map((c) => c.frames.length)));
    const box = $("chartbox"), room = box.clientHeight || 190;
    t.row = Math.max(MIN_ROW, Math.min(ROW, Math.floor(room / depth)));
    const ctx = canvas(chart, Math.max(room, depth * t.row)), sctx = canvas(strip, STRIP);
    if (depth !== t.shown) box.scrollTop = box.scrollHeight;
    t.shown = depth;
    if (!t.win) return;
    const base = chart.clientHeight - depth * t.row; // (the rows sit at the bottom)
    const cw = W() / Math.max(1, t.cols.length), cur = t.cursor ?? -1;
    const curCol = t.cols[Math.min(t.cols.length - 1, Math.max(0, Math.floor(x(cur) / cw)))];
    // Runs of equal things, column to column: [from, to) with a key.
    const runs = (key) => {
      const out = [];
      t.cols.forEach((c, k) => {
        const v = key(c), last = out.at(-1);
        if (last && last.v === v && v !== undefined) last.to = k + 1;
        else out.push({ v, from: k, to: k + 1, c });
      });
      return out.filter((r) => r.v !== undefined);
    };
    // The task strip.
    for (const r of runs((c) => c.task)) {
      const p = who(r.v);
      sctx.fillStyle = taskColor(p?.pid ?? 0);
      sctx.fillRect(r.from * cw, 0, (r.to - r.from) * cw, STRIP);
      label(sctx, taskName(p), r.from * cw, (r.to - r.from) * cw, STRIP / 2, "#1f2328");
    }
    // The stacks (the calls at the cursor: darker).
    for (let d = 0; d < depth; d++) {
      for (const r of runs((c) => (c.frames[d] ? c.frames[d].join(":") : undefined))) {
        const [id, first, inl] = r.c.frames[d], f = fn(id), y = base + rowY(d);
        const x1 = r.from * cw, w = (r.to - r.from) * cw;
        const on = curCol?.frames[d]?.[1] === first && cur >= t.win.a && cur <= t.win.b;
        ctx.fillStyle = color(f?.file, on);
        ctx.fillRect(x1, y, Math.max(1, w - 1), t.row - 1);
        if (first === t.lit || first === t.sel) {
          ctx.strokeStyle = css("--fg");
          ctx.lineWidth = 1.5;
          ctx.strokeRect(x1 + 0.75, y + 0.75, Math.max(1, w - 2.5), t.row - 2.5);
        }
        label(ctx, inl ? iname(inl - 1) : nameOf(id, first), x1, w, y + t.row / 2, on ? "#fff" : "#1f2328");
      }
    }
    if (cur >= t.win.a && cur <= t.win.b) {
      ctx.fillStyle = css("--fg");
      ctx.fillRect(x(cur) - 1, 0, 2, chart.clientHeight);
      sctx.fillStyle = css("--fg");
      sctx.fillRect(x(cur) - 1, 0, 2, STRIP);
    }
    $("chart-title").textContent = t.title || "the recording";
    $("chart-verbs").replaceChildren(...(t.home ? [" · ", el("a", { textContent: "zoom", title: "the window at the selection (z)", onclick: zoomHome })] : []),
      ...(t.win && (t.win.sa > 0 || t.win.sb < m.steps() - 1) ? [" · ", el("a", { textContent: "whole", title: "the whole recording (x; double-click empty chart)", onclick: whole })] : []));
  }
  function label(ctx, text, x1, w, y, fill) {
    if (w < 24) return;
    ctx.save();
    ctx.beginPath();
    ctx.rect(x1, y - ROW / 2, w - 2, ROW);
    ctx.clip();
    ctx.fillStyle = fill;
    ctx.fillText(text, x1 + 3, y);
    ctx.restore();
  }
  function load() {
    if (!t.win) return (t.cols = []);
    const cols = Math.max(1, Math.min(W(), span()));
    const flat = m.chart(t.win.a, t.win.b, cols), out = [];
    for (let k = 0; k < flat.length;) {
      const task = flat[k], depth = flat[k + 1], frames = [];
      for (let d = 0; d < depth; d++) frames.push(flat.slice(k + 2 + 3 * d, k + 5 + 3 * d));
      out.push({ task, frames });
      k += 2 + 3 * depth;
    }
    t.cols = out;
  }
  function draw() {
    drawOverview();
    drawChart();
  }

  // ---- The window ---- (set in steps)
  function setWindow(sa, sb) {
    const n = Math.max(20, Math.round(sb - sa)), last = Math.max(0, m.steps() - 1);
    sa = Math.round(Math.max(0, Math.min(sa, last - n)));
    sb = Math.min(last, sa + n);
    t.win = { sa, sb, a: m.record_of(sa), b: m.record_of(sb) };
    load();
    draw();
  }
  // (A selection (records) with a little room either side, in steps, to
  // see where it starts and ends.)
  const pad = (w) => {
    const [sa, sb] = [m.step_of(w.a), m.step_of(w.b)];
    const p = Math.max(4, Math.round((sb - sa) * 0.02));
    return [sa - p, sb + p];
  };

  // ---- Input ----
  const pan = (px) => {
    const d = (px / W()) * (t.win.sb - t.win.sa);
    setWindow(t.win.sa + d, t.win.sb + d);
  };
  // (Zoom about step `s`.)
  const zoom = (s, amount) => {
    const k = Math.exp(Math.max(-0.5, Math.min(0.5, amount)));
    setWindow(s - (s - t.win.sa) * k, s + (t.win.sb - s) * k);
  };
  // Scrolling up and down (or a pinch): zoom; sideways, or with Shift: pan.
  chart.onwheel = (e) => {
    if (!t.win) return;
    e.preventDefault();
    if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return pan(e.deltaX);
    if (e.shiftKey) return pan(e.deltaY);
    zoom(m.step_of(at(e.offsetX)), e.deltaY * (e.ctrlKey ? 0.01 : 0.002));
  };
  // The task strip: who ran; click to put the cursor there.
  strip.onpointermove = (e) => {
    const c = t.win && t.cols[Math.floor((e.offsetX / W()) * t.cols.length)];
    strip.title = c ? taskName(who(c.task)) : "";
  };
  strip.onclick = (e) => t.win && go(m.seek(Math.max(0, at(e.offsetX) - 1), 1, "block"));
  // W/S: zoom (around the cursor, or the middle); A/D: pan.
  chart.tabIndex = 0;
  chart.onkeydown = (e) => {
    if (!t.win) return;
    const mid = t.cursor != null && t.cursor >= t.win.a && t.cursor <= t.win.b ? m.step_of(t.cursor) : (t.win.sa + t.win.sb) / 2;
    const act = { w: () => zoom(mid, -0.25), s: () => zoom(mid, 0.25), a: () => pan(-W() / 8), d: () => pan(W() / 8), z: zoomHome, x: whole }[e.key.toLowerCase()];
    if (!act) return;
    e.preventDefault();
    act();
  };
  // Hover: the call there outlined, and what it was (a syscall: its
  // arguments and result; a function: where, and its arguments).
  const tip = document.body.appendChild(Object.assign(document.createElement("div"), { className: "tip", hidden: true }));
  const args = new Map(); // first record -> the call's arguments
  // (What it returned, if it did in the recording: " = 26".)
  const retOf = (first) => {
    const end = m.call_end(first), at = end == null ? null : m.at(end);
    return at?.kind === "return" && at.value != null ? ` = ${app.kernel.result(at.value)}` : "";
  };
  const argsOf = (first) => args.get(first) ?? args.set(first, m.call_stack(first)?.find((f) => !f.inlined)?.args ?? []).get(first);
  // The call under the pointer: its column `k` and depth `d` (above a
  // column's stack: its top, the call running then; -1: none).
  function hit(e) {
    const k = Math.floor((e.offsetX / W()) * t.cols.length), c = t.cols[k];
    const d = Math.min(rowAt(e.offsetY - (chart.clientHeight - t.depth * t.row)), (c?.frames.length ?? 0) - 1);
    return { k, c, d };
  }
  function hover(e) {
    if (!t.win) return;
    const { k, c, d } = hit(e);
    const [id, first, inl] = c?.frames[d] ?? [];
    hilite(first ?? null);
    // (The view follows the pointer: the moment under it, with the call
    // under it selected: the source shows where that call was, the Stack
    // marks it. Along one bar, the view stays in that function.)
    app.peek(first == null ? at(e.offsetX) : Math.max(first, at(e.offsetX)), first == null ? null : { first, inlined: inl ? iname(inl - 1) : null });
    if (first == null) {
      tip.hidden = true;
      chart.title = c && d < 0 ? taskName(who(c.task)) : "";
      return;
    }
    chart.title = "";
    const f = fn(id), s = t.syscallAt.get(first);
    const line = (text, cls) => Object.assign(document.createElement("div"), { className: cls, textContent: text });
    const tipLines = (inl
      ? [line(iname(inl - 1), "")]
      : s
      ? [line(`${s.name ?? s.nr}(${argsOf(first).map((a) => a.value ?? "{…}").join(", ")}) = ${s.ret ?? "…"}`, ""),
         line(`a syscall by ${taskName(s.task)}`, "dim")]
      : [line(`${f?.name ?? "?"}${retOf(first)}`, ""), ...argsOf(first).map((a) => line(`${a.name} = ${a.value ?? "{…}"}`, "f"))]);
    tip.replaceChildren(...tipLines);
    tip.hidden = false;
    const r = tip.getBoundingClientRect();
    tip.style.left = `${Math.min(e.clientX + 14, innerWidth - r.width - 8)}px`;
    tip.style.top = `${e.clientY + 16 + r.height > innerHeight ? e.clientY - r.height - 8 : e.clientY + 16}px`;
  }
  chart.onpointerleave = () => {
    tip.hidden = true;
    hilite(null);
    app.peek(null);
  };
  // Mark call `first` as the selected one (null: none).
  function selected(first) {
    t.sel = first;
    if (t.win) drawChart();
  }
  // Outline the call whose first record is `first` (null: none).
  function hilite(first) {
    if (first === t.lit) return;
    t.lit = first;
    if (t.win) drawChart();
  }
  let drag = null;
  chart.onpointerdown = (e) => {
    tip.hidden = true;
    // (Released off the chart too: the drag ends.)
    try { chart.setPointerCapture(e.pointerId); } catch { /* a synthetic event */ }
    if (t.win) drag = { x: e.clientX, y: e.clientY, top: $("chartbox").scrollTop, a: t.win.sa, b: t.win.sb, moved: false };
  };
  // Drag: pan, in time (sideways) and through the stacks (up and down).
  chart.onpointermove = (e) => {
    if (!drag) return hover(e);
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (Math.abs(dx) > 3 || Math.abs(dy) > 3) drag.moved = true;
    if (drag.moved) {
      tip.hidden = true;
      $("chartbox").scrollTop = drag.top - dy;
      const per = (drag.b - drag.a) / W();
      if (dx) setWindow(drag.a - dx * per, drag.b - dx * per);
    }
  };
  chart.onpointerup = (e) => {
    const was = drag;
    drag = null;
    if (!was || was.moved) return;
    // A call: the cursor in it, and the call selected (the source shows
    // where it is then). Elsewhere: the cursor at the next block.
    const { c, d } = hit(e), [, first, inl] = c?.frames[d] ?? [];
    if (first == null) return go(m.seek(Math.max(0, at(e.offsetX) - 1), 1, "block"));
    // (A syscall: selected too, its records from its entry to its exit.)
    const s = !inl && t.syscallAt.get(first);
    if (s) select(s);
    go(Math.max(first, at(e.offsetX)));
    app.selectCall(first, inl ? iname(inl - 1) : null);
  };
  chart.ondblclick = (e) => {
    // Select the call there: its records, from its first to its return.
    // (Empty chart: the whole recording again.)
    const { c, d } = hit(e), [id, first, inl] = c?.frames[d] ?? [];
    if (first == null) return whole();
    if (inl || t.syscallAt.has(first)) return;
    selectCall(first, fn(id)?.name ?? "a call");
    go(m.seek(first, 1, "line"));
  };
  // Select the call from record `first` (called `title`): its records, from
  // its first to where it returns (found again after a snapshot, by id).
  function selectCall(first, title) {
    const id = m.record_id(first), task = t.cols.find((c) => c.frames.some((f) => f[1] === first))?.task;
    selectWindow(() => {
      const a = m.record_index(id);
      return a == null ? null : { a, b: m.call_end(a) ?? t.n - 1, task };
    }, title);
  }
  let overDrag = false;
  const overMove = (e) => {
    if (!t.win) return;
    const s = Math.round((e.offsetX / over.clientWidth) * (m.steps() - 1)), n = t.win.sb - t.win.sa;
    setWindow(s - n / 2, s + n / 2);
  };
  over.onpointerdown = (e) => {
    overDrag = true;
    try { over.setPointerCapture(e.pointerId); } catch { /* a synthetic event */ }
    overMove(e);
  };
  over.onpointermove = (e) => overDrag && overMove(e);
  over.onpointerup = () => (overDrag = false);
  addEventListener("resize", () => {
    t.buckets = t.n ? m.timeline(Math.max(1, over.clientWidth)) : [];
    if (t.win) (load(), draw());
  });

  // ---- The API ----
  // A fresh kernel: no selection, no cursor.
  function clear() {
    Object.assign(t, { where: null, home: null, title: "", cursor: null, sel: null, lit: null });
  }
  // After a snapshot: the new extent; the window at the selection (or all).
  function refresh(calls) {
    t.n = m.records();
    t.calls = calls;
    t.syscallAt = new Map(calls.map((c) => [c.first, c]));
    tasks.clear();
    args.clear();
    t.buckets = m.timeline(Math.max(1, over.clientWidth));
    if (t.found.name) t.found.at = [...m.calls_of(t.found.name, 0, Math.max(0, t.n - 1))];
    t.home = t.where?.() ?? null;
    t.depth = 1;
    if (t.cursor != null && t.cursor >= t.n) t.cursor = null;
    // (A new recording: the whole of it; one grown: the window as it was.)
    if (!t.n) t.win = null;
    else if (t.win && t.win.sb <= m.steps() - 1) setWindow(t.win.sa, t.win.sb);
    else setWindow(0, m.steps() - 1);
    draw();
  }
  // Select records `where()` gives ({ a, b, task }: asked again after each
  // snapshot, as records move), called `title`: the window at them.
  // The window stays as it is (the whole recording by default: zooming is
  // asked for, `z` or "zoom"), unless the selection is not in it.
  function selectWindow(where, title) {
    t.depth = 1;
    t.where = where;
    t.title = title;
    t.home = where();
    if (t.home && t.win && (t.home.a < t.win.a || t.home.b > t.win.b)) whole();
    else draw();
  }
  // The window at the selection, and at the whole recording.
  const zoomHome = () => t.home && setWindow(...pad(t.home));
  const whole = () => t.n && setWindow(0, m.steps() - 1);
  // Select syscall `s` (found again in later snapshots by its task and
  // its first record's id: indexes move as the ring goes on).
  function select(s) {
    selectWindow(() => {
      // (Gone from the recording: no selection.)
      const c = t.calls.find((c) => c.id === s.id);
      return c ? { a: c.first, b: c.last ?? t.n - 1, task: c.task.addr } : null;
    }, `${s.name ?? s.nr} by ${taskName(s.task)}`);
  }
  // ---- Finding a function ----
  // Find function `name`: its calls marked; `dir` 1: go to the next one
  // after the cursor, -1: the one before (none: says so).
  function find(name, dir) {
    if (name !== t.found.name) t.found = { name, at: name && t.n ? [...m.calls_of(name, 0, t.n - 1)] : [] };
    const at = t.found.at, cur = t.cursor ?? -1;
    const i = dir > 0 ? at.find((x) => x > cur) : at.findLast((x) => x < cur);
    drawOverview();
    if (i == null) {
      finder.hint.textContent = !name ? "" : !at.length ? (m.definition(name) ? `${name} is not called in the recording` : `no function ${name}`)
        : `no call of ${name} ${dir > 0 ? "after" : "before"} the cursor (${at.length} in the recording)`;
      return false;
    }
    finder.hint.textContent = `${at.indexOf(i) + 1} of ${at.length}`;
    app.goCall(i);
    return true;
  }
  // (The box, in the header, before the legend.)
  const finder = { input: el("input", { className: "find mono", placeholder: "find a function…", spellcheck: false,
                     title: "a function: Enter, its next call after the cursor; Shift+Enter, the one before; Esc clears" }),
                   hint: el("span", { className: "dim findhint" }) };
  finder.input.onkeydown = (e) => {
    if (e.key === "Enter") find(finder.input.value.trim(), e.shiftKey ? -1 : 1);
    else if (e.key === "Escape") ((finder.input.value = ""), find("", 1), finder.input.blur());
    else return;
    e.preventDefault();
  };
  $("legend").before(finder.input, finder.hint);

  // Put the cursor at record `i`, and show it.
  function go(i) {
    mark(i);
    const a = t.cursor != null ? m.at(t.cursor) : null;
    if (a) app.showAt(a);
  }
  // Put the cursor at record `i` without showing it (the caller shows the
  // line there); the window follows it: out past an edge, it slides so
  // that the cursor is a third of the way in from that edge, with the
  // records ahead in view (as a source view pages).
  function mark(i) {
    if (!t.n) return;
    t.sel = null;
    const was = t.cursor;
    t.cursor = Math.max(0, Math.min(t.n - 1, i));
    if (t.win && (t.cursor < t.win.a || t.cursor > t.win.b)) {
      const n = t.win.sb - t.win.sa, forward = was == null || t.cursor > was, s = m.step_of(t.cursor);
      const a = forward ? s - n / 3 : s - (2 * n) / 3;
      setWindow(a, a + n);
    } else draw();
  }
  // The legend.
  $("legend").replaceChildren(...AREAS.map(([p, , name]) => {
    const k = document.createElement("span");
    k.className = "key";
    k.innerHTML = `<i style="background:${color(p, false)}"></i>${name}`;
    return k;
  }));
  // Where the box of the call from record `first` is, on the page: its
  // middle ({ x, y }, client coordinates), or null if it is not shown.
  function boxOf(first) {
    const r = chart.getBoundingClientRect(), cw = W() / Math.max(1, t.cols.length);
    const base = chart.clientHeight - t.depth * t.row;
    for (let k = 0; k < t.cols.length; k++) {
      const d = t.cols[k].frames.findIndex((f) => f[1] === first && !f[2]);
      if (d >= 0) return { x: r.left + (k + 0.5) * cw, y: r.top + base + rowY(d) + t.row / 2 };
    }
    return null;
  }
  return { refresh, clear, go, mark, select, selectCall, hilite, selected, boxOf, focusFind: () => finder.input.focus(),
           home: () => t.home, cursor: () => t.cursor, records: () => t.n };
}
