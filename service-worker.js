// Fridgeflow service worker.
//
// This worker caches ONLY the public app shell (HTML, manifest, icons, the
// worker itself). It MUST NEVER cache any /api/* response, because those
// responses contain private inventory data and are gated by an authenticated
// session cookie. Caching them would both leak private data to disk and let a
// logged-out user see stale inventory.
//
// Spec references: README/10-PWA-INSTALLATION.md and
// README/11-PIN-ACCESS-AND-SECURITY.md.

const CACHE_VERSION = "fridgeflow-shell-v1";
const SHELL = [
  "/",
  "/index.html",
  "/manifest.webmanifest",
  "/icons/icon.svg",
  "/icons/icon-192.svg",
  "/icons/icon-512.svg",
];

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches.open(CACHE_VERSION).then((cache) => cache.addAll(SHELL)).catch(() => {}),
  );
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((k) => k !== CACHE_VERSION).map((k) => caches.delete(k))),
    ),
  );
  self.clients.claim();
});

self.addEventListener("fetch", (event) => {
  const req = event.request;
  if (req.method !== "GET") return; // Never cache mutations.
  const url = new URL(req.url);

  // Same-origin only; cross-origin requests bypass the worker.
  if (url.origin !== self.location.origin) return;

  // Private API responses MUST bypass the cache entirely. We also refuse to
  // store any response from /api/*, even if the browser attempted to.
  if (url.pathname.startsWith("/api/")) {
    event.respondWith(fetch(req));
    return;
  }

  // App shell: cache-first with background refresh for GET navigations.
  event.respondWith(
    caches.match(req).then((cached) => {
      if (cached) {
        // Fire-and-forget revalidate.
        fetch(req)
          .then((fresh) => {
            if (fresh && fresh.ok && isCacheable(fresh)) {
              caches.open(CACHE_VERSION).then((cache) => cache.put(req, fresh.clone()));
            }
          })
          .catch(() => {});
        return cached;
      }
      return fetch(req)
        .then((resp) => {
          if (resp && resp.ok && isCacheable(resp)) {
            const copy = resp.clone();
            caches.open(CACHE_VERSION).then((cache) => cache.put(req, copy));
          }
          return resp;
        })
        .catch(() => {
          // Last resort: for navigations fall back to the cached index shell.
          if (req.mode === "navigate") return caches.match("/index.html");
          return Response.error();
        });
    }),
  );
});

function isCacheable(response) {
  // Guard against caching anything that smells like a private/auth response
  // even if a future route starts returning one from the shell path.
  if (!response || response.type === "opaque") return false;
  if (response.headers.get("set-cookie")) return false;
  const cacheControl = response.headers.get("cache-control") || "";
  if (/no-store/i.test(cacheControl)) return false;
  return true;
}
