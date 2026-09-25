// The source view: a whole kernel file in Monaco (web/editor.js), with the
// lines where blocks ran (how often: hover the line number; untaken ones
// marked), the
// function's lines, and the cursor. Hover a name for its value at the
// cursor (an argument, a global, and fields: file->f_pos); F12 or
// Ctrl+click on a function to open its source.
import { $ } from "./dom.js";
import { loadMonaco, readOnly } from "./editor.js";

const KEYWORDS = new Set(("auto break case char const continue default do double else enum extern float for goto if " +
  "inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void " +
  "volatile while bool true false NULL").split(" "));
const WORD = /[A-Za-z0-9_]/;

// The expression at offset `k` of `text`: the name there, with the fields
// before it ("file->f_pos" on f_pos); null if not a name.
export function exprAt(text, k) {
  if (!WORD.test(text[k] ?? "")) return null;
  let a = k, b = k;
  while (a > 0 && WORD.test(text[a - 1])) a--;
  while (b < text.length && WORD.test(text[b])) b++;
  let expr = text.slice(a, b);
  if (/^\d/.test(expr) || KEYWORDS.has(expr)) return null;
  for (;;) {
    const m = text.slice(0, a).match(/([A-Za-z_]\w*)\s*(->|\.)\s*$/);
    if (!m) break;
    expr = `${m[1]}${m[2]}${expr}`;
    a -= m[0].length;
  }
  return { expr, start: a, end: b };
}

