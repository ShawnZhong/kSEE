// The timeline of the recording. On top, an overview of all of it: which
// task ran when, the window shown below, the cursor. Below, the window as a
// stack chart (a flame graph, Machine.chart): each task's call stack over
// time, outermost at the bottom, each call a bar colored by where its
// function is (a syscall's entry named for the syscall); under it, a strip
// of which task runs. Each depth keeps its row: the rows fit the box, as
// thin as the deepest stack of the selection needs (so they do not move
// while zooming), and the box scrolls when deeper still.
//
// Scroll (or pinch, or W/S) to zoom; drag to pan, in time and up and down
// through deep stacks; Shift+scroll, sideways scroll or A/D to pan in time. Hover a call
// for what it was (the source previews where it is); click to put the
// cursor there and select it (a syscall: its records, from entry to exit);
// double-click a call to select its records, from its first to its return. Click or
// drag the overview to move the window.
import { $, taskName } from "./dom.js";

const ROW = 15, MIN_ROW = 11, STRIP = 12, OVER = 18;
const css = (v) => getComputedStyle(document.body).getPropertyValue(v).trim();
const hue = (pid) => (pid * 137) % 360;
// Calls are colored by where the function is.
const AREAS = [["fs/", 210, "fs"], ["mm/", 135, "mm"], ["kernel/sched/", 28, "sched"], ["kernel/", 48, "kernel"],
               ["drivers/", 280, "drivers"], ["lib/", 175, "lib"], ["include/", 330, "include"], ["arch/", -1, "arch"]];
