// Syscalls from JavaScript: the kernel as a library. The page's program
// lines (examples.js), the tests and the console (window.ksee.kernel) all
// make their calls through it.
//
//   const k = kernel(m);                              // m: a Machine (ksee.js)
//   const buf = k.declare("char buf[64]");            // the program's memory
//   const fd = await k.sys.openat(k.C.AT_FDCWD, "/note.txt", k.C.O_RDONLY);
//   const n = await k.sys.read(fd, buf, 64);          // 18
//   buf.show();                                       // '"hello from memory\n"'
//   await k.sys.clock_nanosleep_time64(k.C.CLOCK_MONOTONIC, 0, { tv_sec: 0, tv_nsec: 30000000 }, null);
//   k.sys.on(child).read(0, buf, 64);                 // another runner (a thread, a child)
//
// Arguments are JS values, taken as C would: a number (or a BigInt) as it
// is; a string as a C string; a Uint8Array as its bytes; null as NULL; an
// array of strings (and nulls) as argv; a variable (declare) as its
// address; a plain object (or array) as a compound literal, laid out as the
// parameter's type says ({ tv_sec: 0, tv_nsec: 30000000 }: a struct
// __kernel_timespec). A call is recorded, as any the page makes; its
// promise settles when it returns (a call that blocks stays pending while
// others go on, until one wakes it: as threads do).

// Value `v` written as C (the language the core lays arguments out by:
// Machine.arg).
const c = (v) => v === null || v === undefined ? "NULL"
  : typeof v === "string" ? `"${v.replace(/[\\"]/g, (x) => `\\${x}`).replace(/\n/g, "\\n").replace(/\t/g, "\\t").replace(/\0/g, "\\0")}"`
  : Array.isArray(v) ? `{${v.map(c).join(", ")}}`
  : typeof v === "object" ? `{${Object.entries(v).map(([k, x]) => `.${k} = ${c(x)}`).join(", ")}}`
  : String(v);

export function kernel(m) {
  // (The calls not finished yet, by id: their promises' resolves.)
  const waiting = new Map();
  // (Each call's quiet: the kernel run until it waits for the page again,
  // one after another; what it finished settles. A quiet cut short, by a
  // restart, does not stop the ones after.)
  let last = Promise.resolve();
  const settle = () => (last = last.catch(() => {}).then(async () => {
    const news = await m.quiet();
    m.snapshot();
    for (const f of news.finished) {
      waiting.get(f.id)?.(f);
      waiting.delete(f.id);
    }
    return news;
  }));

  // Argument `k` of syscall `name`, from its JS value: as Machine.call takes it.
  function arg(name, k, v) {
    if (typeof v === "number") return v;
    if (typeof v === "bigint") return Number(v);
    if (typeof v === "string") return v;
    if (v === null || v === undefined) return 0;
    if (v instanceof Uint8Array) return { bytes: v };
    if (v.addr != null && v.ty != null) return v.addr; // (a variable)
    if (v.bytes != null || v.strings != null) return v; // (as Machine.call takes it already)
    if (Array.isArray(v) && v.every((x) => typeof x === "string" || x === null)) return { strings: v };
    return m.arg(name, k, c(v), {});
  }

  // A call's result as the page says it: a number (in hex past 0xffff: a
  // pointer, a flag word), a failure with its errno's name: "-2 (ENOENT)".
  const errno = (r) => (r < 0 ? m.errno(-r) : undefined);
  const result = (r) => (r > 0xffff ? `0x${r.toString(16)}` : r >= 0 ? String(r) : `${r} (${errno(r) ?? "an error"})`);

  // Make syscall `name` with `args` (JS values: see the top) on runner
  // `tid` (none: the program's own, Machine.main): a promise of how it finished ({ id, tid, how:
  // "returned" | "exec" | "exited", ret }), with its `id`, and
  // `quiet`: a promise of the kernel quiet after it ({ finished, runners }:
  // Machine.quiet's), whether or not it finished.
  function call(name, args = [], tid) {
    const id = m.call(name, args.map((v, k) => arg(name, k, v)), tid);
    const done = new Promise((ok) => waiting.set(id, ok));
    return Object.assign(done, { id, quiet: settle() });
  }

  // A variable of the program's (see Machine.declare): `text` as in C
  // ("char buf[64]", "struct __kernel_timespec ts = {0, 30000000}"), in
  // runner `tid`'s process: { name, addr, ty, count, size, get() (a JS
  // value, as it is now: "ok", [3, 4], { tv_sec: 0, tv_nsec: 30000000 }),
  // set(x) (x a JS value, laid out as its type says; recorded), show() (as
  // C writes it), bytes(n) (its first n bytes) }; passed to a call, its
  // address. `known`: the names its initializer may use.
  function declare(text, tid, known = {}) {
    const v = m.declare(tid, text, known), count = v.count ?? undefined;
    return Object.assign(v, {
      get: () => m.get(v.ty, v.addr, count),
      set: (x) => m.set(v.ty, v.addr, count, c(x), {}),
      show: () => m.show(v.ty, v.addr, count),
      bytes: (n = (v.count ?? 1) * v.size) => m.memory(v.addr, n),
    });
  }

  // The syscalls, by name: sys.read(fd, buf, 64), a promise of its result
  // (a negative errno if it failed, as the kernel returns it); sys.on(tid)
  // the same, made by another runner.
  const on = (tid) => new Proxy({}, { get: (_, name) => typeof name === "string" && name !== "then"
    ? (...args) => call(name, args, tid).then((f) => f.ret) : undefined });
  const sys = new Proxy(on(undefined), { get: (t, name) => (name === "on" ? on : t[name]) });

  return { call, declare, sys, C: m.constants(), arg, errno, result };
}

