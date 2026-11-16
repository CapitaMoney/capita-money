// SPDX-License-Identifier: AGPL-3.0-only
// The prototype's money rules, mirrored from rules/alpha.toml: Capita come
// by age – the age at the first verification, twice the age on each birthday.
// Pure functions: state in, new state out. No storage, no clock, no screen.
// Integer cents everywhere (ADR 0003). No function lets a balance go below
// zero (rules.md rule 10: no debt).

export const RULES = Object.freeze({
  startGrantCents: 100, // issuance.start_grant = "1.00" (decided)
  verificationPriceCents: 100, // verification.price_eur = "1.00" (decided); paid by the start grant
  ageFirstCents: 100, // issuance.age.at_verification = "1.00" per year of age, once, at the first verification (decided)
  agePerYearCents: 200, // issuance.age.per_year = "2.00" per year of age on each birthday (decided)
  claimWindowMs: 14 * 86400000, // issuance.claim_window = "14 days" (decided)
  membershipMs: 365 * 86400000, // membership 365 days: verify again once a year
});

// ---------------------------------------------------------------- age (ADR 0026)
// A date of birth is "YYYY-MM-DD"; a birthday begins at 00:00 UTC, as on
// the ledger. Born on 29 February, the birthday of other years is 1 March.

/** "1990-05-17" → [1990, 5, 17], or null if it is no day of the calendar. */
function dateParts(text) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(text ?? ""));
  if (!m) return null;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const real = new Date(Date.UTC(y, mo - 1, d));
  return y >= 1 && real.getUTCMonth() === mo - 1 && real.getUTCDate() === d ? [y, mo, d] : null;
}

export const isDate = (text) => dateParts(text) !== null;

/** Completed years of age at the moment `t`. */
export function ageAt(born, t) {
  const [y, mo, d] = dateParts(born);
  const day = new Date(t);
  const before = day.getUTCMonth() + 1 < mo || (day.getUTCMonth() + 1 === mo && day.getUTCDate() < d);
  return Math.max(0, day.getUTCFullYear() - y - (before ? 1 : 0));
}

/** The moment the birthday begins on which `age` years are completed. */
export function birthdayAt(born, age) {
  const [y, mo, d] = dateParts(born);
  // Date.UTC turns 29 February of a year without it into 1 March.
  return Date.UTC(y + age, mo - 1, d);
}

/** The next birthday after `now`: { t, age, cents }, or null without a date of birth. */
export function nextBirthday(s, now) {
  if (!s.born) return null;
  const age = ageAt(s.born, now) + 1;
  return { t: birthdayAt(s.born, age), age, cents: age * RULES.agePerYearCents };
}

/** The demo is not connected: its person turns 30 on the day of the tap. */
export function demoBorn(now) {
  const day = new Date(now);
  const two = (n) => String(n).padStart(2, "0");
  const born = `${day.getUTCFullYear() - 30}-${two(day.getUTCMonth() + 1)}-${two(day.getUTCDate())}`;
  // Tapped on 29 February: the year 30 years back has none.
  return isDate(born) ? born : `${day.getUTCFullYear() - 30}-03-01`;
}

export const MAX_NAME = 40;

/** A fresh account: the start grant is there at once, earmarked for the own verification. */
export function newAccount(name, now) {
  const clean = cleanName(name);
  if (!clean) throw new Error("name");
  return {
    v: 1,
    name: clean,
    createdAt: now,
    balanceCents: RULES.startGrantCents,
    reservedCents: RULES.startGrantCents,
    verifiedAt: null,
    lastClaimAt: null,
    born: null,
    firstPaid: false,
    history: [{ t: now, kind: "gift", cents: RULES.startGrantCents }],
  };
}

export function cleanName(name) {
  return String(name ?? "").replace(/\s+/g, " ").trim().slice(0, MAX_NAME);
}

export function isVerified(s, now) {
  return s.verifiedAt !== null && now - s.verifiedAt < RULES.membershipMs;
}

/** What may be sent: the balance without the earmarked start grant. */
export function spendableCents(s) {
  return Math.max(0, s.balanceCents - s.reservedCents);
}

/**
 * Pays the verification with the start grant (demo: the check itself is a placeholder)
 * and records it. `born` is the date of birth the check read, if any.
 * Returns the new state; throws if nothing is there to pay with.
 */
export function verify(s, now, born = null) {
  if (isVerified(s, now)) {
    // Verified already: nothing to pay; a date of birth learned only now counts.
    return born && !s.born ? verified(s, now, born) : s;
  }
  let reserved = s.reservedCents;
  let balance = s.balanceCents;
  const price = RULES.verificationPriceCents;
  // The start grant pays first; a renewal a year later is paid from the balance.
  const fromGrant = Math.min(reserved, price);
  const rest = price - fromGrant;
  if (rest > spendableCents(s)) throw new Error("funds");
  reserved -= fromGrant;
  balance -= price;
  const paid = {
    ...s,
    balanceCents: balance,
    reservedCents: reserved,
    history: [...s.history, { t: now, kind: "verify", cents: -price }],
  };
  return verified(paid, now, born);
}

