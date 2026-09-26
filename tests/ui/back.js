// --back: the page opened with a program changed (the URL's p=): its calls made, the explanation's links
// resolved against them (the ones into the changed call gone, if its function does not run).
(async () => {
  for (let i = 0; i < 100 && document.querySelectorAll("ol.steps > li.done").length < 2; i++) await sleep(100);
  await sleep(500);
  const done = [...document.querySelectorAll("ol.steps > li.done")].map(line);
  const links = [...document.querySelectorAll(".explain .place")].map((a) => `${a.textContent}${a.classList.contains("gone") ? " (gone)" : ""}`);
  if (done.length !== 2 || !done[1].includes("= 9") || !links.length || !document.querySelector(".explain a.place.on"))
    throw new Error("not back: " + JSON.stringify([done, links, location.hash]));
  return { done, links, url: location.hash };
})()
