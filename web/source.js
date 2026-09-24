// The source view: a whole kernel file, with the lines where blocks ran
// (and how often), breakpoints, and where the kernel is stopped or where
// the history cursor is; and a function's control flow graph.
import { $, el, svg } from "./dom.js";

const KEYWORDS = new Set(("auto break case char const continue default do double else enum extern float for goto if " +
  "inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void " +
  "volatile while bool true false NULL u8 u16 u32 u64 s8 s16 s32 s64 size_t ssize_t loff_t __user unlikely likely").split(" "));

// C, roughly: comments (also across lines), strings, preprocessor lines,
// keywords, numbers. One list of nodes per line.
function highlight(lines) {
  let comment = false;
  return lines.map((line) => {
    const out = [];
    const push = (cls, text) => out.push(cls ? el("span", { className: cls, textContent: text }) : text);
    if (!comment && /^\s*#/.test(line)) return [el("span", { className: "pp", textContent: line })];
    for (let i = 0; i < line.length;) {
      if (comment) {
        const end = line.indexOf("*/", i), stop = end < 0 ? line.length : end + 2;
        push("cm", line.slice(i, stop));
        i = stop;
        comment = end < 0;
        continue;
      }
      const rest = line.slice(i);
      let t;
      if (rest.startsWith("//")) { push("cm", rest); break; }
      if (rest.startsWith("/*")) { comment = true; continue; }
      if ((t = rest.match(/^"(\\.|[^"\\])*"?|^'(\\.|[^'\\])*'?/))) push("st", t[0]);
      else if ((t = rest.match(/^[A-Za-z_]\w*/))) push(KEYWORDS.has(t[0]) ? "kw" : "", t[0]);
      else if ((t = rest.match(/^(0x[0-9a-fA-F]+|\d+)[uUlL]*/))) push("nu", t[0]);
      else push("", (t = [rest[0]])[0]);
      i += t[0].length;
    }
    return out;
  });
}

export function source(app) {
  const { m } = app;
  const files = new Map();
  const fetchFile = (file) => {
    if (!files.has(file))
      files.set(file, file.startsWith("/") ? Promise.resolve(null)
        : fetch(new URL(`./src/${file}`, location)).then((r) => (r.ok ? r.text() : null)).catch(() => null));
    return files.get(file);
  };
  // What is shown, and the markers: breakpoints, the stop, the history cursor.
  const v = { tab: "src", file: null, fn: null, syscall: undefined, breaks: new Set(), stop: null, cursor: null };

  async function showFile(file, focus) {
    v.file = file;
    const text = await fetchFile(file);
    if (v.file !== file || v.tab !== "src") return; // changed while loading
    if (text === null)
      return $("code").replaceChildren(el("div", { className: "empty", textContent: `${file}: not in this site's sources.` }));
    const cov = new Map(m.file_coverage(file, v.syscall).map((c) => [c.line, c.count]));
    const lines = text.split("\n"), hl = highlight(lines);
    const fn = v.fn?.file === file ? v.fn : null;
    const last = fn ? Math.max(fn.line, ...fn.blocks.filter((b) => b.file === file).map((b) => b.line)) : 0;
    const mark = (at, n) => at && at.file === file && at.line === n;
    const rows = lines.map((_, i) => {
      const n = i + 1, c = cov.get(n), key = `${file}:${n}`;
      const cls = [c === undefined ? "" : c > 0 ? "hit" : "miss", mark(v.stop, n) ? "at" : "", mark(v.cursor, n) ? "cursor" : "",
                   fn && n >= fn.line && n <= last ? "infn" : ""].join(" ");
      const num = el("td", { className: "n" + (c !== undefined ? " can" : ""), textContent: n });
      if (c !== undefined)
        num.onclick = () => {
          const on = !v.breaks.has(key);
          if (m.break_line(file, n, on)) on ? v.breaks.add(key) : v.breaks.delete(key);
          showFile(file, n);
        };
      return el("tr", { className: cls, id: `L${n}` },
        el("td", { className: "bp", textContent: mark(v.stop, n) ? "▶" : mark(v.cursor, n) ? "◆" : v.breaks.has(key) ? "●" : "" }),
        num, el("td", { className: "h", textContent: c === undefined ? "" : c > 0 ? `${c}×` : "0" }),
        el("td", { className: "code" }, ...hl[i]));
    });
    $("code").replaceChildren(el("table", { className: "src" }, el("tbody", {}, ...rows)));
    $("code").querySelector(`#L${focus ?? fn?.line ?? 1}`)?.scrollIntoView({ block: focus ? "center" : "start" });
    const ran = fn ? ` · ${fn.name}: ${fn.blocks.filter((b) => b.count > 0).length} of ${fn.blocks.length} blocks ran` : "";
    $("fn-title").textContent = file + ran;
  }

  // Blocks in order; forward edges on the right, backward on the left;
  // taken edges thick, with counts. Click a block for its source.
  function cfg(f) {
    const H = 22, GAP = 8, X = 170, W = 240, y = (i) => 8 + i * (H + GAP);
    const index = new Map(f.blocks.map((b, i) => [b.n, i]));
    const edges = f.blocks.flatMap((b) => b.succs.map((e) => ({ from: index.get(b.n), to: index.get(e.to), count: e.count })))
      .filter((e) => e.to !== undefined);
    for (const fwd of [true, false])
      edges.filter((e) => e.to > e.from === fwd && Math.abs(e.to - e.from) > 1)
        .sort((a, b) => Math.abs(a.to - a.from) - Math.abs(b.to - b.from)).forEach((e, i) => (e.lane = 1 + Math.min(i, 12)));
    const g = svg("svg", { width: X + W + 150, height: y(f.blocks.length) + 8 });
    for (const e of edges) {
      const y1 = y(e.from) + H / 2, y2 = y(e.to) + H / 2, x = e.to > e.from ? X + W + 10 * e.lane : X - 10 * (e.lane ?? 1);
      const d = e.to === e.from + 1 ? `M ${X + W / 2} ${y(e.from) + H} V ${y(e.to)}`
        : `M ${e.to > e.from ? X + W : X} ${y1} H ${x} V ${y2} H ${e.to > e.from ? X + W : X}`;
      g.append(svg("path", { d, class: e.count > 0 ? "e taken" : "e" }));
      if (e.count > 0 && e.to !== e.from + 1)
        g.append(svg("text", { x: e.to > e.from ? x + 3 : x - 3, y: (y1 + y2) / 2, class: "lbl", "text-anchor": e.to > e.from ? "start" : "end" }, `${e.count}`));
    }
    f.blocks.forEach((b, i) => {
      const here = v.stop?.func === f.name && v.stop.block === b.n;
      const blk = svg("g", { class: "blk" });
      blk.append(svg("rect", { x: X, y: y(i), width: W, height: H, rx: 4, class: (b.count > 0 ? "box hit" : "box") + (here ? " here" : "") }),
        svg("text", { x: X + 6, y: y(i) + 15 }, `${here ? "▶ " : ""}b${b.n}  ${b.file === f.file ? "" : b.file.split("/").pop() + ":"}${b.line}:${b.column}`),
        svg("text", { x: X + W - 6, y: y(i) + 15, "text-anchor": "end", class: "lbl" }, b.count ? `${b.count}×` : "–"));
      blk.onclick = () => {
        tab("src");
        showFile(b.file, b.line);
      };
      g.append(blk);
    });
    return el("div", {}, el("div", { className: "note", textContent: `${f.name}: basic blocks in order. Green: ran. Thick: branches taken, with counts. Click a block for its source.` }), g);
  }

  function showFunction(name, syscall, focus) {
    const [f] = m.functions(name, syscall);
    if (!f) return;
    v.fn = f;
    v.syscall = syscall;
    if (v.tab === "cfg") return $("code").replaceChildren(cfg(f));
    showFile(f.file, focus);
  }
  function tab(t) {
    v.tab = t;
    $("tab-src").classList.toggle("on", t === "src");
    $("tab-cfg").classList.toggle("on", t === "cfg");
  }
  for (const t of ["src", "cfg"])
    $(`tab-${t}`).onclick = () => {
      tab(t);
      if (v.fn) showFunction(v.fn.name, v.syscall);
    };
  return { view: v, showFunction, showFile };
}
