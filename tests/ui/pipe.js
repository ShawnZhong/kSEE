// --pipe: the pipe example (two processes: /init, and its child running
// /bin/cat), all its calls made: the vfork that blocked finished as the
// child's execve let it go; the child's calls are its; cat's output is on
// the console.
(async () => {
  for (let i = 0; i < 100 && (document.querySelectorAll("ol.steps > li.done").length < 10 || !document.querySelector(".tour li.on")); i++) await sleep(100);
  const r = { calls: lis().map(line), stop: document.querySelector(".tour").innerText.replace(/\s+/g, " "), console: $("console").textContent };
  const bad = (k, want) => !r.calls[k]?.includes(want);
  if (document.querySelector("ol.steps > li.blocked") || bad(1, "= 19") || bad(2, "tid ") || bad(5, "tid ") || bad(5, "new program")
      || bad(7, "= 6") || bad(8, "= 0") || bad(9, "= 0") || !r.stop.includes("Stop 1 of") || !r.console.includes("hello"))
    throw new Error("the pipe: " + JSON.stringify(r));
  return r;
})()
