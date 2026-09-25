// The examples, one at a time: a selector (and ◀ ▶) switches, each on a
// fresh kernel (see go: the kernel starts again, the page stays); the URL
// says which (#ex=NAME). An example is a question
// about the kernel, and a list of
// syscalls that asks it (user/examples/*.json), each with stops (a note on a
// function it runs). A runner makes each call (Machine.call: /init, or a
// thread or child process of it, which is a runner too), one at a time,
// each recorded: then its syscall is selected, and its stops are places in
// its recording (the first call of the stop's function, on the runner's
// task, or any for a stop that says so). A call may block: it waits for
// another's (a futex's wake, a pipe's write), and finishes then. Next goes
// from one stop to the next. Below the calls, add one: write it as in C,
// and Enter.
//
// A step: { call, args, stops?, as?, tid? }; its arguments written as in C
// (see args.js): "a string", 0644, O_CREAT | O_WRONLY, [64], {0, 30000000},
// $2. Its runner: the one call `as` started, or thread `tid`; else /init.
import { $, el } from "./dom.js";
import { arg, made, parseCall, result } from "./args.js";

export async function examples(app) {
  const { m } = app;
  const consts = m.constants();
  const list = await fetch(new URL("./examples.json", location)).then((r) => (r.ok ? r.json() : [])).catch(() => []);
  // The open example: its steps, the ones asked ({ id, from, tid, nr, how?,
  // ret?, text?, ol }: how once it finished), the runner each started, and
  // the stops Next goes through (the selected step's: cur, at, k).
  const t = { ex: null, steps: [], done: [], started: [], running: false, cur: { stops: [], ol: null }, at: [], k: -1, list: null, status: null };

  // The example in the URL (#ex=NAME, or #back=: see branch), else the
  // first: { ex, steps, back (the calls to make again, then a new one) }.
  function fromURL() {
    const h = location.hash, name = h.match(/^#ex=([\w-]+)$/)?.[1];
    let back = null;
    try {
      back = JSON.parse(decodeURIComponent(h.match(/^#back=(.*)$/)?.[1] ?? "null"));
    } catch { /* none */ }
    const ex = list.find((x) => x.name === (back?.name ?? name)) ?? list[0];
    return { ex, steps: back?.steps ?? ex?.steps ?? [], back };
  }
  // Switch to example `ex` (its calls `steps`; `back`: to make them again,
  // see branch): on a fresh kernel, so that it always starts from the same
  // state (the kernel starts again; the page stays).
  function go(ex, steps = ex.steps, back = null) {
    history.replaceState(null, "", back ? `#back=${encodeURIComponent(JSON.stringify({ name: ex.name, steps }))}` : `#ex=${ex.name}`);
    open(ex, steps, back);
    t.status.textContent = "Starting a fresh kernel…";
    app.restart().then(ready);
  }

  function open(ex, steps = ex.steps, back = null) {
    t.ex = ex;
    t.back = back;
    t.branch = null;
    t.running = false;
    t.steps = steps.map((s) => ({ ...s })); // (the form adds to them)
    t.done = [];
    t.started = [];
    t.cur = { stops: [], ol: null };
    t.at = [];
    t.k = -1;
    t.status = el("div", { className: "tstatus dim" });
    t.list = el("ol", { className: "steps" });
    const k = list.indexOf(ex);
    const pick = el("select", { className: "pick", title: "the examples (each on a fresh kernel)", onchange: () => go(list[pick.selectedIndex]) },
      ...list.map((x) => el("option", { textContent: x.title, selected: x === ex })));
    $("examples").replaceChildren(
      el("div", { className: "thead" },
        el("div", { className: "nav" },
          el("button", { textContent: "◀", title: "the example before", disabled: k <= 0, onclick: () => go(list[k - 1]) }), pick,
          el("button", { textContent: "▶", title: "the next example", disabled: k < 0 || k + 1 >= list.length, onclick: () => go(list[k + 1]) })),
        ex.question ? el("div", { className: "q", textContent: ex.question }) : null,
        el("div", { className: "tbar" }, el("button", { className: "go", textContent: "▶ Next call", onclick: () => step() }))),
      t.list, form(), t.status);
    draw();
  }

  // The kernel is up: make the example's first call; or, after going back
  // (see branch), its calls up to the one changed.
  function ready() {
    if (!t.back) return step();
    const n = t.steps.length;
    return upTo(n - 1).then(() => (t.status.textContent = n > 1
      ? `Went back to before call ${n}: on a fresh kernel, ${n > 2 ? `calls 1–${n - 1}` : "call 1"} made again, then this one.` : ""));
  }
  // Go back to before call `j` and make `call` instead: a kernel cannot be
  // put back as it was (its tasks' stacks are the browser's), so it starts
  // again (a fresh kernel), and calls 1..j-1 are made again, then it. (The
  // calls are in the URL, #back=: a link to this, too.)
  function branch(j, call) {
    const old = t.steps[j];
    go(t.ex, [...t.steps.slice(0, j), { ...call, stops: old?.call === call.call ? old.stops : undefined }], true);
  }

  const rets = () => t.done.map((d) => d.ret);
  // A step's runner (a thread id).
  const tidOf = (s) => s.tid ?? (s.as ? t.started[s.as - 1] : 1);
  // A step's call, as written (or with `values` for $n: as made); `tid: `
  // before it if its runner is not /init.
  const text = (s, values) => `${s.tid ? `${s.tid}: ` : ""}${s.call}(${s.args.map((a) => (values ? made(a, values) : a)).join(", ")})`;
  // How a call finished, or not yet.
  const how = (d) => d.how == null ? " … blocked (it waits for another call)"
    : d.how === "exec" ? " (execve: no return, the new program runs)"
    : d.how === "exited" ? " (no return: it exited)" : ` = ${result(d.ret, consts)}`;
  // Who makes step `j`'s call, if not /init.
  const who = (j) => {
    const tid = t.done[j]?.tid ?? tidOf(t.steps[j]);
    return tid === 1 || tid == null ? null : el("span", { className: "mono tid", textContent: `tid ${tid} `,
      title: t.steps[j].as ? `the runner call ${t.steps[j].as} started (a thread or a child of /init)` : "the runner with this thread id" });
  };
  function draw() {
    t.list.replaceChildren(...t.steps.map((s, j) => {
      const d = t.done[j];
      if (d) d.ol = s.stops?.length ? el("ol", { className: "stops" }, ...s.stops.map((st, k) => el("li", { onclick: () => {
        select(j);
        stop(k);
      } }, el("b", { className: "mono", textContent: st.func }), " ", st.note))) : null;
      return el("li", { className: d ? (d.how == null ? "done blocked" : "done") : j === t.done.length ? "next" : "" },
        // (As written; once made, and if $n made it differ, as made too.)
        s.tid ? null : who(j),
        el("a", { className: "mono", textContent: text(s), onclick: () => (d ? select(j) : upTo(j)) }),
        d ? el("span", { className: "mono ret", textContent: how(d) }) : null,
        t.started[j] ? el("span", { className: "dim", textContent: ` → tid ${t.started[j]}, a runner` }) : null,
        // (Change it: go back to before it, and make another.)
        d ? el("a", { className: "who", textContent: " ✎", title: "change this call: go back to before it, and make another instead", onclick: () => {
          t.branch = j;
          t.input.value = text(s);
          t.hint.textContent = `Changing call ${j + 1}: Enter goes back to before it (a fresh kernel, calls 1–${j} again) and makes this instead; Esc: not.`;
          t.input.focus();
        } }) : null,
        d && text(s, rets()) !== text(s) ? el("div", { className: "mono dim", textContent: `as made: ${text(s, rets())}` }) : null,
        d?.text ? el("div", { className: "mono dim", textContent: d.text }) : null,
        d?.ol ?? null);
    }));
  }

  // Make the calls up to step `j`.
  async function upTo(j) {
    while (t.done.length <= j) if (!(await step())) return;
  }
  // Make the next call, recorded, and wait until the kernel is quiet; then
  // select it (it finished, or it blocks).
  async function step() {
    const j = t.done.length, s = t.steps[j];
    if (!s || t.running) return false;
    t.running = true;
    t.status.textContent = `${s.call}…`;
    let q, news;
    // (If the kernel starts again meanwhile, the call gives up: another
    // example's, then, not this one's to say.)
    const mine = t.list;
    try {
      const args = s.args.map((a) => arg(a, consts, rets())), tid = tidOf(s);
      q = { ...m.call(s.call, args, tid), tid, nr: m.syscall(s.call).nr, args };
      t.done[j] = q;
      const before = new Set(m.runners());
      news = await m.quiet();
      if (t.list !== mine) return false;
      t.started[j] = [...news.runners].find((r) => !before.has(r));
    } catch (e) {
      if (t.list === mine) {
        t.running = false;
        t.status.textContent = e.message;
      }
      return false;
    }
    t.running = false;
    // (The calls it finished: it, or ones it woke.)
    for (const f of news.finished) {
      const d = t.done.find((x) => x?.id === f.id);
      if (d) Object.assign(d, { how: f.how, ret: f.ret, text: outputs(t.steps[t.done.indexOf(d)].call, d.args, f.buffers) });
    }
    app.refresh();
    const woke = news.finished.filter((f) => f.id !== q.id).map((f) => t.done.findIndex((x) => x?.id === f.id) + 1);
    t.status.textContent = (woke.length ? `It woke call ${woke.join(", ")}: done now. ` : "")
      + (q.how == null ? "It blocks: make another call to wake it. " : "")
      + (j + 1 < t.steps.length ? "" : "That was the last call: add one below, or click a call to see it again.");
    draw();
    select(j);
    return true;
  }
  // Step `j`'s syscall summary: its runner's first of its number since it was asked.
  const syscallOf = (d) => app.calls().find((x) => x.task.pid === d.tid && x.nr === d.nr && x.id >= d.from);
  // Step `j`'s recording: its syscall selected; its stops found in it.
  function select(j) {
    const d = t.done[j], s = t.steps[j];
    const c = d && syscallOf(d);
    if (!c) return (t.status.textContent = "This call's recording is gone: the ring has moved on.");
    app.select(c);
    t.cur = { stops: s.stops ?? [], ol: d.ol };
    const b = c.last ?? m.records() - 1;
    t.at = t.cur.stops.map((st) => m.first_call(st.func, c.first, b, st.any ? undefined : c.task.addr));
    t.k = -1;
    const k = t.at.findIndex((i) => i != null);
    if (k >= 0) stop(k);
  }

  // Go to stop `k` (of the selected step): the cursor there, and its note.
  function stop(k) {
    if (t.at[k] == null) return;
    t.k = k;
    t.cur.ol?.querySelectorAll("li").forEach((li, j) => {
      li.classList.toggle("on", j === k);
      li.classList.toggle("done", j < k);
    });
    app.goStop(t.at[k], t.cur.stops[k], k, t.cur.stops.length);
  }
  const next = () => {
    for (let k = t.k + 1; k < t.at.length; k++) if (t.at[k] != null) return stop(k);
  };

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

  // Add a call, written as in C: `openat(AT_FDCWD, "/mnt/x", O_CREAT, 0644)`,
  // Enter to make it. A hint below: the syscalls the name may be, or its
  // signature (the kernel's own declaration); Tab completes the name.
  function form() {
    const names = m.syscall_names();
    const input = el("input", { className: "mono", placeholder: 'add a call: openat(AT_FDCWD, "/mnt/x", O_CREAT | O_WRONLY, 0644)',
                                spellcheck: false, autocomplete: "off" });
    const hint = el("div", { className: "mono dim hint" });
    t.input = input;
    t.hint = hint;
    const signature = (name) => {
      const s = m.syscall(name);
      return s && `${name}(${s.params.map((p) => `${p.ty}${p.ty.endsWith("*") ? "" : " "}${p.name}`).join(", ")})`;
    };
    const matches = (prefix) => names.filter((n) => n.startsWith(prefix));
    input.oninput = () => {
      const word = input.value.match(/^\s*(?:\d+\s*:\s*)?([A-Za-z_]\w*)/)?.[1] ?? "";
      if (!word) {
        const others = m.runners().filter((r) => r !== 1);
        return (hint.textContent = others.length ? `runners: 1 (/init), ${others.join(", ")}: write "${others[0]}: call(...)" for one of them` : "");
      }
      const sig = input.value.includes("(") && signature(word);
      const also = matches(word);
      hint.textContent = sig || (also.length ? also.slice(0, 8).join("  ") + (also.length > 8 ? "  …" : "") : `no syscall ${word}`);
    };
    input.onkeydown = (e) => {
      if (e.key === "Tab" && !input.value.includes("(")) {
        const pre = input.value.match(/^\s*(?:\d+\s*:\s*)?/)[0];
        const also = matches(input.value.slice(pre.length).trim());
        if (!also.length) return;
        e.preventDefault();
        // (The longest start they share; the "(" if there is only one.)
        let common = also[0];
        for (const n of also) while (!n.startsWith(common)) common = common.slice(0, -1);
        input.value = pre + (also.length === 1 ? `${common}(` : common);
        input.oninput();
      } else if (e.key === "Enter") {
        e.preventDefault();
        add();
      } else if (e.key === "Escape" && t.branch != null) {
        t.branch = null;
        input.value = hint.textContent = "";
      }
    };
    function add() {
      // (`tid: call(...)`: runner tid makes it.)
      const [, tid, rest] = input.value.match(/^\s*(?:(\d+)\s*:)?\s*([\s\S]*)$/);
      const c = parseCall(rest);
      if (!c) return (hint.textContent = "write a call: name(arg, arg, …)");
      const s = m.syscall(c.call);
      if (!s) return (hint.textContent = `no syscall ${c.call}`);
      if (c.args.length > s.params.length) return (hint.textContent = `${signature(c.call)}: ${s.params.length} arguments, not ${c.args.length}`);
      try {
        // (Checked now: $n as if the calls before were made.)
        c.args.forEach((a) => arg(a, consts, t.steps.map(() => 0)));
      } catch (e) {
        return (hint.textContent = e.message);
      }
      if (tid && !m.runners().includes(Number(tid))) return (hint.textContent = `no runner ${tid}: the runners are ${m.runners().join(", ")}`);
      const call = { call: c.call, args: c.args, ...(tid && Number(tid) !== 1 ? { tid: Number(tid) } : {}) };
      if (t.branch != null) return branch(t.branch, call);
      t.steps.push(call);
      input.value = "";
      hint.textContent = "";
      draw();
      upTo(t.steps.length - 1);
    }
    return el("div", { className: "callform" }, input, hint);
  }

  // (The example, shown while the kernel boots.)
  const u = fromURL();
  if (u.ex) open(u.ex, u.steps, u.back);
  ready(); // (The kernel is up: app gets a booted Machine.)
  return { open, next, step, list, ready };
}
