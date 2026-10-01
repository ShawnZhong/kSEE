// --pipe: the pipe example (two processes: the program, and a child
// sharing its memory, with fds of its own): the child's read blocked,
// finished as the program's write woke it; what each call changed; both
// tasks in the strip.
(async () => {
  if (!(await until(() => done(7) && $$(".strip .run").length, 300))) fail("the calls made", cells());
  const r = { cells: cells(), changed: $$(".changed").map((c) => c.textContent) };
  // (The child's line selected: its task in the strip as the program's other.)
  document.querySelectorAll(".cells .cell textarea")[5].focus();
  await sleep(500);
  r.strip = [...new Set($$(".strip .run").map((s) => s.textContent).filter((n) => n.startsWith("prog(")))];
  if (r.cells[5].result !== "= 6 (during line 7)" || r.cells[6].result !== "= 6" || !r.changed.includes("fds = {3, 4}") || r.strip.length < 2)
    fail("the pipe", r);
  return r;
})()
