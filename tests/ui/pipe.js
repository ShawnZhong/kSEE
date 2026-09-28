// --pipe: the pipe example (two processes: the program, and its child running
// /bin/cat), all its calls made: the vfork that blocked finished as the
// child's execve let it go; the child's calls are its; cat's output is on
// the console.
(async () => {
  for (let i = 0; i < 120 && (document.querySelectorAll("ol.steps > li.done").length < 12 || !document.querySelector(".explain a.place.on")); i++) await sleep(100);
  const r = { calls: lis().map(line), stop: document.querySelector(".explain").innerText.replace(/\s+/g, " "), console: $("console").textContent };
  const bad = (k, want) => !r.calls[k]?.includes(want);
  // (Line k's lane: its column, 1 the program's.)
  const lane = (k) => lis()[k]?.querySelector(".cell").style.gridColumn.split(" ")[0];
  // (Its lines: int fds[2], pipe2, clone, the child's four, the program's three,
  // siginfo_t info, waitid.)
  if (document.querySelector("ol.steps > li.blocked") || bad(1, "= 0") || !/= \d+$/.test(r.calls[2] ?? "") || lane(3) !== "2" || lane(6) !== "2" || lane(7) !== "1" || bad(6, "new program")
      || bad(8, "= 6") || bad(9, "= 0") || bad(11, "= 0") || !r.stop.includes("create_pipe_files") || !r.console.includes("hello"))
    throw new Error("the pipe: " + JSON.stringify(r));
  return r;
})()
