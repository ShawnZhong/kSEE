// The kernel, for the page's program: its lines made one after another
// (Machine.run, each recorded), and the kernel run until it is quiet after
// each. The page's program and the tests make their lines through it.
//
//   const k = kernel(m);                              // m: a Machine (ksee.js)
//   const p = runner(k, m, ['char buf[64]', 'int fd = openat(AT_FDCWD, "/note.txt", O_RDONLY)', 'read(fd, buf, 64)'].map((l) => m.statement(l)));
//   while (await p.next());                           // each: { j, decl }, { j, assigned } or { j, q, news }

// `snapshot`: after each quiet, a new snapshot of the recording (the page's
// refresh: it redraws; else Machine.snapshot).
export function kernel(m, { snapshot = () => m.snapshot() } = {}) {
  // (Each quiet: the kernel run until it waits for the page again, one
  // after another. A quiet cut short, by a restart, does not stop the ones
  // after.)
  let last = Promise.resolve();
  const quiet = () => (last = last.catch(() => {}).then(async () => {
    const news = await m.quiet();
    snapshot();
    return news;
  }));

  return { quiet };
}

// A step's line, as written, its runner's `tid: ` first.
export const text = (s) => `${s.tid != null ? `${s.tid}: ` : ""}${s.text}`;

// A program (the page's: its lines, `steps`, each as Machine.statement
// has it), made one line after another, on a fresh kernel: how each went,
// the runner each call started, the variables.
export function runner(k, m, steps) {
  // (Per step: how it went: a call's { id, tid, how?, ret?, at? (during
  // which step it finished) }, a declaration's { decl, how }, an
  // assignment's { shown, how }.) The variables: { name, addr, ty, count,
  // size, j }.
  const done = [], started = [], vars = [];
  // Calls are numbered from 1, as the explanations have them: the
  // declarations and assignments are not calls. Call `n`'s step.
  const isCall = (s) => s.call != null;
  const stepOf = (n) => steps.findIndex((s, i) => isCall(s) && steps.slice(0, i + 1).filter(isCall).length === n);
  // Make the next step (null if none), and wait until the kernel is quiet
  // after a call ({ j, q, news }: q gets how it went, and so do the calls
  // before it finished meanwhile).
  async function next() {
    const j = done.length, s = steps[j];
    if (!s) return null;
    if (s.error) throw new Error(s.error); // (a line that is no statement)
    const before = new Set(m.runners()), r = m.run(text(s));
    if (r.decl) {
      vars.push(Object.assign(r.decl, { j }));
      done[j] = { decl: r.decl, how: "declared" };
      return { j, decl: r.decl };
    }
    if (r.assigned != null) {
      done[j] = { how: "assigned", shown: r.assigned };
      return { j, assigned: r.assigned };
    }
    const q = (done[j] = { id: r.id, tid: r.tid });
    const news = await k.quiet();
    started[j] = [...news.runners].find((t) => !before.has(t));
    for (const f of news.finished) {
      const d = done.find((x) => x?.id === f.id);
      if (d) Object.assign(d, { how: f.how, ret: f.ret, at: j });
    }
    return { j, q, news };
  }
  return { steps, done, started, vars, stepOf, next };
}
