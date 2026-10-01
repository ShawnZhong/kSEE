// --back: the page opened with a program changed (the URL's p=): its lines
// made, said to be a try, and the lesson's own program a click away.
(async () => {
  if (!(await until(() => done(2), 300))) fail("the program made", cells());
  const r = { cells: cells(), note: document.querySelector(".lesson p.dim")?.textContent ?? "" };
  if (r.cells.length !== 2 || r.cells[1].result !== "= 9" || !r.note.includes("not the lesson's own program")) fail("not back", r);
  return r;
})()
