// The page's layout: the columns and the timeline's height, dragged (kept
// in localStorage); the console's text (its tab).
import { $ } from "./dom.js";

// The layout, set up; its console: add(text), clear().
export function layout() {
  let consoleText = "";
  function showConsole() {
    $("console").textContent = consoleText;
    $("pane-console").scrollTop = $("pane-console").scrollHeight;
  }

  const sizes = { left: 340, right: 380, chart: 190 };
  try { Object.assign(sizes, JSON.parse(localStorage.getItem("ksee.sizes") ?? "{}")); } catch { /* none kept */ }
  const draw = () => {
    $("app").style.gridTemplateColumns = `${sizes.left}px 5px minmax(0, 1fr)`;
    $("main").style.gridTemplateColumns = `minmax(300px, 1fr) 5px ${sizes.right}px`;
    $("chartbox").style.height = `${sizes.chart}px`;
    try { localStorage.setItem("ksee.sizes", JSON.stringify(sizes)); } catch { /* not kept */ }
  };
  // (`click`: a press that did not move.)
  const drag = (handle, move, click) => (handle.onpointerdown = (e) => {
    e.preventDefault();
    const start = { x: e.clientX, y: e.clientY, ...sizes };
    let moved = false;
    const onMove = (ev) => {
      moved ||= Math.abs(ev.clientX - start.x) + Math.abs(ev.clientY - start.y) > 3;
      if (!moved) return;
      move(start, ev.clientX - start.x, ev.clientY - start.y);
      draw();
    };
    addEventListener("pointermove", onMove);
    addEventListener("pointerup", () => {
      removeEventListener("pointermove", onMove);
      if (!moved) click?.();
      draw();
      dispatchEvent(new Event("resize")); // (the canvases redraw)
    }, { once: true });
  });
  for (const h of document.querySelectorAll(".vsplit")) {
    drag(h, (s, dx) => {
      if (h.dataset.col === "left") sizes.left = Math.max(200, s.left + dx);
      else sizes.right = Math.max(220, s.right - dx);
    });
  }
  drag($("hsplit"), (s, _, dy) => (sizes.chart = Math.max(60, s.chart + dy)));
  draw();
  return {
    add: (text) => ((consoleText += text), showConsole()),
    clear: () => ((consoleText = ""), showConsole()),
  };
}
