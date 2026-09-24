// The history: the trace ring over time, one lane per task (where it ran),
// with its syscalls as labeled bars (tasks without syscalls share a lane); and the cursor, a position in it.
// Click or drag to move the cursor (← → step blocks); click a syscall to
// select it. At the right end is now (or where the kernel is stopped).
// Moving the cursor replays what the trace recorded; the kernel itself
// keeps running (or stays stopped).
import { $, taskName } from "./dom.js";

const LANE = 16, LABEL = 92, TOP = 2;
const hue = (pid) => (pid * 137) % 360;
const css = (v) => getComputedStyle(document.body).getPropertyValue(v).trim();

export function timeline(app) {
  const { m } = app;
  const canvas = $("lanes");
  // n: records; buckets: per pixel column; lanes: tasks by pid; calls: syscalls shown.
  const t = { n: 0, buckets: [], lanes: [], all: [], calls: [], cursor: null, sel: null };

  const width = () => canvas.clientWidth - LABEL;
  const xOf = (i) => LABEL + (t.n > 1 ? (i / (t.n - 1)) * width() : 0);
  const iOf = (x) => Math.max(0, Math.min(t.n - 1, Math.round(((x - LABEL) / width()) * (t.n - 1))));
  const laneOf = (pid) => {
    const k = t.lanes.findIndex((l) => l.pid === pid);
    return k < 0 && pid >= 0 ? t.lanes.length - 1 : k;
  };

  function draw() {
    const dpr = devicePixelRatio, h = TOP + Math.max(1, t.lanes.length) * LANE + 2;
    canvas.style.height = `${h}px`;
    canvas.width = canvas.clientWidth * dpr;
    canvas.height = h * dpr;
    const ctx = canvas.getContext("2d");
    ctx.scale(dpr, dpr);
    ctx.font = `11px ${css("--mono")}`;
    ctx.textBaseline = "middle";
    t.lanes.forEach((l, k) => {
      const y = TOP + k * LANE;
      ctx.fillStyle = css("--dim");
      ctx.fillText(l.pid == null ? l.comm : `${l.comm}(${l.pid})`.slice(0, 13), 4, y + LANE / 2);
      ctx.fillStyle = css("--line");
      ctx.fillRect(LABEL, y + LANE - 1, width(), 1);
    });
    // Where each task ran.
    t.buckets.forEach((b, x) => {
      const k = laneOf(b.pid);
      if (k < 0) return;
      ctx.fillStyle = `hsl(${hue(b.pid)} 50% ${b.pid === 0 ? 80 : 62}% / .55)`;
      ctx.fillRect(LABEL + x, TOP + k * LANE + 3, 1, LANE - 6);
    });
    // Syscalls, in their task's lane.
    for (const s of t.calls) {
      const k = laneOf(s.task.pid);
      if (k < 0) continue;
      const x1 = xOf(s.first), x2 = Math.max(x1 + 2, xOf(s.last ?? t.n - 1)), y = TOP + k * LANE + 2;
      const on = t.sel === app.key(s);
      ctx.fillStyle = on ? css("--accent") : s.control ? css("--line") : css("--sys");
      ctx.fillRect(x1, y, x2 - x1, LANE - 4);
      const label = s.name ?? String(s.nr);
      if (x2 - x1 > ctx.measureText(label).width + 6) {
        ctx.fillStyle = on ? css("--panel") : css("--fg");
        ctx.fillText(label, x1 + 3, y + (LANE - 4) / 2);
      }
    }
    // The cursor (or now, at the end).
    const x = xOf(t.cursor ?? t.n - 1);
    ctx.fillStyle = t.cursor == null ? css("--miss") : css("--fg");
    ctx.fillRect(x - 1, 0, 2, h);
  }

  // After a snapshot: the new extent, and the cursor back at now.
  function refresh(list) {
    t.n = m.records();
    t.all = list;
    t.cursor = null;
    t.buckets = m.timeline(Math.max(1, width()));
    t.calls = list.filter((s) => $("show-control").checked || !s.control);
    // A lane for the idle task and each task with syscalls shown; one for the rest.
    const lanes = new Map([[0, { pid: 0, comm: "swapper/0" }]]);
    for (const s of t.calls) if (s.task.pid != null) lanes.set(s.task.pid, { pid: s.task.pid, comm: s.task.comm });
    t.lanes = [...lanes.values()].sort((a, b) => a.pid - b.pid);
    if (t.buckets.some((b) => b.pid >= 0 && !lanes.has(b.pid))) t.lanes.push({ pid: null, comm: "other tasks" });
    $("where").textContent = `${t.n} records in the trace ring · click or drag to replay, click a syscall to select it`;
    draw();
  }

  // Move the cursor to record `i` (null: now) and show it.
  function go(i) {
    t.cursor = i == null || i >= t.n - 1 ? null : i;
    draw();
    if (t.cursor == null) return app.showNow();
    const at = m.at(i);
    if (!at) return;
    $("where").textContent = `record ${i + 1} of ${t.n} · ${taskName(at.task)} · ${at.func} ${at.file ?? ""}:${at.line ?? ""}`;
    app.showAt(at);
  }
  const hit = (e) => {
    const k = Math.floor((e.offsetY - TOP) / LANE), i = iOf(e.offsetX), slop = (t.n / width()) * 3;
    return t.calls.find((s) => laneOf(s.task.pid) === k && s.first - slop <= i && i <= (s.last ?? t.n) + slop);
  };
  let dragging = false;
  canvas.onpointerdown = (e) => {
    if (e.offsetX < LABEL) return;
    const s = hit(e);
    if (s) return app.select(s);
    dragging = true;
    try { canvas.setPointerCapture(e.pointerId); } catch { /* a synthetic event */ }
    go(iOf(e.offsetX));
  };
  canvas.onpointermove = (e) => {
    if (dragging) go(iOf(e.offsetX));
    else canvas.title = hit(e) ? `${hit(e).name}: click to select` : "";
  };
  canvas.onpointerup = () => (dragging = false);
  canvas.onkeydown = (e) => {
    const dir = e.key === "ArrowLeft" ? -1 : e.key === "ArrowRight" ? 1 : 0;
    if (!dir) return;
    e.preventDefault();
    step(dir, e.shiftKey ? "function" : "block");
  };
  const step = (dir, unit) => go(m.seek(t.cursor ?? t.n - 1, dir, unit));
  for (const [id, dir, unit] of [["back-sys", -1, "syscall"], ["back-fn", -1, "function"], ["back-blk", -1, "block"],
                                 ["fwd-blk", 1, "block"], ["fwd-fn", 1, "function"], ["fwd-sys", 1, "syscall"]])
    $(id).onclick = () => step(dir, unit);
  $("live").onclick = () => go(null);
  addEventListener("resize", () => t.n && refresh(t.all));

  // Select syscall `key` (bars drawn highlighted); the cursor to record `i`.
  function select(key, i) {
    t.sel = key;
    if (i !== undefined) t.cursor = i;
    draw();
  }
  return { refresh, go, select, cursor: () => t.cursor };
}
