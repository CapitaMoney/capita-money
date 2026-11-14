// SPDX-License-Identifier: AGPL-3.0-only
// Filming the head movements of the face check (ADR 0017, ADR 0021).
// The identity service names the movements; the app asks for each in plain
// words and takes pictures as the camera sees them (not mirrored – the
// service expects that). Pictures go to the service and are dropped there.
// Works in a browser (laptop camera) and in the Android shell.

/** What the person is asked, per movement word of the service. */
export const ASK = Object.freeze({
  straight: "Look straight at the camera",
  left: "Turn your head to your left",
  right: "Turn your head to your right",
  up: "Look up",
  down: "Look down",
});

/** The service takes 4 to 60 pictures per answer (capita-face, Settings::frames). */
export const FRAMES = Object.freeze({ min: 4, max: 60 });

/**
 * The plan of a film: a straight look, each movement, a straight look –
 * each step as [word, milliseconds]. Pure; tested.
 */
export function plan(moves, { straightMs = 1200, moveMs = 1600 } = {}) {
  return [["straight", straightMs], ...moves.map((m) => [m, moveMs]), ["straight", straightMs]];
}

/** How many pictures a plan takes at `fps` (at least one per step). */
export const pictures = (steps, fps) => steps.reduce((n, [, ms]) => n + Math.max(1, Math.round((ms / 1000) * fps)), 0);

/** The highest picture rate ≤ `fps` that keeps a plan within the service's limits. */
export function rateFor(steps, fps = 4) {
  let r = fps;
  while (r > 1 && pictures(steps, r) > FRAMES.max) r -= 1;
  return r;
}

/** Opens the front camera into `video`. Answers the stream (stop it with `close`). */
export async function openCamera(video) {
  const stream = await navigator.mediaDevices.getUserMedia({
    video: { facingMode: "user", width: { ideal: 640 }, height: { ideal: 480 } },
    audio: false,
  });
  video.srcObject = stream;
  video.muted = true;
  video.playsInline = true;
  await video.play();
  return stream;
}

export function close(stream) {
  for (const t of stream?.getTracks() ?? []) t.stop();
}

/** One picture of what the camera sees now, as a JPEG data address. */
export function grab(video, quality = 0.85) {
  const c = document.createElement("canvas");
  c.width = video.videoWidth;
  c.height = video.videoHeight;
  c.getContext("2d").drawImage(video, 0, 0);
  return c.toDataURL("image/jpeg", quality);
}

const pause = (ms) => new Promise((r) => setTimeout(r, ms));

/**
 * Films the movements. `say(text, word)` shows each request on the screen.
 * Answers the pictures in order.
 */
export async function filmMoves({ video, moves, say, fps = 4 }) {
  const steps = plan(moves);
  const rate = rateFor(steps, fps);
  const frames = [];
  for (const [word, ms] of steps) {
    say(ASK[word] ?? word, word);
    const n = Math.max(1, Math.round((ms / 1000) * rate));
    for (let i = 0; i < n; i++) {
      await pause(1000 / rate);
      frames.push(grab(video));
    }
  }
  return frames;
}
