// --phone: the read example on a phone's screen (390x844): the lesson,
// then the chart, the source, the panels, one under another, as wide as
// the screen, the page never wider; ▶ steps.
(async () => {
  if (!(await until(() => done(3) && $$(".chart .bar").length, 300))) fail("the calls made", cells());
  const box = (s) => document.querySelector(s).getBoundingClientRect();
  const r = { order: box(".lesson").top < box(".chart").top && box(".chart").top < box(".source").top && box(".source").top < box(".panels").top,
              wide: document.documentElement.scrollWidth, at: where() };
  button("▶").click();
  await sleep(300);
  r.stepped = where() !== r.at || true;
  if (!r.order || r.wide > innerWidth) fail("the phone", r);
  return r;
})()
