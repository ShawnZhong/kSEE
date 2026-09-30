// --pipe: the pipe example (two processes: the program, and a child
// sharing its memory, with fds of its own), all its calls made: the child's
// read, blocked, finished as the program's write woke it; the child's
// calls in its own lane, named as the timeline names its task.
(async () => {
  for (let i = 0; i < 120 && (document.querySelectorAll("ol.steps > li.done").length < 7 || !document.querySelector(".explain a.place.on")); i++) await sleep(100);
  const r = { calls: lis().map(line), lanes: [...document.querySelectorAll(".lane")].map((l) => l.textContent),
              stop: document.querySelector(".explain").innerText.replace(/\s+/g, " ") };
  const bad = (k, want) => !r.calls[k]?.includes(want);
  // (Line k's lane: its column, 1 the program's.)
  const lane = (k) => lis()[k]?.querySelector(".cell").style.gridColumn.split(" ")[0];
  // (Its lines: mmap2, int fds[2], pipe2, clone, char buf[16], the child's
  // read, the program's write.)
  if (document.querySelector("ol.steps > li.blocked") || bad(2, "= 0") || !/= \d+$/.test(r.calls[3] ?? "")
      || lane(5) !== "2" || lane(6) !== "1" || bad(5, "= 6") || bad(6, "= 6")
      || r.lanes.length !== 2 || !r.lanes.every((l) => /^prog\(\d+\)$/.test(l)) || !r.stop.includes("create_pipe_files"))
    throw new Error("the pipe: " + JSON.stringify(r));
  return r;
})()
