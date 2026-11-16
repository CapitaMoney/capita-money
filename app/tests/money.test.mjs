// SPDX-License-Identifier: AGPL-3.0-only
// The money of an account on the ledger as the screens show it (money.js),
// and the payment code (device.js). What a claim brings is checked against
// the runtime's own numbers in chain-read.test.mjs.
import test from "node:test";
import assert from "node:assert/strict";
import * as M from "../money.js";
import * as D from "../device.js";

const DAY = 86400;
const T0 = 1800000000; // 15 January 2027, seconds
const rules = [{
  version: 1, validFrom: 0, startGrantCents: 100, claimWindow: 14 * DAY, ripening: 0,
  age: { minLevel: 1, perYearCents: 200, atVerificationCents: 100, byCountry: [{ country: "DE", perYearCents: 250 }] },
}];
const account = (over = {}) => ({
  person: true, status: "active", openedAt: T0, level: 1, levelUntil: T0 + 300 * DAY, country: null, born: "1990-05-17",
  firstPaid: true, reservedCents: 100, paidUntil: T0, lastClaim: T0, carriedCents: 0, carriedRipeAt: 0, moves: 2, ...over,
});
const view = (over = {}, cents = 3700) => ({ cents, account: account(over), rules });
const ms = (secs) => secs * 1000;
const BIRTHDAY = Date.UTC(2027, 4, 17) / 1000;

test("what may be sent leaves the welcome gift out", () => {
  assert.equal(M.spendableCents(view()), 3600);
  assert.equal(M.spendableCents(view({ reservedCents: 0 })), 3700);
  assert.equal(M.spendableCents({ cents: 100, account: account(), rules }), 0);
  assert.equal(M.spendableCents({ cents: 0, account: null, rules }), 0);
  assert.equal(M.spendableCents(null), 0);
  // Frozen or locked: nothing can be sent, and the screen says why (ADR 0028).
  assert.equal(M.statusNote(view()), "");
  for (const status of ["frozen", "holder-frozen", "disputed", "closed"]) {
    assert.equal(M.spendableCents(view({ status })), 0, status);
    assert.ok(M.statusNote(view({ status })).length > 0, status);
    assert.equal(M.claimableCents(view({ status }), ms(BIRTHDAY + 5)), 0, status);
  }
});

test("verified while the level holds and the account is active", () => {
  assert.ok(M.isVerified(view(), ms(T0 + DAY)));
  assert.ok(!M.isVerified(view(), ms(T0 + 300 * DAY)), "run out");
  assert.ok(!M.isVerified(view({ level: 0, levelUntil: null, person: false }), ms(T0)));
  assert.ok(!M.isVerified(view({ status: "frozen" }), ms(T0)));
  assert.ok(M.isVerified(view({ levelUntil: null }), ms(T0 + 9000 * DAY)), "a level without end");
  assert.ok(!M.isVerified({ cents: 0, account: null, rules }, ms(T0)));
  assert.ok(!M.isVerified(null, ms(T0)));
  assert.equal(M.daysLeft(view(), ms(T0)), 300);
  assert.equal(M.daysLeft(view(), ms(T0 + 400 * DAY)), 0);
  assert.equal(M.daysLeft(view({ level: 0, levelUntil: null }), ms(T0)), null);
  assert.ok(M.wasVerified(view({ level: 1 })) && !M.wasVerified(view({ person: false })));
});

test("the next birthday brings the age times the amount of the country", () => {
  const next = M.nextBirthday(view(), ms(T0));
  assert.deepEqual(next, { t: ms(BIRTHDAY), age: 37, cents: 7400 });
  assert.equal(M.nextBirthday(view({ country: "DE" }), ms(T0)).cents, 9250);
  assert.equal(M.nextBirthday(view({ country: "FR" }), ms(T0)).cents, 7400);
  assert.equal(M.nextBirthday(view({ born: null }), ms(T0)), null);
  // On the birthday itself the next one is a year away.
  assert.equal(M.nextBirthday(view(), ms(BIRTHDAY)).age, 38);
  // Born on 29 February: 1 March in a year without it.
  assert.equal(M.nextBirthday(view({ born: "1996-02-29" }), ms(T0)).t, Date.UTC(2027, 2, 1));
});

