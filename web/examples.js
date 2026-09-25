// The examples. An example is a question about the kernel, and a list of
// syscalls that asks it (user/examples/*.json), each with stops (a note on a
// function it runs). /init makes the calls (process.call), one at a time,
// each recorded: then its syscall is selected, and its stops are places in
// its recording (the first call of the stop's function, on /init's task, or
// any for a stop that says so). Next goes from one stop to the next. Below
// the calls, add one: write it as in C, and Enter.
//
// A step: { call, args, stops? }; its arguments written as in C (see
// args.js): "a string", 0644, O_CREAT | O_WRONLY, [64], {0, 30000000}, $2.
import { $, el } from "./dom.js";
import { arg, made, parseCall, result } from "./args.js";

export async function examples(app) {
  const { m, proc } = app;
  const consts = m.constants();
  const list = await fetch(new URL("./examples/index.json", location)).then((r) => (r.ok ? r.json() : [])).catch(() => []);
  // The open example: its steps, the ones run ({ ret, text, id, ol }), and
  // the stops Next goes through (the selected step's: cur, at, k).
  const t = { ex: null, steps: [], done: [], running: false, cur: { stops: [], ol: null }, at: [], k: -1, list: null, status: null };

  function index() {
    t.ex = null;
    const row = (x) => el("div", { className: "row example", onclick: () => open(x) },
      el("div", { className: "tt", textContent: x.title }), x.question ? el("div", { className: "meta", textContent: x.question }) : null);
    $("examples").replaceChildren(
      el("div", { className: "note", textContent: "Each asks a question about the kernel with a few syscalls: open one, make its calls, and follow their stops." }),
      ...list.map(row),
      el("div", { className: "note dim", textContent: "Your own: a JSON file in user/examples/, then ./ksee.sh build." }));
  }

  function open(ex) {
    t.ex = ex;
    t.steps = ex.steps.map((s) => ({ ...s })); // (the form adds to them)
    t.done = [];
    t.cur = { stops: [], ol: null };
    t.at = [];
    t.k = -1;
    t.status = el("div", { className: "tstatus dim" });
    t.list = el("ol", { className: "steps" });
    $("examples").replaceChildren(
      el("div", { className: "thead" },
        el("a", { textContent: "‹ examples", onclick: index }),
        el("div", { className: "tt", textContent: ex.title }),
        ex.question ? el("div", { className: "q", textContent: ex.question }) : null,
        el("div", { className: "tbar" }, el("button", { className: "go", textContent: "▶ Next call", onclick: () => step() }))),
      t.list, form(), t.status);
    draw();
  }

  // The kernel is up: open the first example, and make its first call.
  function ready() {
    if (!t.ex && list.length) {
      open(list[0]);
      step();
    }
  }

  const rets = () => t.done.map((d) => d.ret);
  // A step's call, as written (or with `values` for $n: as made).
  const text = (s, values) => `${s.call}(${s.args.map((a) => (values ? made(a, values) : a)).join(", ")})`;
  function draw() {
    t.list.replaceChildren(...t.steps.map((s, j) => {
      const d = t.done[j];
      if (d) d.ol = s.stops?.length ? el("ol", { className: "stops" }, ...s.stops.map((st, k) => el("li", { onclick: () => {
        select(j);
        stop(k);
      } }, el("b", { className: "mono", textContent: st.func }), " ", st.note))) : null;
      return el("li", { className: d ? "done" : j === t.done.length ? "next" : "" },
        // (As written; once made, and if $n made it differ, as made too.)
        el("a", { className: "mono", textContent: text(s), onclick: () => (d ? select(j) : upTo(j)) }),
        d ? el("span", { className: "mono ret", textContent: ` = ${result(d.ret, consts)}` }) : null,
        d && text(s, rets()) !== text(s) ? el("div", { className: "mono dim", textContent: `as made: ${text(s, rets())}` }) : null,
        d?.text ? el("div", { className: "mono dim", textContent: d.text }) : null,
        d?.ol ?? null);
    }));
  }

  // Make the calls up to step `j`.
  async function upTo(j) {
    while (t.done.length <= j) if (!(await step())) return;
  }
  // Make the next call, recorded; then select it.
  async function step() {
    const j = t.done.length, s = t.steps[j];
    if (!s || t.running) return false;
    t.running = true;
    t.status.textContent = `${s.call}…`;
    let r = null, args;
    try {
      args = s.args.map((a) => arg(a, consts, rets()));
    } catch (e) {
      t.running = false;
      t.status.textContent = e.message;
      return false;
    }
    m.record(true);
    try {
      r = await proc.call(s.call, args);
    } catch (e) {
      t.status.textContent = e.message;
    } finally {
      m.record(false);
      t.running = false;
    }
    if (!r) return false;
    app.refresh();
    // (Its syscall: the last of them /init made; it waits for the next call in a read, not returned.)
    const nr = m.syscall(s.call).nr, c = app.calls().findLast((x) => x.nr === nr && x.task.pid === 1 && x.last != null);
    t.done[j] = { ret: r.ret, text: outputs(s.call, args, r.buffers), id: c?.id };
    t.status.textContent = j + 1 < t.steps.length ? "" : "That was the last call: add one below, or click a call to see it again.";
    draw();
    select(j);
    return true;
  }
  // Step `j`'s recording: its syscall selected; its stops found in it.
  function select(j) {
    const d = t.done[j], s = t.steps[j];
    const c = app.calls().find((x) => x.id === d?.id);
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
    return args.map((a, i) => (a?.size != null && buffers?.[i] ? (call === "getdents64"
      ? `entries: ${dirents(buffers[i]).join(", ")}` : bytes(buffers[i])) : null)).filter(Boolean).join("  ");
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
    const signature = (name) => {
      const s = m.syscall(name);
      return s && `${name}(${s.params.map((p) => `${p.ty}${p.ty.endsWith("*") ? "" : " "}${p.name}`).join(", ")})`;
    };
    const matches = (prefix) => names.filter((n) => n.startsWith(prefix));
    input.oninput = () => {
      const word = input.value.match(/^\s*([A-Za-z_]\w*)/)?.[1] ?? "";
      if (!word) return (hint.textContent = "");
      const sig = input.value.includes("(") && signature(word);
      const also = matches(word);
      hint.textContent = sig || (also.length ? also.slice(0, 8).join("  ") + (also.length > 8 ? "  …" : "") : `no syscall ${word}`);
    };
    input.onkeydown = (e) => {
      if (e.key === "Tab" && !input.value.includes("(")) {
        const also = matches(input.value.trim());
        if (!also.length) return;
        e.preventDefault();
        // (The longest start they share; the "(" if there is only one.)
        let common = also[0];
        for (const n of also) while (!n.startsWith(common)) common = common.slice(0, -1);
        input.value = also.length === 1 ? `${common}(` : common;
        input.oninput();
      } else if (e.key === "Enter") {
        e.preventDefault();
        add();
      }
    };
    function add() {
      const c = parseCall(input.value);
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
      t.steps.push({ call: c.call, args: c.args });
      input.value = "";
      hint.textContent = "";
      draw();
      upTo(t.steps.length - 1);
    }
    return el("div", { className: "callform" }, input, hint);
  }

  index();
  if (app.ready) ready();
  return { open, next, step, list, ready };
}
