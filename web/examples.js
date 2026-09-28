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
// the call's task; `:any` on any task), the cursor there, and `:calls`
// (or `:stores`, `:objects`, `:stack`, `:trace`) shows that tab
// (`:objects`: the function's call selected, as a double click on it in
// the timeline does, at its return: what it changed); the first such
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
// Or a variable of the program's: { decl }, declared as in C (`char
// buf[64]`, `int fds[2]`, `struct __kernel_timespec ts = {0, 30000000}`:
// Machine.declare), memory of its own in /init's process; its name then
// stands for its address (`read(fd, buf, 64)`, `&ts`), and each call's
// line says the variables it changed, as C writes them (`fds = {3, 4}`).
import { $, el } from "./dom.js";
import { result } from "./args.js";
import { callEditor } from "./calledit.js";
import { runner } from "./kernel.js";

export async function examples(app) {
  const { m } = app;
  const consts = app.consts();
  const list = await fetch(new URL("./examples.json", location)).then((r) => (r.ok ? r.json() : [])).catch(() => []);
  // (An example's program: its lines, as written; made steps here, once.)
  for (const ex of list) ex.steps = ex.steps.map((line) => {
    try {
      return m.statement(line);
    } catch {
      return { call: line, args: [] };
    }
  });
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
    return calls.map((c) => c.trim()).filter(Boolean).map(statement);
  }
  // A program's line (as written: see Machine.statement), as a step: a
  // call, or a variable's declaration. (One not either: a call of that
  // name, whose error making it says so.)
  const statement = (line) => {
    try {
      return m.statement(line);
    } catch {
      return { call: line, args: [] };
    }
  };
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
    t.steps = steps.map((s) => ({ ...s }));
    // (Made by a runner, kernel.js's: its steps, how each went, the runners
    // they started, the variables.)
    t.run = runner(app.kernel, m, t.steps);
    ({ done: t.done, started: t.started, vars: t.vars } = t.run);
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

  // The program's names, numbers and runners: its runner's (kernel.js).
  const known = (...a) => t.run.known(...a);
  const stepOf = (n) => t.run.stepOf(n);
  const tidOf = (s) => t.run.tidOf(s);
  // A step's line, as written:
  // `tid: name = call(args)`.
  const text = (s) => s.decl ?? `${s.tid != null ? `${s.tid}: ` : ""}${s.type ? `${s.type}${s.type.endsWith("*") ? "" : " "}` : ""}${s.name ? `${s.name} = ` : ""}${s.call}(${s.args.join(", ")})`;
  // How a call finished, or not yet.
  // (A variable: as it starts; each call's line says how it changed.)
  const how = (d, s) => s?.decl ? `= ${t.vars.find((v) => v.j === t.done.indexOf(d))?.initial ?? ""}`
    : d.how == null ? "… blocked"
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
          d ? el("span", { className: "mono ret", textContent: how(d, s) }) : null,
          el("a", { className: "rm", textContent: "✕", title: "remove the call (the program runs again without it)", onclick: () => remove(j) })),
        t.started[j] ? el("div", { className: "dim more", textContent: `→ tid ${t.started[j]}, a runner` }) : null,
        d?.text ? el("div", { className: "mono dim more", textContent: d.text }) : null,
        d?.vars ? el("div", { className: "mono more vars", textContent: d.vars }) : null);
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
      const link = part.match(/^\[([^\]]*)\]\((\d+)(?::([\w.]+))?(:any)?(?::(stack|calls|stores|objects|trace))?\)$/);
      if (link) {
        const n = Number(link[2]), l = { n, j: stepOf(n), func: link[3], any: !!link[4], tab: link[5] };
        l.c = syscall(l.j);
        l.at = l.c && l.func ? m.first_call(l.func, l.c.first, l.c.last ?? m.records() - 1, l.any ? undefined : l.c.task.addr) : null;
        const found = l.c && (!l.func || l.at != null);
        l.a = el(found ? "a" : "span", { className: `place mono${found ? "" : " gone"}`, textContent: link[1],
          title: !l.c ? `call ${l.n} is not made` : l.func && l.at == null ? `${l.func} does not run in call ${l.n}`
            : l.func ? `${l.func}, in call ${l.n}'s recording` : `call ${l.n}'s recording`, onclick: found ? () => goTo(l) : null });
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
    if (l.at != null && l.tab === "objects") {
      app.time.selectCall(l.at, l.func);
      app.time.go(m.call_end(l.at) ?? l.at);
    } else if (l.at != null) app.goCall(l.at);
    if (l.tab) ($("right").dataset.tab = l.tab), app.outlined?.();
    for (const x of t.links) x.a.classList.toggle("on", x === l);
    mark(l.j);
  }

  // Make the next call, recorded, and wait until the kernel is quiet (it
  // finished, or it blocks).
  async function step() {
    const j = t.done.length, s = t.steps[j];
    if (!s) return false;
    const mine = t.list;
    let r;
    try {
      r = await t.run.next();
    } catch (e) {
      if (t.list === mine) t.status.textContent = `${s.decl ? "Line" : "Call"} ${j + 1}: ${e.message ?? e}`;
      return false;
    }
    if (t.list !== mine) return false;
    // (A variable: as it starts.)
    if (r.decl) {
      r.decl.shown = r.decl.initial = r.decl.show();
      draw();
      return true;
    }
    const q = r.q;
    // (What the calls finished left in their buffers: it, or ones it woke.)
    for (const [i, d] of t.done.entries()) if (d?.buffers && d.text == null) d.text = outputs(t.steps[i].call, d.args, d.buffers);
    // (And the variables it changed, as they are now; one it was passed as
    // records of a variable length, as its parameter's type reads them:
    // Machine.as_records, a directory's entries.)
    const changed = [];
    for (const v of t.vars) {
      const now = m.show(v.ty, v.addr, v.count ?? undefined);
      if (now !== v.shown) {
        const k = s.args.indexOf(v.name), size = v.size * (v.count ?? 1);
        const records = k >= 0 && q.ret > 0 ? m.as_records(s.call, k, v.addr, Math.min(q.ret, size)) : null;
        changed.push(`${v.name} = ${records ?? now}`);
      }
      v.shown = now;
    }
    if (changed.length) q.vars = changed.join("   ");
    app.refresh();
    draw();
    return true;
  }
  // Step `j`'s syscall summary, if the recording still has it.
  const syscall = (j) => t.done[j] && app.syscalls()[m.syscall_of(t.done[j].id)];
  // Call `j` picked: its recording selected.
  function pick(j) {
    if (t.steps[j]?.decl) return mark(j);
    const c = syscall(j);
    if (!c) return (t.status.textContent = t.done[j] ? "This call's recording is gone: the ring has moved on." : "");
    app.select(c);
    mark(j);
  }
  // (Call `j`'s line marked, the others not.)
  const mark = (j) => [...t.list.children].forEach((li, k) => li.classList.toggle("on", k === j));

  // What a call left in the buffers it was to fill (`[64]`, as older
  // programs have them: a variable says it better).
  function outputs(call, args, buffers) {
    return args.map((a, i) => (a?.size != null && buffers?.[i] ? bytes(buffers[i]) : null)).filter(Boolean).join("  ");
  }
  function bytes(b) {
    const n = b.indexOf(0) < 0 ? b.length : b.indexOf(0), s = new TextDecoder().decode(b.subarray(0, n));
    return /^[\x20-\x7e\n\t]*$/.test(s) ? JSON.stringify(s) : [...b.subarray(0, 32)].map((x) => x.toString(16).padStart(2, "0")).join(" ");
  }

  // The syscalls the examples make: to list first.
  const common = [...new Set(list.flatMap((x) => x.steps.map((s) => s.call)).filter(Boolean))];
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
