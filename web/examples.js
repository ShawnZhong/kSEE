// The examples. An example is a question about the kernel, and a list of
// syscalls that asks it (user/examples/*.json), each with stops (a note on a
// function it runs). /init makes the calls (process.call), one at a time,
// each recorded: then its syscall is selected, and its stops are places in
// its recording (the first call of the stop's function, on /init's task, or
// any for a stop that says so). Next goes from one stop to the next. Below
// the calls, a form adds one: a syscall by name, its parameters as fields.
//
// A step: { call, args, show?, stops? }. An argument: a number, a string, {
// size } (a buffer for the kernel to fill), { i64: [...] } (a buffer of
// 64-bit values, a struct timespec, say), or { ret: k } (step k's result, as
// an fd).
import { $, el } from "./dom.js";

const ERRNO = { 1: "EPERM", 2: "ENOENT", 9: "EBADF", 13: "EACCES", 14: "EFAULT", 17: "EEXIST", 20: "ENOTDIR", 21: "EISDIR",
                22: "EINVAL", 24: "EMFILE", 28: "ENOSPC", 36: "ENAMETOOLONG", 38: "ENOSYS", 39: "ENOTEMPTY" };

export async function examples(app) {
  const { m, proc } = app;
  const list = await fetch(new URL("./examples/index.json", location)).then((r) => (r.ok ? r.json() : [])).catch(() => []);
  // The open example: its steps, the ones run ({ ret, text, key, ol }), and
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

  const show = (a) => (typeof a === "string" ? JSON.stringify(a) : a?.size != null ? `[${a.size}]`
    : a?.i64 ? `{ ${a.i64.join(", ")} }` : a?.ret != null ? `$${a.ret + 1}` : String(a));
  const result = (r) => (r < 0 ? `${r} (${ERRNO[-r] ?? "an error"})` : String(r));
  function draw() {
    t.list.replaceChildren(...t.steps.map((s, j) => {
      const d = t.done[j];
      if (d) d.ol = s.stops?.length ? el("ol", { className: "stops" }, ...s.stops.map((st, k) => el("li", { onclick: () => {
        select(j);
        stop(k);
      } }, el("b", { className: "mono", textContent: st.func }), " ", st.note))) : null;
      return el("li", { className: d ? "done" : j === t.done.length ? "next" : "" },
        el("a", { className: "mono", textContent: s.show ?? `${s.call}(${s.args.map(show).join(", ")})`,
                  onclick: () => (d ? select(j) : upTo(j)) }),
        d ? el("span", { className: "mono ret", textContent: ` = ${result(d.ret)}` }) : null,
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
    const args = s.args.map((a) => (a?.ret != null ? t.done[a.ret]?.ret ?? -1
      : a?.i64 ? { bytes: new Uint8Array(new BigInt64Array(a.i64.map(BigInt)).buffer) } : a));
    let r = null;
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
    t.done[j] = { ret: r.ret, text: outputs(s, r.buffers), key: c && `${c.task.addr}:${m.record_id(c.first)}` };
    t.status.textContent = j + 1 < t.steps.length ? "" : "That was the last call: add one below, or click a call to see it again.";
    draw();
    select(j);
    return true;
  }
  // Step `j`'s recording: its syscall selected; its stops found in it.
  function select(j) {
    const d = t.done[j], s = t.steps[j];
    const c = app.calls().find((x) => `${x.task.addr}:${m.record_id(x.first)}` === d?.key);
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
  function outputs(s, buffers) {
    return s.args.map((a, i) => (a?.size != null && buffers?.[i] ? (s.call === "getdents64"
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

  // Add a call: a syscall by name, its parameters as fields (typed, from the
  // kernel's own declarations).
  function form() {
    const names = el("datalist", { id: "syscall-names" }, ...m.syscall_names().map((n) => el("option", { value: n })));
    const name = el("input", { className: "mono", placeholder: "add a call: openat, write, …" });
    name.setAttribute("list", "syscall-names");
    const fields = el("div", { className: "fields" });
    name.onchange = () => fields.replaceChildren(...(m.syscall(name.value)?.params ?? []).map((p) =>
      el("input", { className: "mono", placeholder: `${p.name} (${p.ty})`, title: `${p.name}: ${p.ty}` })));
    const add = el("button", { textContent: "＋ Add and make it", onclick: () => {
      const values = [...fields.querySelectorAll("input")].map((f) => f.value.trim());
      if (!m.syscall(name.value)) return (t.status.textContent = `no syscall ${name.value}`);
      try {
        t.steps.push({ call: name.value, args: values.map(arg), show: `${name.value}(${values.join(", ")})`, stops: [] });
      } catch (e) {
        return (t.status.textContent = e.message);
      }
      draw();
      upTo(t.steps.length - 1);
    } });
    return el("div", { className: "callform" }, names, name, fields, add);
  }
  function arg(v) {
    if (/^\$\d+$/.test(v)) return { ret: Number(v.slice(1)) - 1 };
    if (/^\[\d+\]$/.test(v)) return { size: Number(v.slice(1, -1)) };
    if (/^".*"$/.test(v)) return JSON.parse(v);
    if (/^-?0[0-7]+$/.test(v)) return parseInt(v, 8);
    if (/^-?(0x[0-9a-f]+|\d+)$/i.test(v)) return Number(v);
    if (v === "") return 0;
    throw new Error(`an argument is a number, "a string", [size] or $n (call n's result): ${v}`);
  }

  index();
  if (app.ready) ready();
  return { open, next, step, list, ready };
}
