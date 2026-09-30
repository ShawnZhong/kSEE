// --phone: the read example on a phone's screen (390x844): the panes one
// under another, the page scrolling (ui.css). Each shown, as wide as the
// screen, the page never wider; the timeline drawn; ▶ steps.
(async () => {
  await until(() => document.querySelector(".explain a.place.on"), 300);
  const box = (id) => $(id).getBoundingClientRect();
  const r = { panes: ["examples", "chart", "src", "stack"].map((id) => [id, Math.round(box(id).width), Math.round(box(id).height)]),
              order: box("examples").top < box("chart").top && box("chart").top < box("src").top && box("src").top < box("stack").top,
              wide: document.documentElement.scrollWidth };
  // (Where it is, as the page says: two lines may share a record.)
  const at = $("where").textContent;
  $("step-fwd").click();
  await sleep(300);
  r.stepped = $("where").textContent !== at;
  if (!r.order || !r.stepped || r.wide > innerWidth || r.panes.some(([, w, h]) => w < document.documentElement.clientWidth - 60 || h < 40))
    throw new Error("the phone: " + JSON.stringify(r));
  return r;
})()