export function source(app) {
  const { m } = app;
  const texts = new Map();
  const fetchFile = (file) => {
    if (!texts.has(file))
      texts.set(file, file.startsWith("/") ? Promise.resolve(null)
        : fetch(new URL(`./src/${file}`, location)).then((r) => (r.ok ? r.text() : null)).catch(() => null));
    return texts.get(file);
  };
  // What is shown, and the cursor's line.
  const v = { file: null, fn: null, syscall: undefined, cursor: null, counts: new Map() };
  let monaco, ed, marks;
  const models = new Map();

  const ready = loadMonaco().then((mo) => {
    monaco = mo;
    $("src").replaceChildren();
    ed = monaco.editor.create($("src"), {
      ...readOnly,
      glyphMargin: true,
      lineNumbersMinChars: 4,
    });
    marks = ed.createDecorationsCollection();
    // Ctrl+click on a function: its source.
    ed.onMouseDown((e) => {
      if (e.target.type === monaco.editor.MouseTargetType.CONTENT_TEXT && (e.event.ctrlKey || e.event.metaKey))
        definition(e.target.position);
    });
    // Hover: the value at the cursor.
    monaco.languages.registerHoverProvider("c", {
      provideHover(model, pos) {
        if (model !== ed.getModel()) return null;
        const h = hover(pos.lineNumber, model.getLineContent(pos.lineNumber), pos.column - 1);
        return h && { range: new monaco.Range(pos.lineNumber, h.start + 1, pos.lineNumber, h.end + 1), contents: h.contents };
      },
    });
    ed.addAction({ id: "ksee.definition", label: "Go to the function's source", contextMenuGroupId: "navigation",
      contextMenuOrder: 1, keybindings: [monaco.KeyCode.F12], run: () => definition(ed.getPosition()) });
  });

  // Whether line `n` of the shown file is in the shown function.
  function inFunction(n) {
    const fn = v.fn?.file === v.file ? v.fn : null;
    return fn && n >= fn.line && n <= Math.max(fn.line, ...fn.blocks.filter((b) => b.file === v.file).map((b) => b.line));
  }

  // The value of the expression at `k` in line `line` (its `text`), for a
  // hover: { start, end, text, contents (markdown) }, or null.
  function hover(line, text, k) {
    const e = exprAt(text, k);
    // (Arguments only in the shown function's own lines; elsewhere, globals.)
    const o = e && app.stack.resolve(e.expr, inFunction(line) ? v.fn.name : null);
    if (!o) return null;
    const fields = o.value == null || (o.target != null && o.value !== "NULL") ? (app.stack.kids(o) ?? []).slice(0, 10) : [];
    const code = [`${e.expr} = ${o.value ?? "{…}"}  // ${o.ty}`, ...fields.map((c) => `  .${c.name} = ${c.value ?? "{…}"}`)].join("\n");
    return { start: e.start, end: e.end, text: code,
             contents: [{ value: "```c\n" + code + "\n```" }] };
  }
  // A function named at `pos`: its source.
  function definition(pos) {
    const word = ed.getModel().getWordAtPosition(pos)?.word;
    if (word && m.functions(word).length) showFunction(word);
  }

  function model(file, text) {
    if (!models.has(file)) models.set(file, monaco.editor.createModel(text, "c", monaco.Uri.parse(`kernel:///${file}`)));
    return models.get(file);
  }

  // The markers for the shown file.
  function decorate() {
    const file = v.file, fn = v.fn?.file === file ? v.fn : null;
    const last = fn ? Math.max(fn.line, ...fn.blocks.filter((b) => b.file === file).map((b) => b.line)) : 0;
    const at = (x) => x && x.file === file && x.line;
    const line = (n, options) => ({ range: new monaco.Range(n, 1, n, 1), options });
    const d = [];
    const where = v.syscall != null ? " in this syscall" : " in the recording";
    for (const [n, c] of v.counts)
      d.push(line(n, c > 0
        ? { isWholeLine: true, className: "ln-hit", overviewRuler: { color: "#2da44e88", position: 4 }, minimap: { color: "#2da44e88", position: 1 },
            lineNumberHoverMessage: { value: `ran ${c}×${where}` } }
        : { linesDecorationsClassName: "ln-miss", lineNumberHoverMessage: { value: `a block starts here; it did not run${where}` } }));
    if (fn) d.push({ range: new monaco.Range(fn.line, 1, last, 1), options: { isWholeLine: true, linesDecorationsClassName: "ln-infn" } });
    if (at(v.cursor)) d.push(line(v.cursor.line, { isWholeLine: true, className: "ln-cursor", glyphMarginClassName: "gl-cursor",
      overviewRuler: { color: "#0969da", position: 7 } }));
    marks.set(d);
  }

  async function showFile(file, focus) {
    v.file = file;
    v.focus = focus;
    const [text] = await Promise.all([fetchFile(file), ready]);
    if (v.file !== file) return; // changed while loading
    const mdl = model(file, text ?? `// ${file}: not in this site's sources.`);
    if (ed.getModel() !== mdl) ed.setModel(mdl);
    v.counts = new Map(text === null ? [] : m.file_coverage(file, v.syscall).map((c) => [c.line, c.count]));
    decorate();
    const fn = v.fn?.file === file ? v.fn : null;
    const to = focus ?? fn?.line ?? 1;
    if (focus) ed.revealLineInCenterIfOutsideViewport(to);
    else ed.revealLineNearTop(to);
    const ran = fn ? ` · ${fn.name}: ${fn.blocks.filter((b) => b.count > 0).length} of ${fn.blocks.length} blocks ran` : "";
    $("fn-title").textContent = (saved ? "preview · " : "") + file + ran;
    $("fn-title").classList.toggle("preview", !!saved);
  }

  // Function `name` (its blocks' counts from syscall `syscall`), in `file`
  // (a block's own, when it is inlined from a header) at line `focus`.
  function show(name, syscall, focus, file) {
    const [f] = m.functions(name, syscall);
    if (!f) return;
    v.fn = f;
    v.syscall = syscall;
    showFile(file ?? f.file, focus);
  }
  function showFunction(name, syscall, focus, file) {
    saved = null;
    show(name, syscall, focus, file);
  }

  // A preview (hovering a call): function `func` at `file:line`, until
  // unpreview goes back to what was shown (or another showFunction keeps it).
  let saved = null;
  function preview(func, file, line) {
    saved ??= { fn: v.fn, syscall: v.syscall, cursor: v.cursor, file: v.file, focus: v.focus, top: ed?.getScrollTop() };
    v.cursor = { file, line };
    show(func, v.syscall, line, file);
  }
  async function unpreview() {
    if (!saved) return;
    const was = saved;
    saved = null;
    Object.assign(v, { fn: was.fn, syscall: was.syscall, cursor: was.cursor });
    if (was.file) await showFile(was.file, was.focus);
    if (!saved && was.top != null) ed.setScrollTop(was.top);
  }

  // For tests: the hover at the first `needle` of the shown function (and
  // after it), on `word`.
  async function hoverAt(needle, word) {
    await ready;
    const mdl = ed.getModel();
    // (From the shown function's first line.)
    for (let n = v.fn?.file === v.file ? v.fn.line : 1; n <= mdl.getLineCount(); n++) {
      const text = mdl.getLineContent(n), k = text.indexOf(needle);
      if (k >= 0) return hover(n, text, k + needle.indexOf(word))?.text ?? null;
    }
    return null;
  }

  return { view: v, showFunction, showFile, preview, unpreview, hoverAt, ready };
}
