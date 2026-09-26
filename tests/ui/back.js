// --back: the page opened with a program changed (the URL's p=): its calls made, the tutorial at the URL's stop.
(async () => {
    for (let i = 0; i < 100 && document.querySelectorAll("ol.steps > li.done").length < 2; i++) await sleep(100);
  const done = [...document.querySelectorAll("ol.steps > li.done")].map(line);
  const stop = document.querySelector(".tour .tnav span")?.textContent;
  if (done.length !== 2 || !done[1].includes("= 9") || !stop?.startsWith("Stop 2 of") || !location.hash.includes("stop=2"))
    throw new Error("not back: " + JSON.stringify([done, stop, location.hash]));
  return { done, stop, url: location.hash };
})()
