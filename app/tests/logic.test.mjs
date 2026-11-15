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

const BORN = "1990-05-17"; // 36 on T0 (14 November 2026)
const at = (y, m, d, h = 0) => Date.UTC(y, m - 1, d, h);

test("age and birthdays follow the calendar, in UTC", () => {
  assert.equal(L.ageAt(BORN, T0), 36);
  assert.equal(L.ageAt(BORN, at(2027, 5, 16, 23)), 36);
  assert.equal(L.ageAt(BORN, at(2027, 5, 17)), 37);
  assert.equal(L.ageAt(BORN, at(1980, 1, 1)), 0);
  assert.equal(L.birthdayAt(BORN, 37), at(2027, 5, 17));
  // Born on 29 February: 1 March in a year without that day.
  assert.equal(L.birthdayAt("1996-02-29", 31), at(2027, 3, 1));
  assert.equal(L.birthdayAt("1996-02-29", 32), at(2028, 2, 29));
  assert.equal(L.ageAt("1996-02-29", at(2027, 2, 28, 23)), 30);
  assert.equal(L.ageAt("1996-02-29", at(2027, 3, 1)), 31);
  assert.ok(L.isDate("2028-02-29"));
  for (const no of ["2027-02-29", "1990-13-01", "17.05.1990", "", null]) assert.ok(!L.isDate(no), String(no));
});

test("the first verification brings the age in Capita, once", () => {
  const s = L.verify(L.newAccount("Eva", T0), T0, BORN);
  assert.equal(s.balanceCents, 3600);
  assert.equal(s.born, BORN);
  assert.ok(s.firstPaid);
  assert.deepEqual(s.history.at(-1), { t: T0, kind: "age", cents: 3600 });
  // Verified again a year later (paid from the balance): no second time.
  const later = T0 + 366 * DAY;
  const again = L.verify(L.collect(s, later).state, later, BORN);
  assert.equal(again.history.filter((h) => h.kind === "age").length, 1);
});

test("without a date of birth nothing comes by age, until one is known", () => {
  let s = L.verify(L.newAccount("Noa", T0), T0);
  assert.equal(s.balanceCents, 0);
  assert.equal(L.collect(s, T0 + 300 * DAY).collected, 0);
  assert.equal(L.nextBirthday(s, T0), null);
  // The passport check comes later, while still verified: free, and it pays.
  s = L.verify(s, T0 + 10 * DAY, BORN);
  assert.equal(s.balanceCents, 3600);
});

test("each birthday brings twice the age, all at once", () => {
  let s = L.verify(L.newAccount("Finn", T0), T0, BORN);
  assert.deepEqual(L.nextBirthday(s, T0), { t: at(2027, 5, 17), age: 37, cents: 7400 });
  let r = L.collect(s, at(2027, 5, 16, 23));
  assert.equal(r.collected, 0);
  r = L.collect(r.state, at(2027, 5, 17));
  assert.equal(r.collected, 7400);
  assert.equal(r.state.balanceCents, 3600 + 7400);
  assert.deepEqual(r.state.history.at(-1), { t: at(2027, 5, 17), kind: "birthday", cents: 7400 });
  // Never twice.
  assert.equal(L.collect(r.state, at(2027, 5, 18)).collected, 0);
});

test("a birthday older than the claim window lapses", () => {
  const s = L.verify(L.newAccount("Gil", T0), T0, BORN);
  const inTime = L.collect(s, at(2027, 5, 17) + 14 * DAY - 1);
  assert.equal(inTime.collected, 7400);
  const late = L.collect(s, at(2027, 5, 17) + 14 * DAY);
  assert.equal(late.collected, 0);
  assert.equal(late.lapsed, 7400);
  assert.equal(late.state.balanceCents, 3600);
  assert.equal(L.collect(late.state, at(2027, 6, 30)).lapsed, 0, "lapsed once");
});

test("an unverified account collects nothing", () => {
  const s = L.newAccount("Gül", T0);
  const r = L.collect(s, T0 + 10 * DAY);
  assert.equal(r.collected, 0);
  assert.equal(r.state, s);
});

test("a birthday after the verification has run out brings nothing", () => {
  // Verified one day after the birthday: the next one is 364 days on, in time.
  const start = at(2026, 5, 18);
  const s = L.verify(L.newAccount("Hana", start), start, BORN);
  assert.equal(L.collect(s, at(2027, 5, 17)).collected, 7400);
  // Verified two days before the birthday: a year later it has run out
  // one day before the next birthday.
  const early = at(2026, 5, 15);
  const t = L.verify(L.newAccount("Ida", early), early, BORN);
  const first = L.collect(t, at(2026, 5, 17));
  assert.equal(first.collected, 7200);
  assert.ok(!L.isVerified(first.state, at(2027, 5, 17)));
  assert.equal(L.collect(first.state, at(2027, 5, 17)).collected, 0);
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

test("the demo without a network verifies a person of 30", () => {
  const born = L.demoBorn(T0);
  assert.equal(born, "1996-11-14");
  assert.equal(L.ageAt(born, T0), 30);
  assert.equal(L.demoBorn(at(2028, 2, 29, 12)), "1998-03-01");
  const s = L.verify(L.newAccount("Lea", T0), T0, born);
  assert.equal(s.balanceCents, 3000);
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
  assert.equal(L.describe({ kind: "age" }), "Your age in Capita");
  assert.equal(L.describe({ kind: "birthday" }), "Birthday");
});
