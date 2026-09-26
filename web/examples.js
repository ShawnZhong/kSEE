// The examples, one at a time: a selector (and ◀ ▶) switches, each on a
// fresh kernel (the kernel starts again, the page stays); the URL says
// which (#ex=NAME). An example is a question about the kernel, a program
// that asks it (user/examples/*.json: syscalls, one after the other), and
// its explanation: paragraphs of text with links into the recording.
//
// Opening an example makes all its calls, each recorded (a runner makes
// each: /init, or a thread or child of it; see Machine.call). A call may
// block, and finish as a later one wakes it. Then the explanation's links
// lead into the recording: `[label](2)` is call 2 (its recording
// selected), `[label](2:vfs_read)` the first call of vfs_read in it (on
// the call's task; `:any` on any task), the cursor there; the first such
// link is where the cursor starts; `[label](try:1)` runs the example's
// `tries[0]`, another program (calls separated by ";"), instead: to see
// what changes. `code` in backticks. The program is
// below, a call a line: click one for its recording; type in it to change
// it (Enter: the program runs again, on a fresh kernel), empty it (or ✕)
// to remove it; add a call at the end; ↺ for the example's own program
// again.
//
// A step: { call, args, name?, tid? }; its arguments written as in
// C (see args.js and Machine.arg): "a string", 0644, O_CREAT | O_WRONLY,
// [64], {0, 30000000}, fd or $2 (a result: `name` names a call's). Its
// runner: thread `tid` (a number, or a result: "child", "$2"); else /init.
import { $, el } from "./dom.js";
import { made, parseCall, result } from "./args.js";
import { callEditor } from "./calledit.js";

