// SPDX-License-Identifier: AGPL-3.0-only
// Run: node --test app/tests/
import test from "node:test";
import assert from "node:assert/strict";
import * as L from "../logic.js";

const T0 = Date.UTC(2026, 10, 14, 9, 0, 0);
const DAY = 86400000;

test("a new account holds the start grant, earmarked", () => {
  const s = L.newAccount("  Anna   Müller ", T0);
  assert.equal(s.name, "Anna Müller");
  assert.equal(s.balanceCents, 100);
  assert.equal(L.spendableCents(s), 0);
  assert.equal(s.history[0].kind, "gift");
  assert.throws(() => L.newAccount("   ", T0));
});

test("the start grant pays the verification, then nothing is earmarked", () => {
  const s = L.verify(L.newAccount("Ben", T0), T0);
  assert.equal(s.balanceCents, 0);
  assert.equal(s.reservedCents, 0);
  assert.ok(L.isVerified(s, T0 + DAY));
  assert.equal(L.daysLeft(s, T0), 365);
  assert.equal(L.verify(s, T0 + DAY), s, "verifying twice changes nothing");
});

test("no debt: nothing can be sent beyond what is there", () => {
  const s = L.newAccount("Cem", T0);
  assert.throws(() => L.send(s, "Dana", 1, T0), /funds/);
  const r = L.receive(s, "Dana", 250, T0);
  assert.equal(L.spendableCents(r), 250);
  const out = L.send(r, "Dana", 250, T0);
  assert.equal(out.balanceCents, 100);
  assert.throws(() => L.send(out, "Dana", 1, T0), /funds/);
  assert.throws(() => L.send(r, "Dana", 0, T0), /amount/);
  assert.throws(() => L.send(r, "Dana", 1.5, T0), /amount/);
});

test("a verified person collects 10.00 a month, lazily", () => {
  let s = L.verify(L.newAccount("Eva", T0), T0);
  const month = L.RULES.monthMs;
  // Ten collections of a tenth of a month each sum to the month, no cent lost.
  let total = 0;
  for (let i = 1; i <= 10; i++) {
    const r = L.collect(s, T0 + Math.round((month * i) / 10));
    s = r.state;
    total += r.collected;
  }
  assert.ok(total === 1000 || total === 999, `total ${total}`);
  const next = L.collect(s, T0 + month + 1000);
  assert.ok(total + next.collected === 1000 || total + next.collected === 999);
});

test("money older than the claim window lapses", () => {
  const s = L.verify(L.newAccount("Finn", T0), T0);
  const r = L.collect(s, T0 + 30 * DAY);
  // 14 days collected, 16 days lapsed.
  const perDay = 1000 / (365.25 / 12);
  assert.ok(Math.abs(r.collected - 14 * perDay) <= 1, `collected ${r.collected}`);
  assert.ok(Math.abs(r.lapsed - 16 * perDay) <= 1, `lapsed ${r.lapsed}`);
  assert.equal(r.state.balanceCents, r.collected);
});

test("an unverified account collects nothing", () => {
  const s = L.newAccount("Gül", T0);
  const r = L.collect(s, T0 + 10 * DAY);
  assert.equal(r.collected, 0);
  assert.equal(r.state, s);
});

test("nothing accrues after the verification has run out", () => {
  let s = L.verify(L.newAccount("Hana", T0), T0);
  s = L.collect(s, T0 + 360 * DAY).state;
  const a = L.collect(s, T0 + 365 * DAY).collected;
  const b = L.collect(s, T0 + 400 * DAY).collected;
  assert.equal(a, b);
  assert.ok(!L.isVerified(s, T0 + 366 * DAY));
});

test("renewal a year later is paid from the balance", () => {
  let s = L.verify(L.newAccount("Ivo", T0), T0);
  s = L.receive(s, "Jo", 500, T0 + DAY);
  const later = T0 + 366 * DAY;
  const r = L.verify(s, later);
  assert.equal(r.balanceCents, 400);
  assert.ok(L.isVerified(r, later));
  const poor = L.verify(L.newAccount("Kai", T0), T0);
  assert.throws(() => L.verify(poor, later), /funds/);
});

test("collections on one day show as one line", () => {
  let s = L.verify(L.newAccount("Lea", T0), T0);
  s = L.collect(s, T0 + 2 * 3600000).state;
  s = L.collect(s, T0 + 5 * 3600000).state;
  assert.equal(s.history.filter((h) => h.kind === "monthly").length, 1);
});

test("amounts: parse and format", () => {
  assert.equal(L.parseAmount("12"), 1200);
  assert.equal(L.parseAmount("12.5"), 1250);
  assert.equal(L.parseAmount("12,05"), 1205);
  assert.equal(L.parseAmount("0.01"), 1);
  assert.equal(L.parseAmount("1.234"), null);
  assert.equal(L.parseAmount("-1"), null);
  assert.equal(L.parseAmount(""), null);
  assert.equal(L.formatCents(100), "1.00");
  assert.equal(L.formatCents(123456789), "1 234 567.89");
  assert.equal(L.formatCents(-5), "−0.05");
});

test("history speaks plain words, never codes", () => {
  assert.equal(L.describe({ kind: "in", who: "Anna" }), "From Anna");
  assert.equal(L.describe({ kind: "gift" }), "Welcome gift");
});
