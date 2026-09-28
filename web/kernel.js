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

export function kernel(m, { changed } = {}) {
  // (The calls not finished yet, by id: their promises' resolves.)
  const waiting = new Map();
  // (Each call's quiet: the kernel run until it waits for the page again,
  // one after another; what it finished settles.)
  let last = Promise.resolve();
  const settle = () => (last = last.then(async () => {
    const news = await m.quiet();
    m.snapshot();
    for (const f of news.finished) {
      waiting.get(f.id)?.(f);
      waiting.delete(f.id);
    }
    changed?.(news);
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
    if (v.bytes != null || v.size != null || v.strings != null) return v; // (as Machine.call takes it already)
    if (Array.isArray(v) && v.every((x) => typeof x === "string" || x === null)) return { strings: v };
    return m.arg(name, k, c(v), {});
  }

  // Make syscall `name` with `args` (JS values: see the top) on runner
  // `tid` (/init: 1): a promise of how it finished ({ id, tid, how:
  // "returned" | "exec" | "exited", ret, buffers }), with its `id`, and
  // `quiet`: a promise of the kernel quiet after it ({ finished, runners }:
  // Machine.quiet's), whether or not it finished.
  function call(name, args = [], tid = 1) {
    const id = m.call(name, args.map((v, k) => arg(name, k, v)), tid);
    const done = new Promise((ok) => waiting.set(id, ok));
    return Object.assign(done, { id, quiet: settle() });
  }

  // A variable of the program's (see Machine.declare): `text` as in C
  // ("char buf[64]", "struct __kernel_timespec ts = {0, 30000000}"), in
  // runner `tid`'s process: { name, addr, ty, count, size, show() (as C
  // writes it, now), bytes(n) (its first n bytes, now) }; passed to a call,
  // its address. `known`: the names its initializer may use.
  function declare(text, tid = 1, known = {}) {
    const v = m.declare(tid, text, known);
    return Object.assign(v, {
      show: () => m.show(v.ty, v.addr, v.count ?? undefined),
      bytes: (n = (v.count ?? 1) * v.size) => m.memory(v.addr, n),
    });
  }

  // The syscalls, by name: sys.read(fd, buf, 64), a promise of its result
  // (a negative errno if it failed, as the kernel returns it); sys.on(tid)
  // the same, made by another runner.
  const on = (tid) => new Proxy({}, { get: (_, name) => typeof name === "string" && name !== "then"
    ? (...args) => call(name, args, tid).then((f) => f.ret) : undefined });
  const sys = new Proxy(on(1), { get: (t, name) => (name === "on" ? on : t[name]) });

  return { call, declare, sys, C: m.constants(), arg };
}
