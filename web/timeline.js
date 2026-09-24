// The history: the trace ring as a strip (colored by the running task,
// syscalls marked), with a cursor to move through it. Moving the cursor
// shows where the kernel was at that record: which syscall, function and
// source line, on which task. The kernel itself keeps running (or stays
// stopped); this only replays what the trace recorded.
import { $, el, taskName } from "./dom.js";

const hue = (pid) => (pid < 0 ? 0 : (pid * 137) % 360);

export function timeline(app) {
  const { m } = app;
  const canvas = $("strip"), slider = $("cursor");
  let n = 0, buckets = [];

  function draw() {
    const w = (canvas.width = canvas.clientWidth * devicePixelRatio), h = (canvas.height = canvas.clientHeight * devicePixelRatio);
    const ctx = canvas.getContext("2d");
    ctx.clearRect(0, 0, w, h);
    buckets.forEach((b, x) => {
      ctx.fillStyle = `hsl(${hue(b.pid)} 55% ${b.pid === 0 ? 85 : 60}%)`;
      ctx.fillRect(x * devicePixelRatio, 0, devicePixelRatio, h * 0.6);
      if (b.syscall >= 0) {
        ctx.fillStyle = getComputedStyle(document.body).getPropertyValue("--accent");
        ctx.fillRect(x * devicePixelRatio, h * 0.7, devicePixelRatio, h * 0.3);
      }
    });
    const x = n ? (Number(slider.value) / (n - 1)) * w : 0;
    ctx.fillStyle = getComputedStyle(document.body).getPropertyValue("--fg");
    ctx.fillRect(x - devicePixelRatio, 0, 2 * devicePixelRatio, h);
  }

  // After a snapshot: the new extent; the cursor at the end.
  function refresh() {
    n = m.records();
    slider.max = String(Math.max(0, n - 1));
    slider.value = slider.max;
    buckets = m.timeline(Math.max(1, canvas.clientWidth));
    const tasks = [...new Map(buckets.filter((b) => b.pid >= 0).map((b) => [b.pid, b.comm])).entries()];
    $("legend").replaceChildren(...tasks.slice(0, 8).map(([pid, comm]) =>
      el("span", { className: "key" }, el("i", { style: `background:hsl(${hue(pid)} 55% ${pid === 0 ? 85 : 60}%)` }), `${comm}(${pid})`)),
      el("span", { className: "key" }, el("i", { style: "background:var(--accent)" }), "in a syscall"));
    $("where").textContent = `${n} records in the trace ring: move the cursor to replay`;
    draw();
  }

  function go(i) {
    slider.value = String(i);
    draw();
    const at = m.at(i);
    if (!at) return;
    $("where").textContent = `record ${i + 1}/${n} · ${taskName(at.task)} · ${at.func} ${at.file ?? ""}:${at.line ?? ""}` +
      (at.syscall != null ? ` · in syscall #${at.syscall}` : "");
    app.showAt(at);
  }
  slider.oninput = () => go(Number(slider.value));
  canvas.onclick = (e) => go(Math.round((e.offsetX / canvas.clientWidth) * (n - 1)));
  for (const [id, dir, unit] of [["back-sys", -1, "syscall"], ["back-fn", -1, "function"], ["back-blk", -1, "block"],
                                 ["fwd-blk", 1, "block"], ["fwd-fn", 1, "function"], ["fwd-sys", 1, "syscall"]])
    $(id).onclick = () => go(m.seek(Number(slider.value), dir, unit));
  addEventListener("resize", () => n && refresh());
  return { refresh };
}
