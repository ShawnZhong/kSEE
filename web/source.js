// The source view: a whole kernel file in CodeMirror (web/editor.js), with
// the lines that ran (green; how often: hover the line), the function's
// lines, and the cursor. Hover a name for its value at the cursor (an
// argument, a global, and fields: file->f_pos), or, for a type, a field
// or a global with no value here, its declaration's first lines; F12 or
// Ctrl+click on a
// name goes to its definition (a function, inline ones too, a type, a
// global, a field: Machine.definition); F4 runs to the line (Shift+F4:
// back to its last run).
import { $, el } from "./dom.js";
import { storeText } from "./stores.js";
import { tree, openable } from "./tree.js";
import { CM, base } from "./editor.js";

const KEYWORDS = new Set(("auto break case char const continue default do double else enum extern float for goto if " +
  "inline int long register restrict return short signed sizeof static struct switch typedef union unsigned void " +
  "volatile while bool true false NULL").split(" "));
const WORD = /[A-Za-z0-9_]/;

// The expression at offset `k` of `text`: the name there, with the fields
// before it ("file->f_pos" on f_pos); null if not a name.
function exprAt(text, k) {
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
  // A file's state (its document), kept once made.
  const states = new Map();

  // ---- The marks: a state field of line decorations, set as a whole ----
  const setMarks = CM.StateEffect.define();
  const marks = CM.StateField.define({
    create: () => CM.Decoration.none,
    update(set, tr) {
      for (const e of tr.effects) if (e.is(setMarks)) return e.value;
      return set.map(tr.changes);
    },
    provide: (f) => CM.EditorView.decorations.from(f),
  });
  // The cursor's ▶, in a gutter of its own.
  const cursorMark = new (class extends CM.GutterMarker { toDOM() { return el("span", { className: "gl-cursor" }); } })();
  const cursorGutter = CM.gutter({
    class: "cm-cursorGutter",
    lineMarker: (view, line) => (v.cursor?.file === v.file && view.state.doc.lineAt(line.from).number === v.cursor.line ? cursorMark : null),
    lineMarkerChange: (update) => update.transactions.some((tr) => tr.effects.some((e) => e.is(setMarks))),
  });
  // ---- Hover: the value at the cursor; else a declaration, its first lines ----
  const tips = CM.hoverTooltip(async (view, pos) => {
    const line = view.state.doc.lineAt(pos);
    const h = hover(line.number, line.text, pos - line.from) ?? await declaration(line.text, pos - line.from);
    return h && { pos: line.from + h.start, end: line.from + h.end, above: true,
                  create: () => ({ dom: h.o ? valueTip(h) : el("pre", { className: "hovertip mono", textContent: h.text }) }) };
  }, { hoverTime: 200 });
  // A value's hover: it, and its fields (what it points to) as the Stack's
  // tree, each opening, as memory was at the cursor; the hover stays while
  // the pointer is in it.
  function valueTip(h) {
    const st = { open: new Set(), last: new Map(), now: new Map() }, body = el("tbody");
    const { rows, kids } = tree(app, st, draw);
    function draw() {
      st.last = st.now;
      st.now = new Map();
      const out = [], k = openable(h.o) ? kids(h.o) : null;
      if (k) rows(k, "tip", 0, out, null, `${h.expr}${h.o.target != null ? "->" : "."}`);
      body.replaceChildren(...out);
    }
    draw();
    return el("div", { className: "hovertip valuetip" }, el("div", { className: "mono", textContent: h.text }), el("table", { className: "objs" }, body));
  }
  // The declaration of the name at `k` of `text` (a type, a field, a global,
  // an inline function: not a value the recording has): where, and its
  // first lines, for a hover; null if none.
  async function declaration(text, k) {
    const e = exprAt(text, k), w = e && text.slice(e.start, e.end);
    const d = w && !m.functions(w).length && definition(w, e.expr);
    if (!d) return null;
    const src = await fetchFile(d.file);
    if (!src) return null;
    const lines = src.split("\n").slice(d.line - 1, d.line + 7);
    return { start: e.start, end: e.end, text: [`${d.file}:${d.line}`, ...lines].join("\n") };
  }

  // ---- Keys and clicks: definitions, run to a line ----
  const lineOf = (view) => view.state.doc.lineAt(view.state.selection.main.head).number;
  const keys = CM.keymap.of([
    { key: "F12", run: (view) => (definitionAt(view.state.selection.main.head), true) },
    { key: "F4", run: (view) => (app.runTo(v.file, lineOf(view), 1), true) },
    { key: "Shift-F4", run: (view) => (app.runTo(v.file, lineOf(view), -1), true) },
  ]);
  const clicks = CM.EditorView.domEventHandlers({
    mousedown(e, view) {
      if (!(e.ctrlKey || e.metaKey)) return false;
      const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
      if (pos != null) definitionAt(pos);
      return true;
    },
  });

  const ed = new CM.EditorView({ parent: $("src"), state: CM.EditorState.create({ doc: "", extensions: base }) });
  $("src").querySelector(".empty")?.remove();

  function state(file, text) {
    if (!states.has(file)) states.set(file, CM.EditorState.create({ doc: text, extensions: [base, marks, cursorGutter, tips, keys, clicks] }));
    return states.get(file);
  }

  // Whether line `n` of the shown file is in the shown function.
  function inFunction(n) {
    const fn = v.fn?.file === v.file ? v.fn : null;
    return fn && n >= fn.line && n <= fn.last;
  }

  // The values of the expressions on line `line` (`text`), those the
  // Stack has (see hover), each once: "fd = 3  count = 64", or null.
  function values(line, text) {
    const out = [], seen = new Set();
    for (let k = 0; k < text.length; k++) {
      const e = exprAt(text, k);
      if (!e) continue;
      k = e.end - 1;
      if (seen.has(e.expr)) continue;
      seen.add(e.expr);
      const o = app.stack.resolve(e.expr, inFunction(line) ? v.fn.name : null);
      if (o && o.value != null && o.value !== "(optimized out)") out.push(`${e.expr} = ${o.value}`);
    }
    return out.length ? out.join("   ") : null;
  }
  // What a line stored, at its end, within the selected call up to the
  // cursor: its last store (the selection's stores by line, once per
  // selection), and how many it made; a link to that store.
  function stored(file) {
    const w = app.selection(), cur = app.time.cursor();
    if (!w || cur == null) return new Map();
    const key = w.key;
    if (v.stores?.key !== key) {
      const byLine = new Map();
      for (const s of app.selectionStores()) {
        if (!s.write.at) continue;
        const k = `${s.write.at.file}:${s.write.at.line}`;
        (byLine.get(k) ?? byLine.set(k, []).get(k)).push(s);
      }
      v.stores = { key, byLine };
    }
    const out = new Map();
    for (const [k, list] of v.stores.byLine) {
      const cut = k.lastIndexOf(":");
      if (k.slice(0, cut) !== file) continue;
      const done = list.filter((s) => s.write.i <= cur);
      if (done.length) out.set(Number(k.slice(cut + 1)), { s: done.at(-1), n: done.length });
    }
    return out;
  }
  // What the calls a line made returned, those that returned by the
  // cursor (the selection's line steps of the function shown): "vfs_read()
  // = 18", its last; a call made by code inlined into it, at the line it is
  // inlined at.
  function called(file) {
    const cur = app.time.cursor(), out = new Map();
    if (cur == null || !v.fn) return out;
    for (const s of app.lineSteps?.() ?? []) {
      if (s.i > cur) break;
      // (Once it returned, by the cursor.)
      if (!s.called || s.called[2] > cur || s.func !== v.fn.name) continue;
      const [at, where] = s.inlined ? [s.site, v.fn.file] : [s.line, s.file];
      if (where === file && at != null) out.set(at, `${s.called[0]}() = ${app.kernel.result(s.called[1])}`);
    }
    return out;
  }
  class Note extends CM.WidgetType {
    constructor(text) {
      super();
      this.text = text;
    }
    eq(other) {
      return other.text === this.text;
    }
    toDOM() {
      return el("span", { className: "note", textContent: this.text });
    }
  }
  class Stored extends CM.WidgetType {
    constructor(s, n) {
      super();
      this.s = s;
      this.n = n;
    }
    eq(other) {
      return other.s.write.i === this.s.write.i && other.n === this.n;
    }
    ignoreEvent() {
      return true;
    }
    toDOM() {
      // (Short: the member, or the variable, and the change; the line says
      // the rest. In full on hover, and in the Stores.)
      const x = this.s.write, what = x.member || this.s.what.split(/\W/).pop();
      return el("a", { className: "stored", textContent: `${what}: ${x.before} → ${x.after}${this.n > 1 ? ` ×${this.n}` : ""}`,
                       title: `${storeText(this.s)}: what this line stored, last; click: go to that store`, onclick: () => app.time.go(x.i) });
    }
  }
  // The cursor line's notes, a line of their own under it, as indented as
  // its code (`indent`, in columns): room for all of them.
  class Values extends CM.WidgetType {
    constructor(text, indent) {
      super();
      this.text = text;
      this.indent = indent;
    }
    eq(other) {
      return other.text === this.text && other.indent === this.indent;
    }
    toDOM() {
      return el("div", { className: "values", textContent: this.text, style: `padding-left: ${this.indent}ch` });
    }
  }
  // The columns `text`'s leading white space takes.
  const indentOf = (text, tab) => [...text.match(/^\s*/)[0]].reduce((c, ch) => (ch === "\t" ? c + tab - (c % tab) : c + 1), 0);
  // hover: { start, end, text }, or null.
  function hover(line, text, k) {
    const e = exprAt(text, k);
    // (Arguments only in the shown function's own lines; elsewhere, globals.)
    const o = e && app.stack.resolve(e.expr, inFunction(line) ? v.fn.name : null);
    if (!o) return null;
    // (The value, and to open: its fields, and what it points to, a tree.)
    return { start: e.start, end: e.end, text: `${e.expr} = ${o.value ?? "{…}"}  // ${o.ty}`, o, expr: e.expr };
  }
  // The definition of `word` (`expr`: the expression it ends, for a field:
  // file->f_pos): { file, line, func? (an instrumented function: its
  // source, with the lines that ran) }, or null.
  function definition(word, expr) {
    const parent = expr?.match(/^(.*\S)\s*(?:->|\.)\s*\w+$/)?.[1];
    // (A field: of the struct the expression before it is, by name.)
    const owner = parent && app.stack.resolve(parent)?.ty.match(/^(?:const )?(?:struct|union) (\w+)/)?.[1];
    // (A field of a value the stack does not have: of any struct with it.)
    const d = m.definition(owner ? `${owner}.${word}` : parent ? `.${word}` : word);
    return d && { file: d.file, line: d.line, func: !owner && m.functions(word).length ? word : null };
  }
  // Go to the definition of the name at `pos` of the shown file (a
  // function: its source, with the lines that ran; else the file, at the
  // line). Whether there was one.
  function definitionAt(pos) {
    const doc = ed.state.doc, line = doc.lineAt(pos), w = ed.state.wordAt(pos);
    if (!w) return false;
    const word = doc.sliceString(w.from, w.to), jump = definition(word, exprAt(line.text, pos - line.from)?.expr);
    if (!jump) return false;
    if (jump.func) showFunction(jump.func);
    else {
      v.fn = null;
      showFile(jump.file, jump.line);
    }
    return true;
  }

  // (A name with a value at the cursor: see decorate.)
  const valued = CM.Decoration.mark({ class: "has-value" });
  // The marks for the shown file: the lines that ran, the function, the
  // cursor.
  function decorate() {
    const file = v.file, fn = v.fn?.file === file ? v.fn : null, doc = ed.state.doc;
    const at = (n) => n >= 1 && n <= doc.lines && doc.line(n).from;
    const where = v.syscall != null ? " in this syscall" : " in the recording";
    const d = [];
    for (const [n, c] of v.counts) {
      if (at(n) !== false) d.push(CM.Decoration.line({ class: "ln-hit", attributes: { title: `ran ${c}×${where}` } }).range(at(n)));
    }
    if (fn) {
      for (let n = fn.line; n <= Math.min(fn.last, doc.lines); n++) d.push(CM.Decoration.line({ class: "ln-infn" }).range(at(n)));
      // (The names that have a value at the cursor, the function's arguments
      // and locals on the Stack: underlined, a hover shows them; not a
      // member's name, after . or ->.)
      const names = new Set(app.stack.frames().filter((f) => f.func === fn.name && !f.inlined)
        .flatMap((f) => [...(f.args ?? []), ...(f.locals ?? [])]).filter((o) => o.value !== "(optimized out)").map((o) => o.name));
      for (let n = fn.line; names.size && n <= Math.min(fn.last, doc.lines); n++) {
        const line = doc.line(n);
        for (const w of line.text.matchAll(/[A-Za-z_]\w*/g)) {
          if (names.has(w[0]) && !/(\.|->)\s*$/.test(line.text.slice(0, w.index))) d.push(valued.range(line.from + w.index, line.from + w.index + w[0].length));
        }
      }
    }
    if (v.cursor?.file === file && at(v.cursor.line) !== false) {
      d.push(CM.Decoration.line({ class: "ln-cursor" }).range(at(v.cursor.line)));
      // (The line's values, under it: as a debugger shows them; and a note,
      // what the call returns there.)
      const line = doc.line(v.cursor.line), text = [v.cursor.note, values(v.cursor.line, line.text)].filter(Boolean).join("   ");
      if (text) d.push(CM.Decoration.widget({ widget: new Values(text, indentOf(line.text, ed.state.tabSize)), block: true, side: 1 }).range(line.to));
    }
    // (What each line stored so far, at its end; and what the calls it
    // made returned.)
    for (const [n, { s, n: count }] of stored(file)) {
      if (at(n) !== false) d.push(CM.Decoration.widget({ widget: new Stored(s, count), side: 2 }).range(doc.line(n).to));
    }
    for (const [n, text] of called(file)) {
      if (at(n) !== false) d.push(CM.Decoration.widget({ widget: new Note(text), side: 3 }).range(doc.line(n).to));
    }
    d.sort((x, y) => x.from - y.from || x.value.startSide - y.value.startSide);
    ed.dispatch({ effects: setMarks.of(CM.Decoration.set(d, true)) });
  }
  // Show line `n`: as a debugger does, a jump (`jump`: another file or
  // function, a link) puts it a third of the way down, with the lines
  // before it for context; stepping leaves the view alone while the line
  // is in view (only the mark moves), and puts it at that third when it
  // leaves the view.
  function reveal(n, jump) {
    const doc = ed.state.doc;
    if (n < 1 || n > doc.lines) return;
    const pos = doc.line(n).from;
    ed.dispatch({ selection: { anchor: pos } });
    const box = ed.scrollDOM, block = ed.lineBlockAt(pos), top = block.top - box.scrollTop, h = box.clientHeight;
    if (!jump && top >= 0 && top + block.height <= h) return;
    ed.dispatch({ effects: CM.EditorView.scrollIntoView(pos, { y: "start", yMargin: Math.max(0, h / 3) }) });
  }

  let was = { file: null, fn: null }; // (what the view showed last: a jump, or a step)
  async function showFile(file, focus) {
    v.file = file;
    const text = await fetchFile(file);
    if (v.file !== file) return; // changed while loading
    // (Another file: its state in; the one shown keeps its own, as it is
    // now, for when it comes back. The same file: nothing re-rendered.)
    if (v.shown !== file) {
      if (v.shown != null) states.set(v.shown, ed.state);
      ed.setState(state(file, text ?? `// ${file}: not in this site's sources.`));
      v.shown = file;
    }
    v.counts = new Map(text === null ? [] : m.file_coverage(file, v.syscall).map((c) => [c.line, c.count]));
    decorate();
    const fn = v.fn?.file === file ? v.fn : null;
    // (A jump: another file, or another function; else a step in the same one.)
    const jump = file !== was.file || fn?.name !== was.fn;
    was = { file, fn: fn?.name };
    reveal(focus ?? fn?.line ?? 1, jump);
    const ran = !fn ? "" : ` · ${fn.name}` + (fn.sealed ? ": sealed (it loops, only reading memory: its call and return are recorded, not its lines)" : "");
    $("fn-title").textContent = file + ran;
  }

  // Function `name` (the lines that ran: in syscall `syscall`), in `file`
  // (a block's own, when it is inlined from a header) at line `focus`.
  function showFunction(name, syscall, focus, file) {
    const [f] = m.functions(name);
    if (!f) return;
    v.fn = f;
    v.syscall = syscall;
    showFile(file ?? f.file, focus);
  }

  // The shown file's lines (for tests), and the first line at or after
  // `from` with `needle` in it: { line, column (of needle) }, or null.
  const text = () => ed.state.doc.toString();
  function find(needle, from = 1) {
    const doc = ed.state.doc;
    for (let n = from; n <= doc.lines; n++) {
      const k = doc.line(n).text.indexOf(needle);
      if (k >= 0) return { line: n, column: k + 1 };
    }
    return null;
  }
  // For tests: the hover at the first `needle` of the shown function (and
  // after it), on `word`.
  async function hoverAt(needle, word) {
    const at = find(needle, v.fn?.file === v.file ? v.fn.line : 1);
    if (!at) return null;
    const text = ed.state.doc.line(at.line).text, k = at.column - 1 + needle.indexOf(word);
    return (hover(at.line, text, k) ?? await declaration(text, k))?.text ?? null;
  }
  // For tests: go to the definition of the name at `line`, `column`.
  const definitionAtLine = (line, column) => definitionAt(ed.state.doc.line(line).from + column - 1);

  // (`editor`: CodeMirror's view, for tests: a real hover.)
  return { view: v, showFunction, hoverAt, text, find, definitionAt: definitionAtLine,
           lineAt: () => ed.state.doc.lineAt(ed.state.selection.main.head).number };
}
