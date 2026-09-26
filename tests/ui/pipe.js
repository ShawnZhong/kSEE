// --pipe: the pipe example (two processes), all its calls made: the ones that blocked finished as later
// ones woke them; the child's are its.
(async () => {
    for (let i = 0; i < 100 && (document.querySelectorAll("ol.steps > li.done").length < 10 || !document.querySelector(".tour li.on")); i++) await sleep(100);
  const r = { calls: lis().map(line), stop: document.querySelector(".tour").innerText.replace(/\s+/g, " ") };
  const bad = (k, want) => !r.calls[k]?.includes(want);
  if (document.querySelector("ol.steps > li.blocked") || bad(1, "= 19") || bad(2, "tid ") || bad(2, "new program")
      || bad(5, "= 6") || bad(8, "= 0") || bad(9, "= 0") || !r.stop.includes("Stop 1 of"))
    throw new Error("the pipe: " + JSON.stringify(r));
  return r;
})()
