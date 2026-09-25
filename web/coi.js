// Cross-origin isolation where the server can't give it (GitHub Pages):
// the kernel's memory is a SharedArrayBuffer, which takes the COOP and COEP
// headers. On the page (not isolated), this registers itself as a service
// worker and reloads; as the worker, it adds the headers to every response.
if (typeof window === "undefined") {
  self.addEventListener("install", () => self.skipWaiting());
  self.addEventListener("activate", (e) => e.waitUntil(self.clients.claim()));
  self.addEventListener("fetch", (e) => {
    if (e.request.cache === "only-if-cached" && e.request.mode !== "same-origin") return;
    e.respondWith(fetch(e.request).then((r) => {
      if (r.status === 0) return r;
      const headers = new Headers(r.headers);
      headers.set("Cross-Origin-Opener-Policy", "same-origin");
      headers.set("Cross-Origin-Embedder-Policy", "require-corp");
      return new Response(r.body, { status: r.status, statusText: r.statusText, headers });
    }));
  });
} else if (!window.crossOriginIsolated && navigator.serviceWorker) {
  navigator.serviceWorker.register(document.currentScript.src).then((reg) => {
    // (Once: if it's in control and still not isolated, reloading won't help.)
    if (!navigator.serviceWorker.controller) {
      reg.addEventListener("updatefound", () => reg.installing.addEventListener("statechange", (e) => {
        if (e.target.state === "activated") location.reload();
      }));
      if (reg.active) location.reload();
    }
  });
}
