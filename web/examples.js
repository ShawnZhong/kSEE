// The examples, one at a time: a selector (and ◀ ▶) switches, each on a
// fresh kernel (the kernel starts again, the page stays); the URL says
// which (#ex=NAME). An example is a question about the kernel, a program
// that asks it (user/examples/*.json: syscalls, one after the other), and
// a tutorial: stops, each a note on a function one of the calls runs.
//
// Opening an example makes all its calls, each recorded (a runner makes
// each: /init, or a thread or child of it; see Machine.call). A call may
// block, and finish as a later one wakes it. Then the tutorial goes from
// stop to stop (◀ ▶): each is the first call of its function in its call's
// recording (on the call's task, or any for a stop that says so), the
// cursor there. The program is below it, a call a line: click one for its
// recording; type in it to change it (Enter: the program runs again, on a
// fresh kernel), empty it (or ✕) to remove it; add a call at the end; ↺
// for the example's own program again.
//
// A step: { call, args, stops?, as?, tid? }; its arguments written as in C
// (see args.js): "a string", 0644, O_CREAT | O_WRONLY, [64], {0, 30000000},
// $2. Its runner: the one call `as` started, or thread `tid`; else /init.
import { $, el } from "./dom.js";
import { arg, made, parseCall, result } from "./args.js";
import { callEditor } from "./calledit.js";