// A program (the page's: its lines, `steps`), made one step after another:
// a call ({ call, args (as written, in C: Machine.arg), name?, tid? }) or a
// variable's declaration ({ decl }). What the page's program and the tests
// share: the results so far by name and by number, the variables, whose
// runner makes a call.
//   const p = runner(k, m, steps);
//   while (await p.next());          // each: { j, decl } or { j, q, news }
export function runner(k, m, steps) {
  // (Per step: how it went ({ id, tid, args, how?, ret? }, or {
  // decl }); the runner it started, if any. The variables: { name, addr,
  // ty, count, size, j, get(), … }.)
  const done = [], started = [], vars = [];
  // Calls are numbered from 1, as $n and the explanations have them: the
  // declarations are not calls. Step `i`'s number; call `n`'s step.
  const callNumber = (i) => steps.slice(0, i + 1).filter((s) => !s.decl).length;
  const stepOf = (n) => steps.findIndex((s, i) => !s.decl && callNumber(i) === n);
  // The names the first `j` steps make: $n and the name of each call's
  // result; each variable's address, an array's elements as they are now.
  // (`runners`: a call not returned yet is, as a runner, the one it started:
  // a vfork's child, before the clone returns.)
  function known(j = done.length, runners = false) {
    const o = {};
    for (const v of vars) {
      if (v.j >= j) continue;
      o[v.name] = v.addr;
      // (A char array's elements: its bytes, as C has them.)
      const now = v.count == null ? null : typeof v.get() === "string" ? [...v.bytes()] : v.get();
      if (Array.isArray(now)) now.forEach((x, i) => typeof x === "number" && (o[`${v.name}[${i}]`] = x));
    }
    done.slice(0, j).forEach((d, i) => {
      const r = d?.ret ?? (runners ? started[i] : null);
      if (steps[i].decl || r == null) return;
      o[`$${callNumber(i)}`] = r;
      if (steps[i].name) o[steps[i].name] = r;
    });
    return o;
  }
  // Step `s`'s runner (a thread id): as written, a number or a result;
  // else the program's own (Machine.main).
  const tidOf = (s) => (s.tid == null ? m.main() : typeof s.tid === "number" ? s.tid : m.value(String(s.tid), known(done.length, true)));
  // Make the next step (null if none): a variable declared, or a call made
  // and the kernel run until it is quiet ({ j, q, news }; q gets how it
  // went, and so do the calls before it finished meanwhile).
  async function next() {
    const j = done.length, s = steps[j];
    if (!s) return null;
    if (s.decl) {
      const v = Object.assign(k.declare(s.decl, tidOf(s), known()), { j });
      vars.push(v);
      done[j] = { decl: v, how: "declared" };
      return { j, decl: v };
    }
    const args = s.args.map((a, i) => m.arg(s.call, i, a, known())), tid = tidOf(s);
    const before = new Set(m.runners()), made = k.call(s.call, args, tid);
    const q = (done[j] = { id: made.id, tid, args });
    const news = await made.quiet;
    started[j] = [...news.runners].find((r) => !before.has(r));
    for (const f of news.finished) {
      const d = done.find((x) => x?.id === f.id);
      if (d) Object.assign(d, { how: f.how, ret: f.ret, at: j }); // (at: during which step it finished)
    }
    return { j, q, news };
  }
  return { steps, done, started, vars, known, tidOf, stepOf, next };
}
