// The timeline of the recording. On top, an overview of all of it: which
// task ran when, the window shown below, the cursor. Below, the window as a
// stack chart: a strip of which task runs, and under it that task's call
// stack (Machine.chart), each call a bar, colored by where its function is
// (a syscall's entry named for the syscall). The calls every column of the
// window shares are one line above it; the rest fit the box (rows as thin
// as it takes). Scroll to zoom (sideways: pan), drag to pan, click to put
// the cursor there, double-click a call to zoom to it; click or drag the
// overview to move the window.
import { $, taskName } from "./dom.js";

const ROW = 15, STRIP = 12, OVER = 18;
const css = (v) => getComputedStyle(document.body).getPropertyValue(v).trim();
const hue = (pid) => (pid * 137) % 360;
// Calls are colored by where the function is.
const AREAS = [["fs/", 210, "fs"], ["mm/", 135, "mm"], ["kernel/sched/", 28, "sched"], ["kernel/", 48, "kernel"],
               ["drivers/", 280, "drivers"], ["lib/", 175, "lib"], ["include/", 330, "include"], ["arch/", -1, "arch"]];
const color = (file, on) => {
  const h = (AREAS.find(([p]) => file?.startsWith(p)) ?? [null, -1])[1];
  return h < 0 ? `hsl(0 0% ${on ? 55 : 76}%)` : `hsl(${h} ${on ? 65 : 55}% ${on ? 50 : 74}%)`;
};

