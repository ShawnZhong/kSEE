// The history. On top, the whole trace ring: a lane per task (where it
// ran; tasks without syscalls share one), its syscalls as bars. Below, a
// flame chart of a window of it (the selected syscall, on its task): the
// calls, rebuilt from the trace (Machine.frames), by depth. Both show the
// cursor. Click or drag to move it; click a syscall bar to select it; in
// the flame chart, scroll to zoom and drag to pan (double-click: back to
// the syscall). Moving the cursor replays what the trace recorded; the
// kernel itself keeps running (or stays stopped).
import { $, el, taskName } from "./dom.js";

const LANE = 16, LABEL = 92, TOP = 2, ROW = 15;
const hue = (pid) => (pid * 137) % 360;
const css = (v) => getComputedStyle(document.body).getPropertyValue(v).trim();

// Calls are colored by where the function is.
const AREAS = [["fs/", 210, "fs"], ["mm/", 135, "mm"], ["kernel/sched/", 28, "sched"], ["kernel/", 48, "kernel"],
               ["drivers/", 280, "drivers"], ["lib/", 175, "lib"], ["include/", 330, "include"], ["arch/", -1, "arch"]];
const area = (file) => AREAS.find(([p]) => file.startsWith(p)) ?? [null, -1, "other"];
const color = (file, on) => {
  const [, h] = area(file);
  return h < 0 ? `hsl(0 0% ${on ? 55 : 72}%)` : `hsl(${h} ${on ? 70 : 55}% ${on ? 48 : 70}%)`;
};