// (Code inlined into a function: its color, lighter.)
const color = (file, on, inlined) => {
  const h = (AREAS.find(([p]) => file?.startsWith(p)) ?? [null, -1])[1], l = (on ? 50 : 74) + (inlined ? 12 : 0);
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
              lit: null, sel: null, inlined: false, syscallAt: new Map() };
  try { t.inlined = localStorage.getItem("ksee.inlined") === "1"; } catch { /* not kept */ } // the calls outlined (their first records): hovered here or in the Stack; selected
  const funcs = new Map(), tasks = new Map();
  const fn = (id) => (funcs.has(id) ? funcs.get(id) : funcs.set(id, m.function(id)).get(id));
  const inames = new Map();
  const iname = (k) => inames.get(k) ?? inames.set(k, m.inline_name(k)).get(k);
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
  const x = (i) => ((i - t.win.a) / (t.win.b - t.win.a + 1)) * W();
  const at = (px) => Math.max(0, Math.min(t.n - 1, Math.round(t.win.a + (px / W()) * (t.win.b - t.win.a + 1))));

  // ---- Drawing ----
  function drawOverview() {
    const ctx = canvas(over, OVER), w = over.clientWidth;
    if (!t.n) {
      ctx.fillStyle = css("--dim");
      ctx.fillText("Nothing recorded yet: open an example and make its calls. What the kernel runs appears here.", 4, OVER / 2);
      return;
    }
    t.buckets.forEach((b, k) => {
      ctx.fillStyle = b.pid == null ? css("--line") : `hsl(${hue(b.pid)} 50% ${b.pid === 0 ? 85 : 62}%)`;
      ctx.fillRect(k, 3, 1, OVER - 6);
    });
    const ox = (i) => (i / Math.max(1, t.n - 1)) * w;
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
  // A call's name (a syscall's entry, from its first record: the syscall's).
  const nameOf = (id, first) => {
    const s = t.syscallAt.get(first);
    return s ? `${s.name ?? s.nr}()` : fn(id)?.name ?? "?";
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
      const p = who(r.v, t.win.a);
      sctx.fillStyle = `hsl(${hue(p?.pid ?? 0)} 50% ${p?.pid === 0 ? 85 : 62}%)`;
      sctx.fillRect(r.from * cw, 0, (r.to - r.from) * cw, STRIP);
      label(sctx, taskName(p), r.from * cw, (r.to - r.from) * cw, STRIP / 2, "#1f2328");
    }
    // The stacks (the calls at the cursor: darker).
    for (let d = 0; d < depth; d++) {
      for (const r of runs((c) => (c.frames[d] ? c.frames[d].join(":") : undefined))) {
        const [id, first, inl] = r.c.frames[d], f = fn(id), y = base + rowY(d);
        const x1 = r.from * cw, w = (r.to - r.from) * cw;
        const on = curCol?.frames[d]?.[1] === first && cur >= t.win.a && cur <= t.win.b;
        ctx.fillStyle = color(f?.file, on, inl);
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
    $("chart-title").textContent = `${t.title || "the recording"} · records ${t.win.a + 1}–${t.win.b + 1} of ${t.n}`;
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
    const cols = Math.max(1, Math.min(W(), t.win.b - t.win.a + 1));
    const flat = m.chart(t.win.a, t.win.b, cols), out = [];
    for (let k = 0; k < flat.length;) {
      const task = flat[k], depth = flat[k + 1], frames = [];
      for (let d = 0; d < depth; d++) frames.push(flat.slice(k + 2 + 3 * d, k + 5 + 3 * d));
      // (all: with the code inlined into each call, shown if asked for.)
      out.push({ task, all: frames, frames: t.inlined ? frames : frames.filter((f) => !f[2]) });
      k += 2 + 3 * depth;
    }
    t.cols = out;
  }
  function draw() {
    drawOverview();
    drawChart();
  }

  // ---- The window ----
  function setWindow(a, b) {
    const span = Math.max(20, Math.round(b - a));
    a = Math.round(Math.max(0, Math.min(a, t.n - 1 - span)));
    t.win = { a, b: Math.min(t.n - 1, a + span) };
    load();
    draw();
  }
  // (A selection with a little room either side, to see where it starts and ends.)
  const pad = (w) => {
    const p = Math.max(4, Math.round((w.b - w.a) * 0.02));
    return [w.a - p, w.b + p];
  };

  // ---- Input ----
  const pan = (px) => {
    const d = (px / W()) * (t.win.b - t.win.a);
    setWindow(t.win.a + d, t.win.b + d);
  };
  const zoom = (at_, amount) => {
    const k = Math.exp(Math.max(-0.5, Math.min(0.5, amount)));
    setWindow(at_ - (at_ - t.win.a) * k, at_ + (t.win.b - at_) * k);
  };
  // Scroll or pinch (ctrl+wheel): zoom; sideways or Shift+scroll: pan.
  // Scrolling up and down (or a pinch): zoom; sideways, or with Shift: pan.
  chart.onwheel = (e) => {
    if (!t.win) return;
    e.preventDefault();
    if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return pan(e.deltaX);
    if (e.shiftKey) return pan(e.deltaY);
    zoom(at(e.offsetX), e.deltaY * (e.ctrlKey ? 0.01 : 0.002));
  };
  // The task strip: who ran; click to put the cursor there.
  strip.onpointermove = (e) => {
    const c = t.win && t.cols[Math.floor((e.offsetX / W()) * t.cols.length)];
    strip.title = c ? taskName(who(c.task, t.win.a)) : "";
  };
  strip.onclick = (e) => t.win && go(m.seek(Math.max(0, at(e.offsetX) - 1), 1, "block"));
  // W/S: zoom (around the cursor, or the middle); A/D: pan.
  chart.tabIndex = 0;
  chart.onkeydown = (e) => {
    if (!t.win) return;
    const mid = t.cursor != null && t.cursor >= t.win.a && t.cursor <= t.win.b ? t.cursor : (t.win.a + t.win.b) / 2;
    const act = { w: () => zoom(mid, -0.25), s: () => zoom(mid, 0.25), a: () => pan(-W() / 8), d: () => pan(W() / 8) }[e.key.toLowerCase()];
    if (!act) return;
    e.preventDefault();
    act();
  };
  // Hover: the call there outlined, and what it was (a syscall: its
  // arguments and result; a function: where, and its arguments).
  const tip = document.body.appendChild(Object.assign(document.createElement("div"), { className: "tip", hidden: true }));
  const args = new Map(); // first record -> the call's arguments
  const argsOf = (first) => args.get(first) ?? args.set(first, m.call_stack(first)?.find((f) => !f.inlined)?.args ?? []).get(first);
  // The call under the pointer: its column `k` and depth `d` (above a
  // column's stack: its top, the call running then; -1: none).
  function hit(e) {
    const k = Math.floor((e.offsetX / W()) * t.cols.length), c = t.cols[k];
    const d = Math.min(rowAt(e.offsetY - (chart.clientHeight - t.depth * t.row)), (c?.frames.length ?? 0) - 1);
    return { k, c, d };
  }
  function hover(e) {
    const { k, c, d } = hit(e);
    const [id, first, inl] = c?.frames[d] ?? [];
    hilite(first ?? null);
    const key = first == null ? null : `${first}:${inl}:${k}`;
    if (key !== t.peek) {
      // A preview of where the call is there (see app.preview), and so the tip's place.
      t.peek = key;
      const i = first == null ? 0 : Math.max(first, at(e.offsetX));
      app.preview(first == null ? null : () => {
        const frames = m.call_stack(i), name = inl ? iname(inl - 1) : null;
        const f = frames.find((x) => x.first === first && (name ? x.inlined && x.func === name : !x.inlined));
        const owner = frames.find((x) => x.first === first && !x.inlined);
        if (!f) return null;
        // (Where it was called from: the call outside it, where it is.)
        const caller = frames[frames.indexOf(f) + 1];
        // (And what runs then, on top of it.)
        if (key === t.peek) tip.querySelector(".here").textContent =
          (caller ? `called at ${caller.file}:${caller.line} · ` : "") + `now at ${f.file}:${f.line}` +
          (frames[0] && frames[0] !== f ? ` · running: ${frames[0].func}${frames[0].inlined ? " (inlined)" : ""} (${frames[0].file}:${frames[0].line})` : "");
        return { func: owner?.func ?? f.func, file: f.file, line: f.line, i };
      });
    }
    if (first == null) {
      tip.hidden = true;
      chart.title = c && d < 0 ? taskName(who(c.task, t.win.a)) : "";
      return;
    }
    chart.title = "";
    const f = fn(id), s = t.syscallAt.get(first);
    let end = k;
    while (end + 1 < t.cols.length && t.cols[end + 1].frames[d]?.[1] === first) end++;
    const span = `records ${first + 1}–${at(((end + 1) / t.cols.length) * W()) + 1}`;
    const line = (text, cls) => Object.assign(document.createElement("div"), { className: cls, textContent: text });
    const tipLines = (inl
      ? [line(`${iname(inl - 1)} (inlined)`, ""), line(`its code, inlined into ${f?.name}: not a call of its own · ${span}`, "dim")]
      : s
      ? [line(`${s.name ?? s.nr}(${s.args.slice(0, 3).join(", ")}…) = ${s.ret ?? "…"}`, ""),
         line(`a syscall by ${taskName(s.task)} · ${span}`, "dim")]
      : [line(f?.name ?? "?", ""), ...argsOf(first).map((a) => line(`${a.name} = ${a.value ?? "{…}"}`, "f")),
         ...inlinedIn(c, first).map((n) => line(`in ${n} (inlined)`, "f")),
         line(span, "dim")]);
    tip.replaceChildren(...tipLines, line("", "dim here"));
    tip.hidden = false;
    const r = tip.getBoundingClientRect();
    tip.style.left = `${Math.min(e.clientX + 14, innerWidth - r.width - 8)}px`;
    tip.style.top = `${e.clientY + 16 + r.height > innerHeight ? e.clientY - r.height - 8 : e.clientY + 16}px`;
  }
  // The code inlined into call `first` in column `c`, when it is not shown:
  // the functions, outermost first.
  const inlinedIn = (c, first) => (t.inlined ? [] : c.all.filter((f) => f[1] === first && f[2]).map((f) => iname(f[2] - 1)));
  chart.onpointerleave = () => {
    tip.hidden = true;
    t.peek = null;
    app.preview(null);
    hilite(null);
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
    if (t.win) drag = { x: e.clientX, y: e.clientY, top: $("chartbox").scrollTop, a: t.win.a, b: t.win.b, moved: false };
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
    // (A syscall: selected, its records from its entry to its exit.)
    const s = !inl && t.syscallAt.get(first);
    if (s) return app.select(s);
    go(Math.max(first, at(e.offsetX)));
    app.selectCall(first, inl ? iname(inl - 1) : null);
  };
  chart.ondblclick = (e) => {
    // Select the call there: its records, from its first to its return.
    const { c, d } = hit(e), [id, first, inl] = c?.frames[d] ?? [];
    if (first == null || inl || t.syscallAt.has(first)) return;
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
    const i = Math.round((e.offsetX / over.clientWidth) * (t.n - 1)), span = t.win.b - t.win.a;
    setWindow(i - span / 2, i + span / 2);
  };
  over.onpointerdown = (e) => {
    overDrag = true;
    try { over.setPointerCapture(e.pointerId); } catch { /* a synthetic event */ }
    overMove(e);
  };
  over.onpointermove = (e) => overDrag && overMove(e);
  over.onpointerup = () => (overDrag = false);
  addEventListener("resize", () => t.win && (load(), draw()));

  // ---- The API ----
  // After a snapshot: the new extent; the window at the selection (or all).
  function refresh(calls) {
    t.n = m.records();
    t.calls = calls;
    t.syscallAt = new Map(calls.map((c) => [c.first, c]));
    funcs.clear();
    tasks.clear();
    args.clear();
    inames.clear();
    t.buckets = m.timeline(Math.max(1, over.clientWidth));
    t.home = t.where?.() ?? null;
    t.depth = 1;
    if (t.cursor != null && t.cursor >= t.n) t.cursor = null;
    if (!t.n) t.win = null;
    else if (t.home) setWindow(...pad(t.home));
    else setWindow(0, t.n - 1);
    draw();
  }
  // Select records `where()` gives ({ a, b, task }: asked again after each
  // snapshot, as records move), called `title`: the window at them.
  function selectWindow(where, title) {
    t.depth = 1;
    t.where = where;
    t.title = title;
    t.home = where();
    if (t.home) setWindow(...pad(t.home));
  }
  // Select syscall `s` (found again in later snapshots by its task and
  // its first record's id: indexes move as the ring goes on).
  function select(s) {
    selectWindow(() => {
      const c = t.calls.find((c) => c.id === s.id) ?? s;
      return { a: c.first, b: c.last ?? t.n - 1, task: c.task.addr };
    }, `${s.name ?? s.nr} by ${taskName(s.task)}`);
  }
  // Put the cursor at record `i`, and show it.
  function go(i) {
    mark(i);
    const a = t.cursor != null ? m.at(t.cursor) : null;
    if (a) app.showAt(a);
  }
  // Put the cursor at record `i` without showing it (the caller shows the
  // line there); the window follows it.
  function mark(i) {
    if (!t.n) return;
    t.sel = null;
    t.cursor = Math.max(0, Math.min(t.n - 1, i));
    if (t.win && (t.cursor < t.win.a || t.cursor > t.win.b)) {
      const span = t.win.b - t.win.a;
      setWindow(t.cursor - span / 2, t.cursor + span / 2);
    } else draw();
  }
  // The legend, and whether to show inlined code as calls.
  const toggle = Object.assign(document.createElement("label"), { className: "key", title: "Show code inlined into a function as calls of their own (rows get thinner)" });
  const box = Object.assign(document.createElement("input"), { type: "checkbox", checked: t.inlined, onchange: () => {
    t.inlined = box.checked;
    try { localStorage.setItem("ksee.inlined", t.inlined ? "1" : "0"); } catch { /* not kept */ }
    t.depth = 1;
    load();
    drawChart();
  } });
  toggle.append(box, "inlined");
  $("legend").replaceChildren(...AREAS.map(([p, , name]) => {
    const k = document.createElement("span");
    k.className = "key";
    k.innerHTML = `<i style="background:${color(p, false)}"></i>${name}`;
    return k;
  }), toggle);
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
  return { refresh, go, mark, select, hilite, selected, boxOf, home: () => t.home, cursor: () => t.cursor, records: () => t.n };
}