test("a birthday is due from its first second, for two weeks, once, if verified that day", () => {
  assert.equal(M.birthdaysDue(view(), ms(BIRTHDAY - 1)), 0);
  assert.equal(M.birthdaysDue(view(), ms(BIRTHDAY)), 7400);
  assert.equal(M.birthdaysDue(view(), ms(BIRTHDAY + 14 * DAY - 1)), 7400);
  assert.equal(M.birthdaysDue(view(), ms(BIRTHDAY + 14 * DAY + 1)), 0, "lapsed");
  assert.equal(M.birthdaysDue(view({ paidUntil: BIRTHDAY + 5 }), ms(BIRTHDAY + DAY)), 0, "claimed");
  assert.equal(M.birthdaysDue(view({ levelUntil: BIRTHDAY - DAY }), ms(BIRTHDAY + 5)), 0, "verification had run out");
  assert.equal(M.birthdaysDue(view({ born: null }), ms(BIRTHDAY + 5)), 0);
  assert.equal(M.birthdaysDue(view({ status: "frozen" }), ms(BIRTHDAY + 5)), 0);
  assert.equal(M.birthdaysDue({ cents: 0, account: null, rules }, ms(BIRTHDAY + 5)), 0);
  assert.equal(M.birthdaysDue({ cents: 0, account: account(), rules: [] }, ms(BIRTHDAY + 5)), 0);
  // A rule change counts from its day: the amount in force on the birthday.
  const later = [...rules, { ...rules[0], version: 2, validFrom: BIRTHDAY + DAY, age: { ...rules[0].age, perYearCents: 300 } }];
  assert.equal(M.birthdaysDue({ cents: 0, account: account(), rules: later }, ms(BIRTHDAY + 2 * DAY)), 7400);
  assert.equal(M.rulesAt(later, BIRTHDAY + 2 * DAY).version, 2);
  // Money that ripens is due only once ripe.
  const ripening = [{ ...rules[0], ripening: DAY }];
  assert.equal(M.birthdaysDue({ cents: 0, account: account(), rules: ripening }, ms(BIRTHDAY + 5)), 0);
  assert.equal(M.birthdaysDue({ cents: 0, account: account(), rules: ripening }, ms(BIRTHDAY + DAY)), 7400);
});

test("a claim is worth sending for birthdays due and for what the ledger set aside", () => {
  assert.equal(M.claimableCents(view(), ms(BIRTHDAY - 1)), 0);
  assert.equal(M.claimableCents(view(), ms(BIRTHDAY + 5)), 7400);
  // Verified again after the birthday, before claiming: the birthday waits, set aside.
  const renewed = { paidUntil: BIRTHDAY + DAY, carriedCents: 7400, carriedRipeAt: BIRTHDAY + DAY };
  assert.equal(M.birthdaysDue(view(renewed), ms(BIRTHDAY + 2 * DAY)), 0);
  assert.equal(M.claimableCents(view(renewed), ms(BIRTHDAY + 2 * DAY)), 7400);
  assert.equal(M.claimableCents(view({ ...renewed, carriedRipeAt: BIRTHDAY + 9 * DAY }), ms(BIRTHDAY + 2 * DAY)), 0, "not ripe yet");
  assert.equal(M.claimableCents(view({ ...renewed, status: "frozen" }), ms(BIRTHDAY + 2 * DAY)), 0);
  assert.equal(M.claimableCents(null, ms(BIRTHDAY)), 0);
});

