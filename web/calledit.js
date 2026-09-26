// A syscall, written as in C: `openat(AT_FDCWD, "/mnt/x", O_CREAT, 0644)`,
// or `19: read(3, [64], 64)` for runner 19 (see examples.js). As you type:
// a list of what can go at the cursor (syscall names; then, in an
// argument, the kernel's constants, those that fit the parameter first,
// and $n: the calls' results so far), ↑ ↓ to choose, Tab or Enter to take
// one; below, the syscall's signature, the parameter at the cursor in
// bold. Enter (with no list open) makes the call; Esc closes the list, or
// gives up.
import { el } from "./dom.js";
import { arg, parseCall } from "./args.js";

// The constants that fit a parameter, by their names' starts: for
// "syscall.param", or any syscall's parameter of that name.
const FITS = {
  "mmap2.flags": ["MAP_"], "openat.flags": ["O_"], "unlinkat.flag": ["AT_"], "renameat2.flags": ["RENAME_"],
  "waitid.which": ["P_"], "waitid.options": ["W"], "clone.clone_flags": ["CLONE_", "SIG"],
  "futex_time64.op": ["FUTEX_"], "clock_nanosleep_time64.which_clock": ["CLOCK_"], "clock_nanosleep_time64.flags": ["TIMER_"],
  dfd: ["AT_"], olddfd: ["AT_"], newdfd: ["AT_"], flags: ["O_"], prot: ["PROT_"], which_clock: ["CLOCK_"], op: ["FUTEX_"],
};

// { el, input, focus }: `text` to start with; `rets()`: the results of
// the calls before (for $n); `done(call)`: a call made of what was written
// ({ call, args, tid? }); `cancel()`: Esc with no list open; `focus()`:
// it gets the focus; `inline`: a line of a program (its hint only while
// it has the focus).
export function callEditor({ m, consts, rets, text = "", placeholder = "", done, cancel, focus, inline = false }) {
  const names = m.syscall_names();
  const constNames = [...Object.keys(consts), "NULL"];
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

  // Where the cursor is: { name, tid, start (of the word), word, param (its
  // index, in an argument) }.
  function context() {
    const before = input.value.slice(0, input.selectionStart ?? input.value.length);
    const pre = before.match(/^\s*(?:(\d+)\s*:\s*)?/);
    const rest = before.slice(pre[0].length), open = rest.indexOf("(");
    const word = before.match(/[$A-Za-z_]\w*$|\$$/)?.[0] ?? "";
    const at = { tid: pre[1], start: before.length - word.length, word };
    if (open < 0) return { ...at, name: rest.trim(), param: null };
    // (Which argument: the commas outside strings, () and {} before it.)
    let param = 0, depth = 0, quote = false;
    for (let i = open + 1; i < rest.length; i++) {
      const c = rest[i];
      if (quote) (c === "\\" ? i++ : c === '"' && (quote = false));
      else if (c === '"') quote = true;
      else if ("({".includes(c)) depth++;
      else if (")}".includes(c)) depth--;
      else if (c === "," && depth === 0) param++;
    }
    return { ...at, name: rest.slice(0, open).trim(), param };
  }

  // What fits at the cursor: [{ text (to put in), label, detail }].
  function suggest(c) {
    if (c.param == null) {
      const w = c.name;
      if (!w) return [];
      const starts = names.filter((n) => n.startsWith(w)), inside = names.filter((n) => !n.startsWith(w) && n.includes(w));
      return [...starts, ...inside].slice(0, 12).map((n) => ({ text: `${n}(`, label: n, detail: signature(n), name: true }));
    }
    if (c.word.startsWith("$")) {
      return rets().map((r, j) => ({ r, j })).filter(({ r, j }) => r !== undefined && `$${j + 1}`.startsWith(c.word))
        .map(({ r, j }) => ({ text: `$${j + 1}`, label: `$${j + 1}`, detail: `= ${r}: call ${j + 1}'s result` }));
    }
    if (!c.word || /^\d/.test(c.word)) return [];
    const p = m.syscall(c.name)?.params?.[c.param];
    const fits = FITS[`${c.name}.${p?.name}`] ?? FITS[p?.name] ?? [];
    const rank = (n) => (fits.some((f) => n.startsWith(f)) ? 0 : 1);
    return constNames.filter((n) => n.startsWith(c.word)).sort((a, b) => rank(a) - rank(b) || a.localeCompare(b))
      .slice(0, 12).map((n) => ({ text: n, label: n, detail: n === "NULL" ? "0" : String(consts[n]) }));
  }

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
    const open = items.length > 0 && !(items.length === 1 && items[0].label === (c.param == null ? c.name : c.word));
    list.hidden = !open;
    list.replaceChildren(...(open ? items : []).map((it, k) => el("li", {
      className: k === sel ? "on" : "", onmousedown: (e) => (e.preventDefault(), take(k)) },
      el("span", { className: "mono", textContent: it.label }), " ",
      el("span", { className: "dim", textContent: typeof it.detail === "string" ? it.detail : "" }))));
    const sig = c.name && signature(c.name, c.param ?? -1);
    const others = [...m.runners()].filter((r) => r !== 1);
    hint.replaceChildren(...(sig ? sig : c.name && c.param != null ? [`no syscall ${c.name}`]
      : others.length && !input.value ? [`runners: 1 (/init), ${others.join(", ")}: "${others[0]}: call(…)" for another`] : []));
  }

  // Take suggestion `k`: it replaces the word at the cursor (or the name).
  function take(k) {
    const it = items[k], c = context(), v = input.value, end = input.selectionStart ?? v.length;
    let start = c.start;
    if (it.name) start = v.slice(0, end).match(/^\s*(?:\d+\s*:\s*)?/)[0].length;
    input.value = v.slice(0, start) + it.text + v.slice(end);
    fit();
    const at = start + it.text.length;
    input.setSelectionRange(at, at);
    sel = 0;
    show();
  }

  // The call written: { call, args, tid? }, or an error.
  function written() {
    const [, tid, rest] = input.value.match(/^\s*(?:(\d+)\s*:)?\s*([\s\S]*)$/);
    const c = parseCall(rest);
    if (!c) return "write a call: name(arg, arg, …)";
    const s = m.syscall(c.call);
    if (!s) return `no syscall ${c.call}`;
    if (c.args.length > s.params.length) return `${c.call}: ${s.params.length} arguments, not ${c.args.length}`;
    if (tid && !m.runners().includes(Number(tid))) return `no runner ${tid}: the runners are ${[...m.runners()].join(", ")}`;
    try {
      // (Checked now: $n as if the calls before were made.)
      c.args.forEach((a) => arg(a, consts, rets().map((r) => r ?? 0)));
    } catch (e) {
      return e.message;
    }
    return { call: c.call, args: c.args, ...(tid && Number(tid) !== 1 ? { tid: Number(tid) } : {}) };
  }

  input.oninput = () => ((sel = 0), fit(), show());
  input.onfocus = () => (show(), focus?.());
  input.onblur = () => (list.hidden = true);
  input.onkeydown = (e) => {
    const open = !list.hidden;
    if (open && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      sel = (sel + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length;
      show();
    } else if (e.key === "Tab") {
      // (Tab takes the one chosen, even with the list not shown yet.)
      if (!items.length) items = suggest(context());
      if (!items.length) return;
      take(open ? sel : 0);
    } else if (e.key === "Enter" && open) {
      take(sel);
    } else if (e.key === "Enter") {
      const c = written();
      if (typeof c === "string") return (hint.textContent = c, e.preventDefault());
      list.hidden = true;
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
