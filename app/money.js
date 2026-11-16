// SPDX-License-Identifier: AGPL-3.0-only
// The money of an account on the ledger, as the screens need it (ADR 0027).
// Pure functions over what app/chain/calls.js reads: no network, no clock,
// no storage, no screen. Amounts in integer cents; `now` in milliseconds as
// everywhere in the app, the ledger's times in seconds.
//
// The ledger decides every amount. What is counted here only says when a
// claim is worth sending and what the next birthday will bring – with the
// rules read from the chain, never with numbers of the app's own.

import { ageAt, birthdayAt } from "./logic.js";

// ---------------------------------------------------------------- the rules in force

/** The version of the rules in force at `secs`: the latest that has begun. */
export function rulesAt(rules, secs) {
  let hit = rules[0] ?? null;
  for (const version of rules) if (version.validFrom <= secs) hit = version;
  return hit;
}

const perYearIn = (age, country) => age.byCountry.find((c) => c.country === country)?.perYearCents ?? age.perYearCents;

/** The verification level that counts at `secs`: none once it has run out. */
export const levelAt = (account, secs) => (account.levelUntil !== null && secs >= account.levelUntil ? 0 : account.level);

// ---------------------------------------------------------------- what a view says
// A view is what the device read: { cents, account, rules } – `account` is
// null while the ledger does not know the account.

export function isVerified(view, now) {
  return !!view?.account && view.account.status === "active" && levelAt(view.account, Math.floor(now / 1000)) >= 1;
}

/** Whether the account was verified before and the year is over. */
export const wasVerified = (view) => !!view?.account?.person;

/** What may be sent: the balance without the earmarked start grant. */
export const spendableCents = (view) => (view ? Math.max(0, view.cents - (view.account?.reservedCents ?? 0)) : 0);

/** Days until the verification must be renewed; null if not verified or without end. */
export function daysLeft(view, now) {
  const until = view?.account?.levelUntil ?? null;
  if (!view?.account?.level || until === null) return null;
  return Math.max(0, Math.ceil((until * 1000 - now) / 86400000));
}

/**
 * What a claim sent at `now` would bring for birthdays, in cents: every
 * birthday since the ledger last counted, at most the claim window back,
 * on which the verification held (`capita_core::issuance::birthdays_due`).
 */
export function birthdaysDue(view, now) {
  const a = view?.account;
  if (!a || a.status !== "active" || !a.born || !view.rules.length) return 0;
  const secs = Math.floor(now / 1000);
  const current = rulesAt(view.rules, secs);
  const upto = secs - current.ripening;
  let from = a.paidUntil;
  if (upto <= from) return 0;
  if (upto - from > current.claimWindow) from = upto - current.claimWindow;
  let cents = 0;
  for (let age = ageAt(a.born, from * 1000) + 1; ; age++) {
    const day = birthdayAt(a.born, age) / 1000;
    if (day > upto) break;
    if (day <= from) continue; // counted before
    const rules = rulesAt(view.rules, day).age;
    if (levelAt(a, day) < rules.minLevel) continue; // not verified on that day
    cents += age * perYearIn(rules, a.country);
  }
  return cents;
}

/**
 * What a claim sent at `now` would bring by age, in cents: the birthdays
 * due, and what the ledger set aside when the verification changed (a
 * birthday not yet claimed at a renewal waits there).
 */
export function claimableCents(view, now) {
  const a = view?.account;
  if (!a || a.status !== "active") return 0;
  const ripe = a.carriedCents > 0 && Math.floor(now / 1000) >= a.carriedRipeAt ? a.carriedCents : 0;
  return ripe + birthdaysDue(view, now);
}

/** The next birthday after `now`: { t (ms), age, cents }, or null without a date of birth. */
export function nextBirthday(view, now) {
  const a = view?.account;
  if (!a?.born || !view.rules.length) return null;
  const age = ageAt(a.born, now) + 1;
  const t = birthdayAt(a.born, age);
  return { t, age, cents: age * perYearIn(rulesAt(view.rules, t / 1000).age, a.country) };
}

// ---------------------------------------------------------------- the book of this device
// The ledger keeps balances, not a history a phone could read cheaply. The
// device writes down what it did itself, with the name it knew; whatever
// else changed the balance is one line without a name.

const MAX_HISTORY = 200;
const MAX_PEOPLE = 6;

export const newBook = (account) => ({ v: 1, account, cents: null, history: [], people: [], view: null });

/**
 * Brings the book up to what the ledger says. `as` names the change if
 * this device caused it ({ kind, who? }); without it the change came from
 * elsewhere: a payment received, or one made on another device.
 */
export function settle(book, view, now, as = null) {
  if (!view?.account) return { ...book, view };
  if (book.cents === null) {
    // First sight of the account: the welcome gift, and what was there before.
    const gift = Math.min(view.account.reservedCents, Math.max(0, view.cents));
    const rest = view.cents - gift;
    const history = [];
    if (gift > 0) history.push({ t: now, kind: "gift", cents: gift });
    if (rest !== 0) history.push({ t: now, kind: "before", cents: rest, ...(as ?? {}) });
    return { ...book, cents: view.cents, history, view };
  }
  const delta = view.cents - book.cents;
  if (delta === 0) return { ...book, view };
  const entry = { t: now, kind: delta > 0 ? "in" : "out", cents: delta, ...(as ?? {}) };
  return { ...book, cents: view.cents, history: [...book.history, entry].slice(-MAX_HISTORY), view };
}

/** Remembers whom this device paid, latest first: { name, account }. */
export function remember(book, name, account) {
  const people = [{ name, account }, ...book.people.filter((p) => p.account !== account)].slice(0, MAX_PEOPLE);
  return { ...book, people };
}
