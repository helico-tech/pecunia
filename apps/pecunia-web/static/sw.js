// The service worker that lets Pecunia start offline and be installed on a phone's home screen. It
// keeps this release's files in a cache (`cargo xtask web` fills in the name and the list) and
// serves them from there; a new release brings a new sw.js, which swaps the cache. Books never pass
// through here: they live in the person's files or the browser's IndexedDB.
const CACHE = "__CACHE__";
const FILES = __FILES__;

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((c) => c.addAll(FILES)).then(() => self.skipWaiting()));
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const req = event.request;
  if (req.method !== "GET" || new URL(req.url).origin !== self.location.origin) return;
  // The page itself is cached as "./"; other files by their own address.
  const key = req.mode === "navigate" ? "./" : req;
  event.respondWith(caches.open(CACHE).then((c) => c.match(key, { ignoreSearch: true })).then((hit) => hit || fetch(req)));
});