export async function examples(app) {
  const { m } = app;
  const consts = m.constants();
  const list = await fetch(new URL("./examples.json", location)).then((r) => (r.ok ? r.json() : [])).catch(() => []);
  // The open example: its steps, the calls asked ({ id, tid, args, how?,
  // ret?, text?: how once it finished), the runner each started; its
  // explanation's links ({ j: the call, func?, any?, at?: the record, a:
  // the element }).
  const t = { ex: null, steps: [], done: [], started: [], links: [], list: null, text: null, status: null };

  // The URL: #ex=NAME, the example; &p=CALLS, its program if changed (the
  // calls as written, separated by ";": see program). { ex, steps, changed }.
  function fromURL() {
    const q = Object.fromEntries(location.hash.slice(1).split("&").filter(Boolean).map((kv) => {
      const k = kv.indexOf("=");
      return [kv.slice(0, k), decodeURIComponent(kv.slice(k + 1))];
    }));
    const ex = list.find((x) => x.name === q.ex) ?? list[0];
    const steps = q.p != null ? program(q.p) : (ex?.steps ?? []).map((s) => ({ ...s }));
    return { ex, steps, changed: q.p != null };
  }
  function toURL() {
    const p = t.changed ? `&p=${encodeURIComponent(t.steps.map((s) => text(s)).join("; "))}` : "";
    history.replaceState(null, "", `#ex=${t.ex.name}${p}`);
  }
  // Program `text` (calls as written, separated by ";" outside strings).
  function program(text) {
    const calls = [];
    let cur = "", quote = false;
    for (let i = 0; i < text.length; i++) {
      const c = text[i];
      if (quote && c === "\\") (cur += c + (text[++i] ?? ""));
      else if (c === '"') ((quote = !quote), (cur += c));
      else if (c === ";" && !quote) ((calls.push(cur)), (cur = ""));
      else cur += c;
    }
    calls.push(cur);
    return calls.map((c) => c.trim()).filter(Boolean).map((c) => parseCall(c) ?? { call: c, args: [] });
  }
  // Switch to example `ex` (its program `steps`, `changed` from the
  // example's: see change), on a fresh kernel, so that it always starts
  // from the same state; and make its calls.
  function go(ex, steps = ex.steps, changed = false) {
    open(ex, steps);
    t.changed = changed;
    t.reset.hidden = !changed;
    toURL();
    t.status.textContent = "Starting a fresh kernel…";
    app.restart().then(run);
  }

  function open(ex, steps = ex.steps) {
    t.ex = ex;
    // (The form adds to them. `as: k`, an older spelling: the runner call k started, $k.)
    t.steps = steps.map(({ as, ...s }) => (as ? { ...s, tid: `$${as}` } : { ...s }));
    t.done = [];
    t.started = [];
    t.links = [];
    t.changed = false;
    t.status = el("div", { className: "tstatus dim" });
    t.text = el("div", { className: "explain" });
    t.list = el("ol", { className: "steps" });
    const k = list.indexOf(ex);
    // (Grouped by their `group`, in order: Start here, Files, ...)
    const groups = [...new Set(list.map((x) => x.group ?? ""))];
    const pick = el("select", { className: "pick", title: "the examples (each on a fresh kernel)", onchange: () => go(list[pick.selectedIndex]) },
      ...groups.map((g) => el("optgroup", { label: g }, ...list.filter((x) => (x.group ?? "") === g)
        .map((x) => el("option", { textContent: x.title, selected: x === ex })))));
    // (The examples' navigation: in the panel's title.)
    $("ex-head").replaceChildren(el("b", { textContent: "kSEE" }),
      el("button", { textContent: "◀", title: "the example before", disabled: k <= 0, onclick: () => go(list[k - 1]) }), pick,
      el("button", { textContent: "▶", title: "the next example", disabled: k < 0 || k + 1 >= list.length, onclick: () => go(list[k + 1]) }));
    t.form = form();
    $("examples").replaceChildren(
      ex.question ? el("div", { className: "q", textContent: ex.question }) : null,
      t.text, el("div", { className: "ptitle dim" }, "Program", t.reset = el("a", { textContent: "↺ the example's", hidden: true,
        title: "the example's own program again (yours is in the URL until then)", onclick: () => go(ex) })), t.list, t.status);
    draw();
    explain();
  }

  // The kernel is up: make the calls, all of them; then the explanation's
  // links lead into the recording, the cursor at the first.
  async function run() {
    const mine = t.list;
    t.status.textContent = "Making the calls…";
    for (let j = 0; j < t.steps.length; j++) if (!(await step()) || t.list !== mine) return;
    t.status.textContent = "";
    explain();
    const first = t.links.find((l) => l.at != null) ?? t.links.find((l) => l.c);
    if (first) goTo(first);
    else if (t.done.length) pick(t.done.length - 1);
    // (A program of one's own: the prompt, ready.)
    if (!t.links.length) t.form.querySelector("textarea").focus();
  }
  // Change step `j` to `call`: the program runs again, on a fresh kernel (a
  // kernel cannot be put back as it was: its tasks' stacks are the
  // browser's). (The program is in the URL, &p=: a link to it.)
  function change(j, call) {
    const steps = [...t.steps];
    steps[j] = call;
    go(t.ex, steps, true);
  }

  // Remove step `j`: the program runs again without it.
  function remove(j) {
    go(t.ex, t.steps.filter((_, k) => k !== j), true);
  }

  // The results so far, by name: $n for call n, and the name its step
  // gives it (of the first `j` calls).
  // (`runners`: for a runner, a call's result while it has not returned
  // is the runner it started: a vfork's child, before the clone returns.)
  const known = (j = t.done.length, runners = false) => {
    const o = {};
    t.done.slice(0, j).forEach((d, i) => {
      const v = d?.ret ?? (runners ? t.started[i] : null);
      if (v == null) return;
      o[`$${i + 1}`] = v;
      if (t.steps[i].name) o[t.steps[i].name] = v;
    });
    return o;
  };
  // A step's runner (a thread id): as written, a number or a result.
  const tidOf = (s) => (s.tid == null ? 1 : typeof s.tid === "number" ? s.tid : m.value(String(s.tid), known(t.done.length, true)));
  // A step's call, as written (or with `values` for the results: as made):
  // `tid: name = call(args)`.
  const text = (s, values) => `${s.tid != null ? `${s.tid}: ` : ""}${s.name ? `${s.name} = ` : ""}${s.call}(${s.args.map((a) => (values ? made(a, values) : a)).join(", ")})`;
  // How a call finished, or not yet.
  const how = (d) => d.how == null ? "… blocked"
    : d.how === "exec" ? "(no return: the new program runs)"
    : d.how === "exited" ? "(no return: it exited)" : `= ${result(d.ret, consts)}`;
  // Who makes step `j`'s call, if not /init.
  const who = (j) => {
    const tid = t.done[j]?.tid ?? (typeof t.steps[j].tid === "number" ? t.steps[j].tid : null);
    return tid === 1 || tid == null ? null : el("span", { className: "mono tid", textContent: `tid ${tid}`,
      title: "the runner with this thread id (a thread or a child of /init)" });
  };
  // The program: a call a line, each its own editor; then a line to add one.
  function draw() {
    t.list.replaceChildren(...t.steps.map((s, j) => {
      const d = t.done[j];
      const e = callEditor({ m, consts, known: (r) => known(j, r), text: text(s), inline: true, common,
        focus: () => pick(j),
        done: (c) => (c == null ? remove(j) : text(c) === text(s) ? e.input.blur() : change(j, c)),
        cancel: () => ((e.input.value = text(s)), e.input.blur()) });
      e.input.title = "type to change the call (Enter: run the program again with it; Esc, or leaving it: not; emptied: remove it)";
      return el("li", { className: [d ? "done" : "", d && d.how == null ? "blocked" : ""].join(" ").trim() },
        el("div", { className: "line" }, typeof s.tid === "number" ? null : who(j), e.el,
          d ? el("span", { className: "mono ret", textContent: how(d) }) : null,
          el("a", { className: "rm", textContent: "✕", title: "remove the call (the program runs again without it)", onclick: () => remove(j) })),
        t.started[j] ? el("div", { className: "dim more", textContent: `→ tid ${t.started[j]}, a runner` }) : null,
        d && text(s, known()) !== text(s) ? el("div", { className: "mono dim more", textContent: `as made: ${text(s, known())}` }) : null,
        d?.text ? el("div", { className: "mono dim more", textContent: d.text }) : null);
    }), el("li", { className: "add" }, t.form));
  }
  // The explanation: its paragraphs, each link a place in the recording
  // (see the top): a call, or a function's first call in it; resolved
  // once the calls are made (t.links), dim if the recording has no such
  // place; `code` in backticks.
  function explain() {
    t.links = [];
    const paras = (t.ex.text ?? []).map((para) => el("p", {}, ...para.split(/(\[[^\]]*\]\([^)]*\)|`[^`]*`)/).map((part) => {
      // (A try: `[label](try:1)`, the example's `tries[0]`, a program to run instead: see the top.)
      const tr = part.match(/^\[([^\]]*)\]\(try:(\d+)\)$/), prog = tr && t.ex.tries?.[Number(tr[2]) - 1];
      if (tr) return el("a", { className: "try", textContent: tr[1], title: prog ? `run this program instead:\n${prog}` : "no such try",
        onclick: prog ? () => go(t.ex, program(prog), true) : null });
      const link = part.match(/^\[([^\]]*)\]\((\d+)(?::([\w.]+))?(:any)?\)$/);
      if (link) {
        const l = { j: Number(link[2]) - 1, func: link[3], any: !!link[4] };
        l.c = syscall(l.j);
        l.at = l.c && l.func ? m.first_call(l.func, l.c.first, l.c.last ?? m.records() - 1, l.any ? undefined : l.c.task.addr) : null;
        const found = l.c && (!l.func || l.at != null);
        l.a = el(found ? "a" : "span", { className: `place mono${found ? "" : " gone"}`, textContent: link[1],
          title: !l.c ? `call ${l.j + 1} is not made` : l.func && l.at == null ? `${l.func} does not run in call ${l.j + 1}`
            : l.func ? `${l.func}, in call ${l.j + 1}'s recording` : `call ${l.j + 1}'s recording`, onclick: found ? () => goTo(l) : null });
        t.links.push(l);
        return l.a;
      }
      return part.startsWith("`") ? el("code", { className: "mono", textContent: part.slice(1, -1) }) : part;
    })));
    t.text.replaceChildren(...paras, ...(!paras.length && !t.steps.length ? [el("p", { className: "dim", textContent: "Write a program below: a call, and Enter." })] : []));
  }
  // Go to link `l`: its call selected, the cursor at its function's call
  // (or the call's start); the link marked.
  function goTo(l) {
    app.select(l.c);
    if (l.at != null) app.time.go(l.at);
    for (const x of t.links) x.a.classList.toggle("on", x === l);
    mark(l.j);
  }

  // Make the next call, recorded, and wait until the kernel is quiet (it
  // finished, or it blocks).
  async function step() {
    const j = t.done.length, s = t.steps[j];
    if (!s) return false;
    const mine = t.list;
    let q, news;
    try {
      const args = s.args.map((a, k) => m.arg(s.call, k, a, known())), tid = tidOf(s);
      q = { id: m.call(s.call, args, tid), tid, args };
      t.done[j] = q;
      const before = new Set(m.runners());
      news = await m.quiet();
      if (t.list !== mine) return false;
      t.started[j] = [...news.runners].find((r) => !before.has(r));
    } catch (e) {
      if (t.list === mine) t.status.textContent = `Call ${j + 1}: ${e.message ?? e}`;
      return false;
    }
    // (The calls it finished: it, or ones it woke.)
    for (const f of news.finished) {
      const d = t.done.find((x) => x?.id === f.id);
      if (d) Object.assign(d, { how: f.how, ret: f.ret, text: outputs(t.steps[t.done.indexOf(d)].call, d.args, f.buffers) });
    }
    app.refresh();
    draw();
    return true;
  }
  // Step `j`'s syscall summary, if the recording still has it.
  const syscall = (j) => t.done[j] && app.calls()[m.syscall_of(t.done[j].id)];
  // Call `j` picked: its recording selected.
  function pick(j) {
    const c = syscall(j);
    if (!c) return (t.status.textContent = t.done[j] ? "This call's recording is gone: the ring has moved on." : "");
    app.select(c);
    mark(j);
  }
  // (Call `j`'s line marked, the others not.)
  const mark = (j) => [...t.list.children].forEach((li, k) => li.classList.toggle("on", k === j));

  // What a call left in the buffers it was to fill.
  function outputs(call, args, buffers) {
    const shown = (b) => call === "getdents64" ? `entries: ${dirents(b).join(", ")}`
      : call === "pipe2" ? `fds: ${[...new Int32Array(b.buffer, b.byteOffset, 2)].join(", ")}` : bytes(b);
    return args.map((a, i) => (a?.size != null && buffers?.[i] ? shown(buffers[i]) : null)).filter(Boolean).join("  ");
  }
  function dirents(b) {
    const v = new DataView(b.buffer, b.byteOffset, b.byteLength), names = [];
    // (struct linux_dirent64: ino, off, reclen (u16), type, name)
    for (let o = 0; o + 19 < b.length;) {
      const len = v.getUint16(o + 16, true);
      if (!len) break;
      names.push(new TextDecoder().decode(b.subarray(o + 19, b.indexOf(0, o + 19))));
      o += len;
    }
    return names;
  }
  function bytes(b) {
    const n = b.indexOf(0) < 0 ? b.length : b.indexOf(0), s = new TextDecoder().decode(b.subarray(0, n));
    return /^[\x20-\x7e\n\t]*$/.test(s) ? JSON.stringify(s) : [...b.subarray(0, 32)].map((x) => x.toString(16).padStart(2, "0")).join(" ");
  }

  // The syscalls the examples make: to list first.
  const common = [...new Set(list.flatMap((x) => x.steps.map((s) => s.call)))];
  // Add a call, written as in C, and Enter to make it (see calledit.js).
  // (As a shell's prompt: the call made, its result on its line, the
  // prompt again, ↑ for the calls before.)
  function form() {
    const e = callEditor({ m, consts, known: (r) => known(t.done.length, r), common, placeholder: "add a call…", history: () => t.steps.map((s) => text(s)),
      done: async (c) => {
        t.steps.push(c);
        t.changed = true;
        toURL();
        e.input.value = "";
        draw();
        e.focus();
        if (await step()) pick(t.steps.length - 1);
        e.focus();
      } });
    return el("div", { className: "callform" }, e.el);
  }

  // (The example, shown while the kernel boots; its calls made once it is up.)
  const u = fromURL();
  if (u.ex) open(u.ex, u.steps);
  t.changed = u.changed;
  run();
}