export function timeline(app) {
  const { m } = app;
  const lanes = $("lanes"), flame = $("flame");
  // n: records; lanes: tasks; calls: syscalls drawn; win: the flame chart's
  // window { a, b, task }, frames in it; home: the selected syscall's.
  const t = { n: 0, buckets: [], lanes: [], all: [], calls: [], cursor: null, sel: null, win: null, frames: [], home: null };

  // ---- The lanes ----
  const lw = () => lanes.clientWidth - LABEL;
  const lx = (i) => LABEL + (t.n > 1 ? (i / (t.n - 1)) * lw() : 0);
  const li = (x) => Math.max(0, Math.min(t.n - 1, Math.round(((x - LABEL) / lw()) * (t.n - 1))));
  const laneOf = (pid) => {
    const k = t.lanes.findIndex((l) => l.pid === pid);
    return k < 0 && pid >= 0 ? t.lanes.length - 1 : k;
  };
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
  function drawLanes() {
    const h = TOP + Math.max(1, t.lanes.length) * LANE + 2, ctx = canvas(lanes, h);
    t.lanes.forEach((l, k) => {
      const y = TOP + k * LANE;
      ctx.fillStyle = css("--dim");
      ctx.fillText(l.pid == null ? l.comm : `${l.comm}(${l.pid})`.slice(0, 13), 4, y + LANE / 2);
      ctx.fillStyle = css("--line");
      ctx.fillRect(LABEL, y + LANE - 1, lw(), 1);
    });
    t.buckets.forEach((b, x) => {
      const k = laneOf(b.pid);
      if (k < 0) return;
      ctx.fillStyle = `hsl(${hue(b.pid)} 50% ${b.pid === 0 ? 80 : 62}% / .55)`;
      ctx.fillRect(LABEL + x, TOP + k * LANE + 3, 1, LANE - 6);
    });
    for (const s of t.calls) {
      const k = laneOf(s.task.pid);
      if (k < 0) continue;
      const x1 = lx(s.first), x2 = Math.max(x1 + 2, lx(s.last ?? t.n - 1)), y = TOP + k * LANE + 2;
      const on = t.sel === app.key(s);
      ctx.fillStyle = on ? css("--accent") : s.control ? css("--line") : css("--sys");
      ctx.fillRect(x1, y, x2 - x1, LANE - 4);
      const label = s.name ?? String(s.nr);
      if (x2 - x1 > ctx.measureText(label).width + 6) {
        ctx.fillStyle = on ? css("--panel") : css("--fg");
        ctx.fillText(label, x1 + 3, y + (LANE - 4) / 2);
      }
    }
    // The flame chart's window, and the cursor (red: now).
    if (t.win) {
      ctx.strokeStyle = css("--accent");
      ctx.strokeRect(lx(t.win.a) - 1, 0.5, Math.max(2, lx(t.win.b) - lx(t.win.a) + 2), h - 1);
    }
    const x = lx(t.cursor ?? t.n - 1);
    ctx.fillStyle = t.cursor == null ? css("--miss") : css("--fg");
    ctx.fillRect(x - 1, 0, 2, h);
  }

  // ---- The flame chart ----
  const fw = () => flame.clientWidth;
  const fx = (i) => ((i - t.win.a) / Math.max(1, t.win.b - t.win.a + 1)) * fw();
  const fi = (x) => Math.round(t.win.a + (x / fw()) * (t.win.b - t.win.a + 1));
  function loadFrames() {
    if (!t.win) return (t.frames = []);
    const min = Math.max(1, Math.floor((t.win.b - t.win.a) / Math.max(1, fw()) / 2));
    t.frames = m.frames(t.win.a, t.win.b, t.win.task, min);
  }
  function drawFlame() {
    $("flamebox").hidden = !t.win;
    if (!t.win) return;
    const depth = t.frames.reduce((d, f) => Math.max(d, f.depth + 1), 1), ctx = canvas(flame, depth * ROW + 2);
    const cur = t.cursor ?? t.n - 1;
    for (const f of t.frames) {
      const x1 = Math.max(0, fx(f.first)), x2 = Math.min(fw(), fx(f.last + 1)), y = f.depth * ROW;
      if (x2 - x1 < 0.5) continue;
      const on = f.first <= cur && cur <= f.last;
      ctx.fillStyle = color(f.file, on);
      ctx.fillRect(x1, y, Math.max(1, x2 - x1 - 1), ROW - 1);
      if (x2 - x1 > 30) {
        ctx.save();
        ctx.beginPath();
        ctx.rect(x1, y, x2 - x1 - 2, ROW);
        ctx.clip();
        ctx.fillStyle = on ? "#fff" : "#1f2328";
        ctx.fillText(f.func, x1 + 3, y + ROW / 2);
        ctx.restore();
      }
    }
    if (cur >= t.win.a && cur <= t.win.b) {
      ctx.fillStyle = t.cursor == null ? css("--miss") : css("--fg");
      ctx.fillRect(fx(cur) - 1, 0, 2, depth * ROW + 2);
    }
    const s = t.all.find((s) => app.key(s) === t.sel);
    $("flame-title").textContent = s ? `${s.name} by ${taskName(s.task)}: its calls` : "calls";
  }
  const frameAt = (e) => {
    const i = fi(e.offsetX), d = Math.floor(e.offsetY / ROW);
    return t.frames.find((f) => f.depth === d && f.first <= i && i <= f.last);
  };
  // Zoom and pan keep the window inside the trace.
  function setWindow(a, b) {
    const span = Math.max(20, b - a);
    a = Math.max(0, Math.min(a, t.n - 1 - span));
    t.win = { ...t.win, a: Math.round(a), b: Math.round(Math.min(t.n - 1, a + span)) };
    loadFrames();
    draw();
  }
  flame.onwheel = (e) => {
    e.preventDefault();
    const at = fi(e.offsetX), k = e.deltaY > 0 ? 1.25 : 0.8;
    setWindow(at - (at - t.win.a) * k, at + (t.win.b - at) * k);
  };
  let drag = null;
  flame.onpointerdown = (e) => (drag = { x: e.offsetX, a: t.win.a, b: t.win.b, moved: false });
  flame.onpointermove = (e) => {
    if (!drag) {
      const f = frameAt(e);
      flame.title = f ? `${f.func} (${f.file}) · ${f.last - f.first + 1} records` : "";
      return;
    }
    const dx = e.offsetX - drag.x;
    if (Math.abs(dx) > 3) drag.moved = true;
    if (drag.moved) {
      const per = (drag.b - drag.a) / fw();
      setWindow(drag.a - dx * per, drag.b - dx * per);
    }
  };
  flame.onpointerup = (e) => {
    const was = drag;
    drag = null;
    if (was && !was.moved) go(m.seek(Math.max(0, fi(e.offsetX) - 1), 1, "block"));
  };
  flame.ondblclick = () => t.home && ((t.win = { ...t.home }), loadFrames(), draw());

  function draw() {
    drawLanes();
    drawFlame();
  }

  // After a snapshot: the new extent, and the cursor back at now.
  function refresh(list) {
    t.n = m.records();
    t.all = list;
    t.cursor = null;
    t.buckets = m.timeline(Math.max(1, lw()));
    t.calls = list.filter((s) => $("show-control").checked || !s.control);
    const ls = new Map([[0, { pid: 0, comm: "swapper/0" }]]);
    for (const s of t.calls) if (s.task.pid != null) ls.set(s.task.pid, { pid: s.task.pid, comm: s.task.comm });
    t.lanes = [...ls.values()].sort((a, b) => a.pid - b.pid);
    if (t.buckets.some((b) => b.pid >= 0 && !ls.has(b.pid))) t.lanes.push({ pid: null, comm: "other tasks" });
    // The selected syscall's records moved as the ring went on.
    const s = t.all.find((s) => app.key(s) === t.sel);
    if (s) show(s);
    else t.win = null;
    loadFrames();
    draw();
  }

  // Move the cursor to record `i` (null: now) and show it.
  function go(i) {
    t.cursor = i == null || i >= t.n - 1 ? null : i;
    if (t.cursor != null && t.win && (t.cursor < t.win.a || t.cursor > t.win.b)) {
      const span = t.win.b - t.win.a;
      setWindow(t.cursor - span / 2, t.cursor + span / 2);
    } else draw();
    if (t.cursor == null) return app.showNow();
    const at = m.at(t.cursor);
    if (at) app.showAt(at);
  }

  const hit = (e) => {
    const k = Math.floor((e.offsetY - TOP) / LANE), i = li(e.offsetX), slop = (t.n / lw()) * 3;
    return t.calls.find((s) => laneOf(s.task.pid) === k && s.first - slop <= i && i <= (s.last ?? t.n) + slop);
  };
  let dragging = false;
  lanes.onpointerdown = (e) => {
    if (e.offsetX < LABEL) return;
    const s = hit(e);
    if (s) return app.select(s);
    dragging = true;
    try { lanes.setPointerCapture(e.pointerId); } catch { /* a synthetic event */ }
    go(li(e.offsetX));
  };
  lanes.onpointermove = (e) => {
    if (dragging) go(li(e.offsetX));
    else lanes.title = hit(e) ? `${hit(e).name}: click to select` : "";
  };
  lanes.onpointerup = () => (dragging = false);
  addEventListener("resize", () => t.n && refresh(t.all));

  // The flame chart shows syscall `s`.
  function show(s) {
    t.home = { a: s.first, b: s.last ?? t.n - 1, task: s.task.addr };
    t.win = { ...t.home };
  }
  // Select syscall `s` (its bar highlighted, its calls in the flame chart);
  // the cursor to record `i`, if given.
  function select(s, i) {
    const key = app.key(s);
    if (t.sel !== key || !t.win) {
      t.sel = key;
      show(s);
      loadFrames();
    }
    if (i !== undefined) t.cursor = i;
    draw();
  }
  $("legend").replaceChildren(...AREAS.map(([p, , name]) =>
    el("span", { className: "key" }, el("i", { style: `background:${color(p, false)}` }), name)));
  return { refresh, go, select, cursor: () => t.cursor, records: () => t.n,
           selected: () => t.all.find((s) => app.key(s) === t.sel) };
}
