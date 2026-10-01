// The default: the read example (#ex=read): its calls made, its subject
// (the read) selected; its links followed; stepping into, over, out, and by
// key; the chart (a click, find); Objects and Tasks; a line changed, a try
// and back; the next lesson. Returns a report.
(async () => {
  const r = {};
  if (!(await until(() => done(3) && $$(".chart .bar").length, 300))) fail("the calls made", cells());
  r.cells = cells();
  r.selected = document.querySelector(".cell.on textarea")?.value;
  if (!r.selected?.startsWith("read(")) fail("the subject call selected", r);
  r.drawn = { bars: $$(".chart .bar").length, strip: $$(".strip .run").length, source: $$(".source .ln").length, frames: $$("tr.frame").length };
  if (!r.drawn.bars || !r.drawn.strip || !r.drawn.source || !r.drawn.frames) fail("drawn", r);
  // (The links: each in its function, the source at its line.)
  r.links = [];
  for (const name of ["ksys_read", "vfs_read", "filemap_read"]) {
    $$(".explain a").find((a) => a.textContent === name).click();
    await until(() => where().startsWith(name) && document.querySelector(".source .ln.now"), 50);
    r.links.push(where());
    if (!where().startsWith(name) || !document.querySelector(".source .ln.now")) fail(`the link to ${name}`, r);
  }
  // (Stepping: on a line, over vfs_read's calls (no deeper), out of it.)
  $$(".explain a").find((a) => a.textContent === "vfs_read").click();
  await sleep(300);
  const at = where();
  press("▶"); await sleep(200);
  r.into = [at, where()];
  press("◀"); await sleep(200);
  for (let k = 0; k < 3; k++) { press("▶", { shiftKey: true }); await sleep(150); }
  r.over = where();
  press("▶", { altKey: true }); await sleep(200);
  r.out = where();
  if (r.into[1] === at || !r.over.startsWith("vfs_read") || r.out.startsWith("vfs_read")) fail("stepping", r);
  // (Keys, on the work area: → a line.)
  const work = document.querySelector(".work");
  work.focus();
  const before = where();
  work.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
  await sleep(200);
  if (where() === before) fail("→", [before, where()]);
  // (A click on a bar: the cursor there, that frame chosen.)
  const bar = $$(".chart .bar").find((b) => b.textContent.startsWith("vfs_read"));
  bar.click(); await sleep(300);
  r.clicked = [bar.textContent, where(), document.querySelector("tr.frame.on a")?.textContent];
  if (r.clicked[2] !== "vfs_read") fail("a click on a bar", r.clicked);
  // (Find: its bars lit; Enter, the cursor in it.)
  const find = document.querySelector(".find");
  find.value = "copy_page_to_iter"; find.dispatchEvent(new Event("input", { bubbles: true })); await sleep(200);
  r.found = $$(".chart .bar.found").length;
  find.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); await sleep(300);
  if (!r.found || !$$("tr.frame a").some((b) => b.textContent === "copy_page_to_iter")) fail("find", [r.found, where()]);
  // (Objects: what the read changed, its file's position; Tasks: a task_struct opened.)
  $$(".tabs .tab").find((t) => t.textContent === "Objects").click(); await sleep(300);
  r.objects = $$(".obj").map((o) => o.querySelector(".fhead").textContent);
  if (!$$(".obj td").some((td) => td.textContent === "f_pos")) fail("objects", r.objects);
  $$(".tabs .tab").find((t) => t.textContent === "Tasks").click(); await sleep(300);
  const rows = $$(".pane tr").length;
  document.querySelector(".pane a.tw").click(); await sleep(300);
  r.tasks = [rows, $$(".pane tr").length];
  if (!(r.tasks[1] > r.tasks[0] + 5) || !$$(".pane td").some((td) => td.textContent === "comm")) fail("a task_struct opened", r.tasks);
  $$(".tabs .tab").find((t) => t.textContent === "Stack").click();
  // (A line changed: the program made again; 4 bytes read.)
  write(document.querySelectorAll("ol.steps .cell textarea")[2], "read(fd, buf, 4)");
  if (!(await until(() => cells()[2]?.text === "read(fd, buf, 4)" && cells()[2].result === "= 4", 300))) fail("a line changed", cells());
  if (!location.hash.includes("&p=")) fail("the URL: the program changed", location.hash);
  // (Back to the lesson's program; a try: its program; and back.)
  document.querySelector(".ptitle a").click();
  if (!(await until(() => cells()[2]?.result === "= 18", 300))) fail("back to the lesson's program", cells());
  $$(".explain a").find((a) => a.textContent === "read twice")?.click();
  r.tried = await until(() => cells().length === 5 && done(5), 300) && cells().map((c) => c.result);
  if (!r.tried || r.tried.at(-2) !== "= 0") fail("the try", cells());
  document.querySelector(".ptitle a").click();
  if (!(await until(() => cells().length === 3 && done(3), 300))) fail("back to the lesson's program", cells());
  // (The next lesson.)
  const title = document.querySelector("h1").textContent;
  $$(".sidehead button").at(-1).click();
  if (!(await until(() => document.querySelector("h1").textContent !== title, 100))) fail("the next lesson", title);
  return r;
})()