export async function examples(app) {
  const { m } = app;
  const consts = m.constants();
  const list = await fetch(new URL("./examples.json", location)).then((r) => (r.ok ? r.json() : [])).catch(() => []);
  // The open example: its steps, the calls asked ({ id, tid, args, how?,
  // ret?, text?: how once it finished), the runner each started; the
  // tutorial's stops ({ j: the call, k: its stop, at: the record }) and
  // where it is.
  const t = { ex: null, steps: [], done: [], started: [], tour: [], pos: -1, keep: -1, list: null, tut: null, status: null };

  // The URL: #ex=NAME, the example; &p=CALLS, its program if changed (the
  // calls as written, separated by ";": see program); &stop=N, where the
  // tutorial is. { ex, steps, changed, stop }.
  function fromURL() {
    const q = Object.fromEntries(location.hash.slice(1).split("&").filter(Boolean).map((kv) => {
      const k = kv.indexOf("=");
      return [kv.slice(0, k), decodeURIComponent(kv.slice(k + 1))];
    }));
    const ex = list.find((x) => x.name === q.ex) ?? list[0];
    const steps = q.p != null ? program(q.p, ex?.steps ?? []) : ex?.steps ?? [];
    return { ex, steps, changed: q.p != null, stop: q.stop ? Number(q.stop) - 1 : -1 };
  }
  function toURL(stop = t.pos) {
    const p = t.changed ? `&p=${encodeURIComponent(t.steps.map((s) => text(s, null, true)).join("; "))}` : "";
    history.replaceState(null, "", `#ex=${t.ex.name}${p}${stop >= 0 ? `&stop=${stop + 1}` : ""}`);
  }
  // Program `text` (calls as written, separated by ";" outside strings),
  // the stops of `was`'s calls kept where a call is the same syscall.
  function program(text, was) {
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
    return calls.map((c) => c.trim()).filter(Boolean).map((c, j) => {
      // ("19: …": runner 19; "@2: …": the runner call 2 started.)
      const [, at, n, rest] = c.match(/^(?:(@?)(\d+)\s*:)?\s*([\s\S]*)$/);
      const call = parseCall(rest) ?? { call: rest, args: [] };
      const who = n ? (at ? { as: Number(n) } : { tid: Number(n) }) : {};
      return { ...call, ...who, stops: was[j]?.call === call.call ? was[j].stops : undefined };
    });
  }
  // Switch to example `ex` (its program `steps`, `changed` from the
  // example's: see change), on a fresh kernel, so that it always starts
  // from the same state; and make its calls.
  function go(ex, steps = ex.steps, changed = false) {
    open(ex, steps);
    t.changed = changed;
    t.reset.hidden = !changed;
    toURL(-1);
    t.status.textContent = "Starting a fresh kernel…";
    app.restart().then(run);
  }

  function open(ex, steps = ex.steps) {
    t.ex = ex;
    t.steps = steps.map((s) => ({ ...s })); // (the form adds to them)
    t.done = [];
    t.started = [];
    t.tour = [];
    t.pos = -1;
    t.changed = false;
    t.status = el("div", { className: "tstatus dim" });
    t.tut = el("div", { className: "tour" });
    t.list = el("ol", { className: "steps" });
    const k = list.indexOf(ex);
    const pick = el("select", { className: "pick", title: "the examples (each on a fresh kernel)", onchange: () => go(list[pick.selectedIndex]) },
      ...list.map((x) => el("option", { textContent: x.title, selected: x === ex })));
    // (The examples' navigation: in the panel's title.)
    $("ex-head").replaceChildren(el("b", { textContent: "kSEE" }),
      el("button", { textContent: "◀", title: "the example before", disabled: k <= 0, onclick: () => go(list[k - 1]) }), pick,
      el("button", { textContent: "▶", title: "the next example", disabled: k < 0 || k + 1 >= list.length, onclick: () => go(list[k + 1]) }));
    t.form = form();
    $("examples").replaceChildren(
      ex.question ? el("div", { className: "q", textContent: ex.question }) : null,
      t.tut, el("div", { className: "ptitle dim" }, "Program", t.reset = el("a", { textContent: "↺ the example's", hidden: true,
        title: "the example's own program again (yours is in the URL until then)", onclick: () => go(ex) })), t.list, t.status);
    draw();
    tutorial();
  }

  // The kernel is up: make the calls, all of them; then the tutorial, from
  // its first stop (or, after a change, where it was).
  async function run() {
    const mine = t.list;
    t.status.textContent = "Making the calls…";
    for (let j = 0; j < t.steps.length; j++) if (!(await step()) || t.list !== mine) return;
    t.status.textContent = "";
    t.tour = t.steps.flatMap((s, j) => {
      const c = syscall(j);
      if (!c) return [];
      const b = c.last ?? m.records() - 1;
      return (s.stops ?? []).map((st, k) => ({ j, k, at: m.first_call(st.func, c.first, b, st.any ? undefined : c.task.addr) }))
        .filter((x) => x.at != null);
    });
    if (t.tour.length) stop(Math.min(Math.max(t.keep, 0), t.tour.length - 1));
    else if (t.done.length) pick(t.done.length - 1);
    t.keep = -1;
  }
  // Change step `j` to `call` (its stops kept if it is the same syscall):
  // the program runs again, on a fresh kernel (a kernel cannot be put back
  // as it was: its tasks' stacks are the browser's), the tutorial where it
  // was. (The program is in the URL, &p=: a link to it.)
  function change(j, call) {
    const old = t.steps[j], steps = [...t.steps];
    // (Its runner kept, unless another is written.)
    steps[j] = { ...call, ...(call.tid == null && old?.as ? { as: old.as } : {}), stops: old?.call === call.call ? old.stops : undefined };
    t.keep = t.pos;
    go(t.ex, steps, true);
  }

  // Remove step `j`: the program runs again without it.
  function remove(j) {
    const steps = t.steps.filter((_, k) => k !== j);
    t.keep = t.pos;
    go(t.ex, steps, true);
  }

  const rets = () => t.done.map((d) => d.ret);
  // A step's runner (a thread id).
  const tidOf = (s) => s.tid ?? (s.as ? t.started[s.as - 1] : 1);
  // A step's call, as written (or with `values` for $n: as made); `tid: `
  // before it if its runner is not /init (and, `all`, `@k: ` if it is the
  // one call k started: see program).
  const text = (s, values, all) => `${s.tid ? `${s.tid}: ` : all && s.as ? `@${s.as}: ` : ""}${s.call}(${s.args.map((a) => (values ? made(a, values) : a)).join(", ")})`;
  // How a call finished, or not yet.
  const how = (d) => d.how == null ? "… blocked"
    : d.how === "exec" ? "(no return: the new program runs)"
    : d.how === "exited" ? "(no return: it exited)" : `= ${result(d.ret, consts)}`;
  // Who makes step `j`'s call, if not /init.
  const who = (j) => {
    const tid = t.done[j]?.tid ?? tidOf(t.steps[j]);
    return tid === 1 || tid == null ? null : el("span", { className: "mono tid", textContent: `tid ${tid}`,
      title: t.steps[j].as ? `the runner call ${t.steps[j].as} started (a thread or a child of /init)` : "the runner with this thread id" });
  };
  // The program: a call a line, each its own editor; then a line to add one.
  function draw() {
    t.list.replaceChildren(...t.steps.map((s, j) => {
      const d = t.done[j];
      const e = callEditor({ m, consts, rets: () => rets().slice(0, j), text: text(s), inline: true, common,
        focus: () => pick(j),
        done: (c) => (c == null ? remove(j) : text(c) === text(s) ? e.input.blur() : change(j, c)),
        cancel: () => ((e.input.value = text(s)), e.input.blur()) });
      e.input.title = "type to change the call (Enter: run the program again with it; Esc, or leaving it: not; emptied: remove it)";
      return el("li", { className: [d ? "done" : "", d && d.how == null ? "blocked" : "", t.tour[t.pos]?.j === j ? "on" : ""].join(" ").trim() },
        el("div", { className: "line" }, s.tid ? null : who(j), e.el,
          d ? el("span", { className: "mono ret", textContent: how(d) }) : null,
          el("a", { className: "rm", textContent: "✕", title: "remove the call (the program runs again without it)", onclick: () => remove(j) })),
        t.started[j] ? el("div", { className: "dim more", textContent: `→ tid ${t.started[j]}, a runner` }) : null,
        d && text(s, rets()) !== text(s) ? el("div", { className: "mono dim more", textContent: `as made: ${text(s, rets())}` }) : null,
        d?.text ? el("div", { className: "mono dim more", textContent: d.text }) : null);
    }), el("li", { className: "add" }, t.form));
  }
  // The tutorial: its stops, in full, the one it is at marked; ◀ ▶.
  function tutorial() {
    const n = t.tour.length;
    if (!n) return t.tut.replaceChildren(el("div", { className: "dim", textContent: t.steps.length ? "No tutorial: the program's calls, recorded, are below." : "Write a program below: a call, and Enter." }));
    const items = t.tour.map((x, p) => {
      const st = t.steps[x.j].stops[x.k];
      return el("li", { className: p === t.pos ? "on" : "", onclick: () => stop(p) },
        el("b", { className: "mono", textContent: st.func }), " ", st.note, el("span", { className: "dim", textContent: ` · call ${x.j + 1}` }));
    });
    t.tut.replaceChildren(
      el("div", { className: "tnav" },
        el("button", { textContent: "◀ Back", title: "the stop before ([)", disabled: t.pos <= 0, onclick: () => stop(t.pos - 1) }),
        el("span", { className: "dim", textContent: t.pos >= 0 ? `Stop ${t.pos + 1} of ${n}` : `${n} stops` }),
        el("button", { className: "go", textContent: "Next ▶", title: "the next stop (])", disabled: t.pos + 1 >= n, onclick: () => stop(t.pos + 1) })),
      el("ol", { className: "stops" }, ...items));
    items[t.pos]?.scrollIntoView({ block: "nearest" });
  }

  // Make the next call, recorded, and wait until the kernel is quiet (it
  // finished, or it blocks).
  async function step() {
    const j = t.done.length, s = t.steps[j];
    if (!s) return false;
    const mine = t.list;
    let q, news;
    try {
      const args = s.args.map((a) => arg(a, consts, rets())), tid = tidOf(s);
      q = { id: m.call(s.call, args, tid), tid, args };
      t.done[j] = q;
      const before = new Set(m.runners());
      news = await m.quiet();
      if (t.list !== mine) return false;
      t.started[j] = [...news.runners].find((r) => !before.has(r));
    } catch (e) {
      if (t.list === mine) t.status.textContent = `Call ${j + 1}: ${e.message}`;
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
  // Call `j` picked: its recording selected, the tutorial at its first stop.
  function pick(j) {
    const k = t.tour.findIndex((x) => x.j === j);
    if (k >= 0 && t.tour[t.pos]?.j !== j) return stop(k);
    const c = syscall(j);
    if (!c) return (t.status.textContent = t.done[j] ? "This call's recording is gone: the ring has moved on." : "");
    app.select(c);
    mark(j);
  }
  // Go to stop `p` of the tutorial: its call selected, the cursor there.
  function stop(p) {
    const s = t.tour[p], c = s && syscall(s.j);
    if (!c) return;
    t.pos = p;
    app.select(c);
    app.time.go(s.at);
    tutorial();
    mark(s.j);
    toURL();
  }
  // (The tutorial's stops, for the keys: ] the next, [ the one before.)
  app.tour = (d) => stop(t.pos + d);
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
  function form() {
    const e = callEditor({ m, consts, rets, common, placeholder: "add a call…", done: async (c) => {
      t.steps.push(c);
      t.changed = true;
      toURL();
      e.input.value = "";
      draw();
      if (await step()) pick(t.steps.length - 1);
    } });
    return el("div", { className: "callform" }, e.el);
  }

  // (The example, shown while the kernel boots; its calls made once it is up.)
  // (The URL's tutorial stop, once there is a tutorial: see run.)
  const u = fromURL();
  if (u.ex) open(u.ex, u.steps);
  t.changed = u.changed;
  t.keep = u.stop;
  run();
}