export function timeline(app) {
  const { m } = app;
  const over = $("overview"), chart = $("chart");
  // n: records; calls: syscalls; win: the window shown { a, b }; home: the
  // selection (a syscall's or program's records, and its task); cols: the
  // chart's columns [{ task, frames: [[func, first]] }].
  const t = { n: 0, calls: [], win: null, home: null, where: null, title: "", cursor: null, cols: [], buckets: [],
              base: 0, row: 15 }; // (base: the depth where rows start; row: their height)
  const funcs = new Map(), tasks = new Map();
  const fn = (id) => (funcs.has(id) ? funcs.get(id) : funcs.set(id, m.function(id)).get(id));
  const who = (addr, i) => (tasks.has(addr) ? tasks.get(addr) : tasks.set(addr, m.task(addr, i)).get(addr));

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
      ctx.fillText("Nothing recorded yet: run a tour or a program (or ● Record). What the kernel runs appears here.", 4, OVER / 2);
      return;
    }
    t.buckets.forEach((b, k) => {
      ctx.fillStyle = b.pid < 0 ? css("--line") : `hsl(${hue(b.pid)} 50% ${b.pid === 0 ? 85 : 62}%)`;
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
  // A call's name (a syscall's entry: named for the syscall).
  const nameOf = (id, first) => {
    const f = fn(id);
    const s = f?.name === "wasm_syscall" ? t.calls.find((c) => c.first === first) : null;
    return s ? `${s.name ?? s.nr}()` : f?.name ?? "?";
  };
  function drawChart() {
    const depth = t.cols.reduce((d, c) => Math.max(d, c.frames.length), 1);
    // The calls every column shares (same call, same depth): one line.
    let base = 0;
    while (base < depth && t.cols.length && t.cols.every((c) => c.frames[base] && c.frames[base][1] === t.cols[0].frames[base][1])) base++;
    const rows = Math.max(1, depth - base), room = ($("chartbox").clientHeight || 190) - STRIP - 4;
    t.base = base;
    t.row = Math.max(5, Math.min(ROW, Math.floor(room / rows)));
    const ctx = canvas(chart, STRIP + 2 + rows * t.row);
    $("chart-base").textContent = base ? t.cols[0].frames.slice(0, base).map(([id, first]) => nameOf(id, first)).join(" › ") : "";
    if (!t.win) return;
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
      ctx.fillStyle = `hsl(${hue(p?.pid ?? 0)} 50% ${p?.pid === 0 ? 85 : 62}%)`;
      ctx.fillRect(r.from * cw, 0, (r.to - r.from) * cw, STRIP);
      label(ctx, taskName(p), r.from * cw, (r.to - r.from) * cw, STRIP / 2, "#1f2328");
    }
    // The stacks (the calls at the cursor: darker).
    for (let d = base; d < depth; d++) {
      for (const r of runs((c) => (c.frames[d] ? `${c.frames[d][0]}:${c.frames[d][1]}` : undefined))) {
        const [id, first] = r.c.frames[d], f = fn(id), y = STRIP + 2 + (d - base) * t.row;
        const x1 = r.from * cw, w = (r.to - r.from) * cw;
        const on = curCol?.frames[d]?.[1] === first && cur >= t.win.a && cur <= t.win.b;
        ctx.fillStyle = color(f?.file, on);
        ctx.fillRect(x1, y, Math.max(1, w - 1), t.row - 1);
        if (t.row >= 11) label(ctx, nameOf(id, first), x1, w, y + t.row / 2, on ? "#fff" : "#1f2328");
      }
    }
    if (cur >= t.win.a && cur <= t.win.b) {
      ctx.fillStyle = css("--fg");
      ctx.fillRect(x(cur) - 1, 0, 2, STRIP + 2 + rows * t.row);
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
      for (let d = 0; d < depth; d++) frames.push([flat[k + 2 + 2 * d], flat[k + 3 + 2 * d]]);
      out.push({ task, frames });
      k += 2 + 2 * depth;
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
  const pad = (w) => {
    const p = Math.max(10, Math.round((w.b - w.a) * 0.03));
    return [w.a - p, w.b + p];
  };

  // ---- Input ----
  chart.onwheel = (e) => {
    if (!t.win) return;
    e.preventDefault();
    const span = t.win.b - t.win.a;
    if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) {
      // Sideways (a trackpad): pan.
      const d = (e.deltaX / W()) * span;
      return setWindow(t.win.a + d, t.win.b + d);
    }
    const i = at(e.offsetX), k = Math.exp(Math.max(-0.5, Math.min(0.5, e.deltaY * 0.002)));
    setWindow(i - (i - t.win.a) * k, i + (t.win.b - i) * k);
  };
  let drag = null;
  chart.onpointerdown = (e) => t.win && (drag = { x: e.offsetX, a: t.win.a, b: t.win.b, moved: false });
  chart.onpointermove = (e) => {
    if (!drag) {
      const c = t.cols[Math.floor((e.offsetX / W()) * t.cols.length)], d = t.base + Math.floor((e.offsetY - STRIP - 2) / t.row);
      const f = c?.frames[d] && fn(c.frames[d][0]);
      chart.title = f ? `${f.name} (${f.file}:${f.line})` : c ? taskName(who(c.task, t.win.a)) : "";
      return;
    }
    const dx = e.offsetX - drag.x;
    if (Math.abs(dx) > 3) drag.moved = true;
    if (drag.moved) {
      const per = (drag.b - drag.a) / W();
      setWindow(drag.a - dx * per, drag.b - dx * per);
    }
  };
  chart.onpointerup = (e) => {
    const was = drag;
    drag = null;
    if (was && !was.moved) go(m.seek(Math.max(0, at(e.offsetX) - 1), 1, "block"));
  };
  chart.ondblclick = (e) => {
    // Zoom to the call there: from its first record to where it ends.
    const k = Math.floor((e.offsetX / W()) * t.cols.length), d = t.base + Math.floor((e.offsetY - STRIP - 2) / t.row);
    const key = t.cols[k]?.frames[d]?.[1];
    if (key == null) return;
    let end = k;
    while (end + 1 < t.cols.length && t.cols[end + 1].frames[d]?.[1] === key) end++;
    setWindow(...pad({ a: key, b: at(((end + 1) / t.cols.length) * W()) }));
  };
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
    funcs.clear();
    tasks.clear();
    t.buckets = m.timeline(Math.max(1, over.clientWidth));
    t.home = t.where?.() ?? null;
    if (t.cursor != null && t.cursor >= t.n) t.cursor = null;
    if (!t.n) t.win = null;
    else if (t.home) setWindow(...pad(t.home));
    else setWindow(0, t.n - 1);
    draw();
  }
  // Select records `where()` gives ({ a, b, task }: asked again after each
  // snapshot, as records move), called `title`: the window at them.
  function selectWindow(where, title) {
    t.where = where;
    t.title = title;
    t.home = where();
    if (t.home) setWindow(...pad(t.home));
  }
  // Select syscall `s`.
  function select(s) {
    const key = app.key(s);
    selectWindow(() => {
      const c = t.calls.find((c) => app.key(c) === key) ?? s;
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
    t.cursor = Math.max(0, Math.min(t.n - 1, i));
    if (t.win && (t.cursor < t.win.a || t.cursor > t.win.b)) {
      const span = t.win.b - t.win.a;
      setWindow(t.cursor - span / 2, t.cursor + span / 2);
    } else draw();
  }
  $("legend").replaceChildren(...AREAS.map(([p, , name]) => {
    const k = document.createElement("span");
    k.className = "key";
    k.innerHTML = `<i style="background:${color(p, false)}"></i>${name}`;
    return k;
  }));
  return { refresh, go, mark, select, selectWindow, home: () => t.home, cursor: () => t.cursor, records: () => t.n };
}
