// SPDX-License-Identifier: AGPL-3.0-only
// Offline shell: the app opens without network. Change VERSION with every release.
const VERSION = "capita-proto-1";
const FILES = ["./", "index.html", "app.css", "app.js", "logic.js", "vendor/qrcode.mjs", "manifest.webmanifest",
  "icons/icon.svg", "icons/icon-192.png", "fonts/chakra-petch-latin-500-normal.woff2", "fonts/chakra-petch-latin-600-normal.woff2",
  "fonts/ibm-plex-sans-latin-400-normal.woff2", "fonts/ibm-plex-sans-latin-500-normal.woff2"];
self.addEventListener("install", (e) => e.waitUntil(caches.open(VERSION).then((c) => c.addAll(FILES)).then(() => self.skipWaiting())));
self.addEventListener("activate", (e) => e.waitUntil(
  caches.keys().then((ks) => Promise.all(ks.filter((k) => k !== VERSION).map((k) => caches.delete(k)))).then(() => self.clients.claim())));
self.addEventListener("fetch", (e) => {
  if (e.request.method !== "GET") return;
  e.respondWith(caches.match(e.request).then((hit) => hit || fetch(e.request)));
});