test("the book notes what this device did and one line for what came from elsewhere", () => {
  let book = M.newBook("aa");
  // Not opened yet: nothing to write down.
  book = M.settle(book, { cents: 0, account: null, rules }, 1);
  assert.deepEqual([book.cents, book.history], [null, []]);
  // First sight: the welcome gift.
  book = M.settle(book, view({}, 100), 2);
  assert.deepEqual(book.history, [{ t: 2, kind: "gift", cents: 100 }]);
  // The verification brought the age.
  book = M.settle(book, view({}, 3700), 3, { kind: "age" });
  assert.deepEqual(book.history.at(-1), { t: 3, kind: "age", cents: 3600 });
  // Nothing changed: nothing written, the view kept.
  const same = M.settle(book, view({ moves: 9 }, 3700), 4);
  assert.equal(same.history, book.history);
  assert.equal(same.view.account.moves, 9);
  // A payment made here, with the name from the code.
  book = M.settle(same, view({}, 2450), 5, { kind: "out", who: "Anna" });
  assert.deepEqual(book.history.at(-1), { t: 5, kind: "out", cents: -1250, who: "Anna" });
  // Money that came, and money another device of the account paid.
  book = M.settle(book, view({}, 2650), 6);
  assert.deepEqual(book.history.at(-1), { t: 6, kind: "in", cents: 200 });
  book = M.settle(book, view({}, 2600), 7);
  assert.deepEqual(book.history.at(-1), { t: 7, kind: "out", cents: -50 });
  assert.equal(book.cents, 2600);
  // A device that joins an account later: the gift, and the rest in one line.
  const joined = M.settle(M.newBook("aa"), view({}, 2600), 8);
  assert.deepEqual(joined.history, [{ t: 8, kind: "gift", cents: 100 }, { t: 8, kind: "before", cents: 2500 }]);
  // … or verified before the first look: the rest is the age.
  const fresh = M.settle(M.newBook("aa"), view({}, 3700), 9, { kind: "age" });
  assert.deepEqual(fresh.history.at(-1), { t: 9, kind: "age", cents: 3600 });
  // The history does not grow without end.
  let long = book;
  for (let i = 0; i < 300; i++) long = M.settle(long, view({}, 2600 + (i % 2)), 10 + i);
  assert.equal(long.history.length, 200);
});

test("people paid before are remembered, latest first, each once", () => {
  let book = M.newBook("aa");
  for (const n of ["a", "b", "c", "d", "e", "f", "g"]) book = M.remember(book, n.toUpperCase(), n.repeat(64));
  book = M.remember(book, "Bea", "b".repeat(64));
  assert.deepEqual(book.people.map((p) => p.name), ["Bea", "G", "F", "E", "D", "C"]);
});

test("a payment code carries account, name and amount – and reads back", () => {
  const mine = "ab".repeat(32);
  const kept = { node: "http://node:9944", service: "http://service:8787/", account: mine, key: { kind: "ed25519" } };
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: () => JSON.stringify(kept), setItem() {}, removeItem() {} } });
  D.load();
  const code = D.payCode("Anna Müller", 1250);
  assert.deepEqual(D.readCode(code), { what: "pay", account: mine, name: "Anna Müller", cents: 1250, sameService: true });
  assert.equal(D.readCode(D.payCode("Ben")).cents, null);
  // Another network's code is told apart; an amount that is none is no amount.
  assert.equal(D.readCode(code.replace("service%3A8787", "other%3A1")).sameService, false);
  for (const c of ["0", "-5", "12.5", "1e3", "9999999999"]) assert.equal(D.readCode(`capita:pay?a=${mine}&c=${c}`).cents, null, c);
  assert.equal(D.readCode(`capita:pay?a=${mine.slice(2)}`), null, "not an account");
  assert.equal(D.readCode(`capita:pay?a=${mine.toUpperCase()}`), null);
  assert.equal(D.readCode("capita:demo/Anna?amount=5").what, "demo");
  assert.equal(D.readCode("https://example.org/?a=1"), null);
});
