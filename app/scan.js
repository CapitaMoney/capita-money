// SPDX-License-Identifier: AGPL-3.0-only
// Reading a QR code with the camera: the code of a laptop or a person to
// verify, of a new device to link. The browser's own detector where there
// is one, else jsQR (vendor/jsQR.js, Apache-2.0, loaded only when needed) –
// no Google service either way on phones without one.

let jsqr = null;
function loadJsQR() {
  if (globalThis.jsQR) return Promise.resolve(globalThis.jsQR);
  jsqr ??= new Promise((resolve, reject) => {
    const s = document.createElement("script");
    s.src = "vendor/jsQR.js";
    s.onload = () => resolve(globalThis.jsQR);
    s.onerror = () => reject(new Error("QR reader missing"));
    document.head.append(s);
  });
  return jsqr;
}

async function detector() {
  try {
    if ("BarcodeDetector" in globalThis && (await BarcodeDetector.getSupportedFormats()).includes("qr_code")) {
      const d = new BarcodeDetector({ formats: ["qr_code"] });
      return async (video) => (await d.detect(video))[0]?.rawValue ?? null;
    }
  } catch { /* fall back to jsQR */ }
  const read = await loadJsQR();
  const c = document.createElement("canvas");
  const g = c.getContext("2d", { willReadFrequently: true });
  return async (video) => {
    const w = video.videoWidth, h = video.videoHeight;
    if (!w || !h) return null;
    const scale = Math.min(1, 640 / Math.max(w, h));
    c.width = Math.round(w * scale); c.height = Math.round(h * scale);
    g.drawImage(video, 0, 0, c.width, c.height);
    return read(g.getImageData(0, 0, c.width, c.height).data, c.width, c.height, { inversionAttempts: "dontInvert" })?.data ?? null;
  };
}

/**
 * Opens the back camera into `video` and answers the first code `accept`
 * takes (it answers true). `signal` stops it. The camera is closed after.
 */
export async function scan(video, accept, signal) {
  const stream = await navigator.mediaDevices.getUserMedia({ video: { facingMode: "environment" }, audio: false });
  try {
    video.srcObject = stream;
    video.muted = true;
    video.playsInline = true;
    await video.play();
    const look = await detector();
    for (;;) {
      if (signal?.aborted) throw new DOMException("stopped", "AbortError");
      const text = await look(video).catch(() => null);
      if (text && accept(text)) return text;
      await new Promise((r) => setTimeout(r, 200));
    }
  } finally {
    for (const t of stream.getTracks()) t.stop();
    video.srcObject = null;
  }
}
