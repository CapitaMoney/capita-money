// SPDX-License-Identifier: AGPL-3.0-only
// The prototype's money rules, mirrored from rules/alpha.toml.
// Pure functions: state in, new state out. No storage, no clock, no screen.
// Integer cents everywhere (ADR 0003). No function lets a balance go below
// zero (rules.md rule 10: no debt).

export const RULES = Object.freeze({
  startGrantCents: 100, // issuance.start_grant = "1.00" (decided)
  verificationPriceCents: 100, // verification.price_eur = "1.00" (decided); paid by the start grant
  verifiedMonthlyCents: 1000, // ladder step 1 = "10.00" per month (example, 2discuss A)
  claimWindowMs: 14 * 86400000, // issuance.claim_window = "14 days" (decided)
  monthMs: Math.round((365.25 / 12) * 86400000), // month = 1/12 of 365.25 days (core)
  membershipMs: 365 * 86400000, // membership 365 days: verify again once a year
});

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
 * Pays the verification with the start grant (demo: the check itself is a placeholder).
 * Returns the new state; throws if nothing is there to pay with.
 */
export function verify(s, now) {
  if (isVerified(s, now)) return s;
  let reserved = s.reservedCents;
  let balance = s.balanceCents;
  const price = RULES.verificationPriceCents;
  // The start grant pays first; a renewal a year later is paid from the balance.
  const fromGrant = Math.min(reserved, price);
  const rest = price - fromGrant;
  if (rest > spendableCents(s)) throw new Error("funds");
  reserved -= fromGrant;
  balance -= price;
  return {
    ...s,
    balanceCents: balance,
    reservedCents: reserved,
    verifiedAt: now,
    lastClaimAt: now,
    history: [...s.history, { t: now, kind: "verify", cents: -price }],
  };
}

/**
 * Collects what accrued since the last collection, at most the claim window back.
 * Returns { state, collected, lapsed } in cents. Called whenever the app comes
 * to the screen – never by a timer (battery).
 */
export function collect(s, now) {
  if (s.verifiedAt === null || s.lastClaimAt === null) return { state: s, collected: 0, lapsed: 0 };
  const end = Math.min(now, s.verifiedAt + RULES.membershipMs);
  if (end <= s.lastClaimAt) return { state: s, collected: 0, lapsed: 0 };
  const rate = RULES.verifiedMonthlyCents;
  const from = Math.max(s.lastClaimAt, end - RULES.claimWindowMs);
  const lapsed = Math.floor(((from - s.lastClaimAt) * rate) / RULES.monthMs);
  const cents = Math.floor(((end - from) * rate) / RULES.monthMs);
  if (cents <= 0) return { state: { ...s, lastClaimAt: from }, collected: 0, lapsed };
  // Keep the remainder: move the clock only as far as the whole cents reach.
  const used = Math.floor((cents * RULES.monthMs) / rate);
  const history = mergeMonthly(s.history, now, cents);
  return {
    state: { ...s, balanceCents: s.balanceCents + cents, lastClaimAt: from + used, history },
    collected: cents,
    lapsed,
  };
}

function mergeMonthly(history, now, cents) {
  const last = history[history.length - 1];
  if (last && last.kind === "monthly" && sameDay(last.t, now)) {
    return [...history.slice(0, -1), { ...last, t: now, cents: last.cents + cents }];
  }
  return [...history, { t: now, kind: "monthly", cents }];
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
    case "monthly": return "Your Capita";
    case "in": return `From ${item.who}`;
    case "out": return `To ${item.who}`;
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
