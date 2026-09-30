// A syscall, written as in C: `openat(AT_FDCWD, "/mnt/x", O_CREAT, 0644)`,
// `int fd = openat(…)`, `19: read(fd, buf, 64)`, or a variable, `char
// buf[64]` (see ksee-core's program.rs: Machine.statement). As you type:
// a list of what can go at the cursor (syscall names; then, in an
// argument, the results so far by name, and the kernel's constants, those
// that fit the parameter first (Machine.syscall says which)), ↑ ↓ to
// choose, Tab or Enter to take one; below, the syscall's signature, the
// parameter at the cursor in bold. Enter (with no list open) makes the
// call (a program's line emptied: removes it, `done(null)`); Esc closes
// the list, or gives up; ↑ ↓ with nothing written: the calls before
// (`history`), as a shell's.
import { el } from "./dom.js";

// { el, input, focus }: `text` to start with; `known(runners?)`: the
// results of the calls before, by name ({ fd: 3 }; `runners`: for
// a runner, see examples.js); `done(call)`: a call made
// of what was written ({ call, args, name?, tid? }); `cancel()`: Esc with
// no list open; `focus()`: it gets the focus; `inline`: a line of a
// program (its hint only while it has the focus); `common`: the syscalls
// to list first, and with nothing written yet; `history()`: calls to
// recall with ↑.
export function callEditor({ m, consts, known, text = "", placeholder = "", done, cancel, focus, inline = false, common = [], history }) {
  const names = m.syscall_names();
  // (No syscall `name` this kernel builds: those whose names start with
  // it, if any: openat for open.)
  const noSyscall = (name) => {
    const like = names.filter((n) => n.startsWith(name)).slice(0, 3);
    return `this kernel has no syscall ${name}${like.length ? `: ${like.join(", ")}?` : ""}`;
  };
  const constNames = Object.keys(consts);
  // (A text area of one line, as tall as its text wraps to.)
  const input = el("textarea", { className: "mono", value: text, placeholder, spellcheck: false, autocomplete: "off", rows: 1 });
  const fit = () => {
    input.style.height = "auto";
    input.style.height = `${input.scrollHeight}px`;
  };
  requestAnimationFrame(fit);
  const list = el("ul", { className: "complete", hidden: true });
  const hint = el("div", { className: "mono dim hint" });
  let items = [], sel = 0;

  // Where the cursor is (Machine.context): { call (the syscall's name,
  // as far as written), call_start, word (at the cursor), word_start,
  // param (which argument it is in, if in them) }.
  const context = () => m.context(input.value, input.selectionStart ?? input.value.length);

  // What fits at the cursor: [{ text (to put in), label, detail }].
  function suggest(c) {
    if (c.param == null) {
      const w = c.call;
      // (Every one that fits, the list scrolling: the common ones first,
      // then those starting with it, then those with it inside; nothing
      // written: all of them, the common ones first.)
      const starts = names.filter((n) => n.startsWith(w)), inside = w ? names.filter((n) => !n.startsWith(w) && n.includes(w)) : [];
      return [...new Set([...common.filter((n) => n.startsWith(w)), ...starts, ...inside])]
        .map((n) => ({ text: `${n}(`, label: n, detail: signatureText(n), name: true }));
    }
    if (!c.word || /^\d/.test(c.word)) return [];
    // (The results so far, by name, first; then the constants that fit.)
    const results = Object.entries(known()).filter(([n]) => n.startsWith(c.word))
      .map(([n, r]) => ({ text: n, label: n, detail: `${typeof r !== "object" ? `= ${r}` : r.addr != null ? "a variable" : `= 0x${r.value.toString(16)}`}${n.startsWith("$") ? `: call ${n.slice(1)}'s result` : ""}` }));
    if (c.word.startsWith("$")) return results;
    const p = m.syscall(c.call)?.params?.[c.param];
    const fits = p?.consts ?? [];
    const rank = (n) => (fits.some((f) => n.startsWith(f)) ? 0 : 1);
    return [...results, ...constNames.filter((n) => n.startsWith(c.word)).sort((a, b) => rank(a) - rank(b) || a.localeCompare(b))
      .map((n) => ({ text: n, label: n, detail: String(consts[n]) }))];
  }
  // (A syscall's signature as text, once each: the list shows it for all.)
  const signatures = new Map();
  const signatureText = (n) => signatures.get(n) ?? signatures.set(n, signature(n)?.map((p) => p.textContent ?? p).join("")).get(n);

  function signature(name, bold = -1) {
    const s = m.syscall(name);
    if (!s) return null;
    const ps = s.params.map((p, i) => {
      const t = `${p.ty}${p.ty.endsWith("*") ? "" : " "}${p.name}`;
      return i === bold ? el("b", { textContent: t }) : t;
    });
    return [`${name}(`, ...ps.flatMap((p, i) => (i ? [", ", p] : [p])), ")"];
  }

  function show() {
    const c = context();
    items = suggest(c);
    sel = Math.min(sel, Math.max(0, items.length - 1));
    // (Nothing to choose from, or only what is written already.)
    const open = items.length > 0 && !(items.length === 1 && items[0].label === (c.param == null ? c.call : c.word));
    list.hidden = !open;
    list.replaceChildren(...(open ? items : []).map((it, k) => el("li", {
      className: k === sel ? "on" : "", onmousedown: (e) => (e.preventDefault(), take(k)) },
      el("span", { className: "mono", textContent: it.label }), " ",
      el("span", { className: "dim", textContent: it.detail ?? "" }))));
    // (The one chosen in view, as ↑ ↓ move it.)
    list.children[sel]?.scrollIntoView({ block: "nearest" });
    const sig = c.call && signature(c.call, c.param ?? -1);
    hint.replaceChildren(...(sig ? sig : c.call && c.param != null ? [noSyscall(c.call)] : []));
  }

  // Take suggestion `k`: it replaces the word at the cursor (or the name).
  function take(k) {
    const it = items[k], c = context(), v = input.value, end = input.selectionStart ?? v.length;
    const start = it.name ? c.call_start : c.word_start;
    input.value = v.slice(0, start) + it.text + v.slice(end);
    fit();
    const at = start + it.text.length;
    input.setSelectionRange(at, at);
    sel = 0;
    show();
  }

  // The line written: a call { call, args, name?, tid? }, a declaration {
  // decl }, an assignment { target, value }; or an error.
  function written() {
    // (A call, or a variable of the program's, declared as in C.)
    let c;
    try {
      c = m.statement(input.value);
      if (c.decl) return (m.declaration(c.text, known()), c);
      if (c.target != null) return c; // (an assignment: checked as it is made)
    } catch (e) {
      return `${e.message ?? e}: write a call, name(arg, …) or int fd = name(…); or a variable, char buf[64]`;
    }
    const s = m.syscall(c.call);
    if (!s) return noSyscall(c.call);
    if (c.args.length > s.params.length) return `${c.call}: ${s.params.length} arguments, not ${c.args.length}`;
    try {
      // (Checked now, with the results so far.)
      if (c.tid != null) {
        const tid = typeof c.tid === "number" ? c.tid : m.value(c.tid, known(true));
        if (!m.runners().includes(tid)) return `no runner ${c.tid}${tid !== c.tid ? ` (${tid})` : ""}: the runners are ${[...m.runners()].join(", ")}`;
      }
      c.args.forEach((a, k) => m.arg(c.call, k, a, known()));
    } catch (e) {
      return String(e.message ?? e);
    }
    return c;
  }

  input.oninput = () => ((sel = 0), fit(), show());
  // (As the cursor moves too: the parameter it is in.)
  input.onfocus = () => (requestAnimationFrame(show), focus?.());
  input.onclick = show;
  input.onkeyup = (e) => e.key.startsWith("Arrow") || e.key === "Home" || e.key === "End" ? show() : undefined;
  // (A program's line left: as the call is; Enter applies, nothing else.)
  input.onblur = () => ((list.hidden = true), inline && cancel?.());
  // (↑ ↓ with nothing written, or in a recalled call: the calls before.)
  let back = 0;
  input.onkeydown = (e) => {
    const open = !list.hidden;
    if (open && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      sel = (sel + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length;
      show();
    } else if (!open && history && (e.key === "ArrowUp" || e.key === "ArrowDown") && (back || !input.value)) {
      const h = history();
      back = Math.max(0, Math.min(h.length, back + (e.key === "ArrowUp" ? 1 : -1)));
      input.value = back ? h[h.length - back] : "";
      fit();
      show();
    } else if (e.key === "Tab") {
      // (Tab takes the one chosen, even with the list not shown yet.)
      if (!items.length) items = suggest(context());
      if (!items.length) return;
      take(open ? sel : 0);
    } else if (e.key === "Enter" && open) {
      take(sel);
    } else if (e.key === "Enter" && inline && !input.value.trim()) {
      done(null); // (emptied: the call goes)
    } else if (e.key === "Enter") {
      const c = written();
      if (typeof c === "string") return (hint.textContent = c, e.preventDefault());
      list.hidden = true;
      back = 0;
      done(c);
    } else if (e.key === "Escape") {
      if (open) list.hidden = true;
      else cancel?.();
    } else {
      return;
    }
    e.preventDefault();
  };
  return { el: el("div", { className: inline ? "calledit inline" : "calledit" }, input, list, hint), input, focus: () => input.focus() };
}
