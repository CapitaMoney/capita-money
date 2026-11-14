// SPDX-License-Identifier: AGPL-3.0-only
import test from "node:test";
import assert from "node:assert/strict";
import { plan, pictures, rateFor, FRAMES, ASK } from "../native/film.js";
import { yymmdd } from "../native/chip.js";

test("a film: straight, the movements, straight", () => {
  const steps = plan(["left", "up"]);
  assert.deepEqual(steps.map(([w]) => w), ["straight", "left", "up", "straight"]);
  for (const [w] of steps) assert.ok(ASK[w], w);
});

test("the pictures stay within what the service takes", () => {
  for (const n of [1, 2, 3, 4, 8, 20]) {
    const moves = Array.from({ length: n }, (_, i) => ["left", "right", "up", "down"][i % 4]);
    const steps = plan(moves);
    const p = pictures(steps, rateFor(steps));
    assert.ok(p >= FRAMES.min && p <= FRAMES.max, `${n} moves: ${p} pictures`);
  }
});

test("dates for the chip key", () => {
  assert.equal(yymmdd("1974-08-12"), "740812");
});