/**
 * Records a verification that is already paid. The first one with a date of
 * birth brings the age in Capita, at once and once per person; birthdays
 * count from `now`.
 */
export function verified(s, now, born = null) {
  // What fell due under the old verification first: nothing reaches back.
  const settled = collect(s, now).state;
  const next = { ...settled, verifiedAt: now, lastClaimAt: now, born: isDate(born) ? born : settled.born ?? null };
  if (!next.born || settled.firstPaid) return next;
  const cents = ageAt(next.born, now) * RULES.ageFirstCents;
  return {
    ...next,
    firstPaid: true,
    balanceCents: next.balanceCents + cents,
    history: cents > 0 ? [...next.history, { t: now, kind: "age", cents }] : next.history,
  };
}

/**
 * Collects the birthdays since the last collection: each brings the age
 * times the age factor, all at once – if the verification held on that day
 * and the birthday is at most the claim window back. Returns
 * { state, collected, lapsed } in cents. Called whenever the app comes to
 * the screen – never by a timer (battery).
 */
export function collect(s, now) {
  if (s.verifiedAt === null || s.lastClaimAt === null || !s.born) return { state: s, collected: 0, lapsed: 0 };
  if (now <= s.lastClaimAt) return { state: s, collected: 0, lapsed: 0 };
  const from = Math.max(s.lastClaimAt, now - RULES.claimWindowMs);
  const until = s.verifiedAt + RULES.membershipMs;
  let collected = 0;
  let lapsed = 0;
  const history = [...s.history];
  for (let age = ageAt(s.born, s.lastClaimAt) + 1; ; age++) {
    const t = birthdayAt(s.born, age);
    if (t > now) break;
    if (t <= s.lastClaimAt || t >= until) continue; // counted before, or not verified on that day
    const cents = age * RULES.agePerYearCents;
    if (t > from) {
      collected += cents;
      history.push({ t: now, kind: "birthday", cents });
    } else {
      lapsed += cents;
    }
  }
  return { state: { ...s, balanceCents: s.balanceCents + collected, lastClaimAt: now, history }, collected, lapsed };
}

export function send(s, toName, cents, now) {
  if (!Number.isInteger(cents) || cents <= 0) throw new Error("amount");
  if (cents > spendableCents(s)) throw new Error("funds");
  return {
    ...s,
    balanceCents: s.balanceCents - cents,
    history: [...s.history, { t: now, kind: "out", cents: -cents, who: cleanName(toName) }],
  };
}

export function receive(s, fromName, cents, now) {
  if (!Number.isInteger(cents) || cents <= 0) throw new Error("amount");
  return {
    ...s,
    balanceCents: s.balanceCents + cents,
    history: [...s.history, { t: now, kind: "in", cents, who: cleanName(fromName) }],
  };
}

/** "12.50" → 1250; accepts "12", "12.5", "12,50". Returns null if not an amount. */
export function parseAmount(text) {
  const m = /^\s*(\d{1,7})(?:[.,](\d{0,2}))?\s*$/.exec(String(text));
  if (!m) return null;
  return Number(m[1]) * 100 + Number((m[2] || "").padEnd(2, "0"));
}

/** 1250 → "12.50" (no currency word: the screen adds "Capita"). */
export function formatCents(cents) {
  const sign = cents < 0 ? "−" : "";
  const a = Math.abs(cents);
  const whole = String(Math.floor(a / 100)).replace(/\B(?=(\d{3})+(?!\d))/g, " ");
  return `${sign}${whole}.${String(a % 100).padStart(2, "0")}`;
}

/** Plain words for one line of history. Never ids, keys or codes. */
export function describe(item) {
  switch (item.kind) {
    case "gift": return "Welcome gift";
    case "verify": return "Verification";
    case "age": return "Your age in Capita";
    case "birthday": return "Birthday";
    case "monthly": return "Your Capita"; // from before Capita came by age
    // On the ledger a line carries a name only if this device knew it (money.js).
    case "in": return item.who ? `From ${item.who}` : "Received";
    case "out": return item.who ? `To ${item.who}` : "Paid from another of your devices";
    case "before": return "Your Capita so far";
    default: return "";
  }
}

export function sameDay(a, b) {
  const x = new Date(a), y = new Date(b);
  return x.getFullYear() === y.getFullYear() && x.getMonth() === y.getMonth() && x.getDate() === y.getDate();
}

/** Days until the verification must be renewed, or null if not verified. */
export function daysLeft(s, now) {
  if (s.verifiedAt === null) return null;
  return Math.max(0, Math.ceil((s.verifiedAt + RULES.membershipMs - now) / 86400000));
}
