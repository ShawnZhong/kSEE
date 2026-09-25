// The source view: a whole kernel file, with the lines where blocks ran
// (and how often), breakpoints, and where the kernel is stopped or where
// the history cursor is.
import { $, el } from "./dom.js";

const KEYWORDS = new Set(("auto break case char const continue default do double else enum extern float for goto if " +
  "inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void " +
  "volatile while bool true false NULL u8 u16 u32 u64 s8 s16 s32 s64 size_t ssize_t loff_t __user unlikely likely").split(" "));

// C, roughly: comments (also across lines), strings, preprocessor lines,
// keywords, numbers. One list of nodes per line.
export function highlight(lines) {
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
  const v = { file: null, fn: null, syscall: undefined, breaks: new Set(), stop: null, cursor: null };

  async function showFile(file, focus) {
    v.file = file;
    const text = await fetchFile(file);
    if (v.file !== file) return; // changed while loading
    if (text === null)
      return $("src").replaceChildren(el("div", { className: "empty", textContent: `${file}: not in this site's sources.` }));
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
    $("src").replaceChildren(el("table", { className: "src" }, el("tbody", {}, ...rows)));
    $("src").querySelector(`#L${focus ?? fn?.line ?? 1}`)?.scrollIntoView({ block: focus ? "center" : "start" });
    const ran = fn ? ` · ${fn.name}: ${fn.blocks.filter((b) => b.count > 0).length} of ${fn.blocks.length} blocks ran` : "";
    $("fn-title").textContent = file + ran;
  }

  // Function `name` (its blocks' counts from syscall `syscall`), in `file`
  // (a block's own, when it is inlined from a header) at line `focus`.
  function showFunction(name, syscall, focus, file) {
    const [f] = m.functions(name, syscall);
    if (!f) return;
    v.fn = f;
    v.syscall = syscall;
    showFile(file ?? f.file, focus);
  }
  return { view: v, showFunction, showFile };
}
