// SPDX-License-Identifier: AGPL-3.0-only
// The calls the screens send, in SCALE, and the chain state they read.
//
// Calls: `capita.open`, `capita.claim`, `capita.transfer` (ADR 0027),
// `credential.present` / `credential.requestMove` from the identity
// service's answer, and the device calls of `pallet-capita-keys` (ADR 0020). The runtime decodes the same bytes in
// its tests (`runtime/src/tests.rs`, vectors `app/tests/call-vectors.json`
// made by `app/tests/make-call-vectors.mjs`).
//
// State: storage addresses (twox128 of pallet and item, then BLAKE2b-128 ‖
// key) and the few values the screens need – the keys of an account, its
// nonce, a pending home claim, the claim it holds, its balance, what the
// ledger remembers of it and the rules in force. The runtime writes those
// values for fixed inputs (`app/tests/state-vectors.json`); the app's tests
// read the same bytes. Nothing here is shown to the person.

import { blake2b } from "./blake2b.js";
import { twox128 } from "./xxhash.js";
import { KEYS, concat, compact, bytesVec, fromHex, hex, keyBytes } from "./keys.js";

/** Pallet indexes of the runtime (`runtime/src/lib.rs`) and the calls used. */
export const PALLETS = Object.freeze({ capita: 10, credential: 22, keys: KEYS.pallet });
export const CAPITA = Object.freeze({ open: 0, claim: 1, transfer: 2 });
export const CREDENTIAL = Object.freeze({ present: 0, requestMove: 4 });

const u32le = (n) => Uint8Array.of(n & 255, (n >>> 8) & 255, (n >>> 16) & 255, (n >>> 24) & 255);
function u64le(n) {
  let v = BigInt(n);
  const out = new Uint8Array(8);
  for (let k = 0; k < 8; k++) { out[k] = Number(v & 255n); v >>= 8n; }
  return out;
}
function exact(text, n, what) {
  const b = fromHex(text);
  if (b.length !== n) throw new TypeError(`${what}: ${n} bytes`);
  return b;
}
const option = (value, encode) => (value === null || value === undefined ? Uint8Array.of(0) : concat(Uint8Array.of(1), encode(value)));
/** A day "YYYY-MM-DD" as the chain's `Date`: year (u16), month, day. */
function dateBytes(text) {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(text));
  if (!m) throw new TypeError("date: YYYY-MM-DD");
  const [year, month, day] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const real = new Date(Date.UTC(year, month - 1, day));
  if (year < 1 || real.getUTCMonth() !== month - 1 || real.getUTCDate() !== day) throw new TypeError("date: not a day of the calendar");
  return Uint8Array.of(year & 255, year >> 8, month, day);
}

// ---------------------------------------------------------------- calls

/** `capita.open()`: the account joins the ledger (start grant). */
export const openAccount = () => Uint8Array.of(PALLETS.capita, CAPITA.open);

/** `capita.claim()`: the Capita that fell due – the birthdays – reach the account. */
export const claim = () => Uint8Array.of(PALLETS.capita, CAPITA.claim);

/** `capita.transfer(to, amount)`: `cents` to the account `to` (its 32 bytes). */
export function transfer(to, cents) {
  if (!(to instanceof Uint8Array) || to.length !== 32) throw new TypeError("account: 32 bytes");
  if (!Number.isSafeInteger(cents) || cents <= 0) throw new TypeError("amount: whole cents above zero");
  return concat(Uint8Array.of(PALLETS.capita, CAPITA.transfer), to, u64le(cents));
}

/**
 * `Claim` in SCALE, from the JSON the identity service answers
 * (`capita_issuer::http::call_json`). The service sends no residence and no
 * attributes; a claim with either is refused here rather than guessed. The
 * date of birth (`born`, "YYYY-MM-DD" or null) is part of what it signed.
 */
export function claimBytes(c) {
  if (c.residence !== null && c.residence !== undefined) throw new TypeError("claim with residence");
  if (c.attributes?.length) throw new TypeError("claim with attributes");
  return concat(
    u32le(c.issuer),
    exact(c.serial, 16, "serial"),
    exact(c.subject, 32, "subject"),
    exact(c.subjectRef, 32, "subjectRef"),
    option(c.replaces, (r) => exact(r, 32, "replaces")),
    Uint8Array.of(c.level),
    Uint8Array.of(0), // residence: none
    option(c.born, dateBytes), // date of birth: the ledger issues Capita by age
    compact(0), // attributes: none
    u64le(c.issuedAt),
    option(c.expiresAt, u64le),
  );
}

/**
 * The call an answer of the identity service asks for: `credential.present`
 * or, for a person whose account moves to this key, `credential.requestMove`.
 * No name: an identity's name is public for ever; the chain derives one.
 */
export function claimCall(answer) {
  const index = { "credential.present": CREDENTIAL.present, "credential.requestMove": CREDENTIAL.requestMove }[answer.call];
  if (index === undefined) throw new TypeError(`unknown call ${answer.call}`);
  return concat(
    Uint8Array.of(PALLETS.credential, index),
    claimBytes(answer.claim),
    Uint8Array.of(0), exact(answer.signature, 64, "signature"), // Signature::Ed25519
    Uint8Array.of(0), // name: none
  );
}

/** `keys.add_key(key, possession)` from the bytes a new device handed over. */
export const addKeyRaw = (keyRaw, possessionRaw) => concat(Uint8Array.of(PALLETS.keys, KEYS.addKey), keyRaw, possessionRaw);
/** `keys.remove_key(key)` from a key's bytes. */
export const removeKeyRaw = (keyRaw) => concat(Uint8Array.of(PALLETS.keys, KEYS.removeKey), keyRaw);
/** `keys.claim_home()`, `keys.take_home()`, `keys.refuse_home()`. */
export const claimHome = () => Uint8Array.of(PALLETS.keys, KEYS.claimHome);
export const takeHome = () => Uint8Array.of(PALLETS.keys, KEYS.takeHome);
export const refuseHome = () => Uint8Array.of(PALLETS.keys, KEYS.refuseHome);

// ---------------------------------------------------------------- storage addresses

const enc = new TextEncoder();
const prefix = (pallet, item) => concat(twox128(enc.encode(pallet)), twox128(enc.encode(item)));

/** Address of a `Blake2_128Concat` map entry. */
export const mapKey = (pallet, item, key) => concat(prefix(pallet, item), blake2b(key, 16), key);

export const at = Object.freeze({
  keys: (account) => mapKey("Keys", "Keys", account),
  nonce: (account) => mapKey("Keys", "Nonce", account),
  homeClaim: (account) => mapKey("Keys", "HomeClaim", account),
  keyOwner: (keyRaw) => mapKey("Keys", "KeyOwner", keyRaw),
  credential: (account) => mapKey("Credential", "Credentials", account),
  capitaAccount: (account) => mapKey("Capita", "Accounts", account),
  balance: (account) => mapKey("Capita", "Balances", account),
  movedTo: (account) => mapKey("Capita", "MovedTo", account),
  rules: () => prefix("Capita", "ParamsHistory"),
});

// ---------------------------------------------------------------- reading values

class Reader {
  constructor(bytes) { this.b = bytes; this.i = 0; }
  take(n) {
    if (this.i + n > this.b.length) throw new RangeError("short value");
    const out = this.b.slice(this.i, this.i + n);
    this.i += n;
    return out;
  }
  u8() { return this.take(1)[0]; }
  u16() { const b = this.take(2); return b[0] | (b[1] << 8); }
  u32() { const b = this.take(4); return (b[0] | (b[1] << 8) | (b[2] << 16)) + b[3] * 2 ** 24; }
  u64() { const b = this.take(8); let v = 0n; for (let k = 7; k >= 0; k--) v = (v << 8n) | BigInt(b[k]); return Number(v); }
  compact() {
    const first = this.b[this.i];
    const mode = first & 3;
    if (mode === 0) { this.i += 1; return first >> 2; }
    if (mode === 1) { const b = this.take(2); return (b[0] | (b[1] << 8)) >> 2; }
    if (mode === 2) return this.u32() >>> 2;
    throw new RangeError("compact too large");
  }
  option(read) { return this.u8() === 1 ? read() : null; }
  /** The chain's `Date` as "YYYY-MM-DD". */
  date() {
    const two = (n) => String(n).padStart(2, "0");
    return `${String(this.u16()).padStart(4, "0")}-${two(this.u8())}-${two(this.u8())}`;
  }
  country() { const c = this.take(2); return String.fromCharCode(c[0], c[1]); }
  /** Every byte must have been read: a value laid out differently is refused, not guessed. */
  end(what) { if (this.i !== this.b.length) throw new RangeError(`${what}: not the bytes this app knows`); }
}

const KEY_LENGTH = [32, 33, 33];
const KIND = ["ed25519", "passkey", "hardware"];

/** One `DeviceKey`: { kind, raw } – raw = its SCALE bytes (the chain's name for it). */
function deviceKey(r) {
  const tag = r.u8();
  if (tag > 2) throw new RangeError("unknown key kind");
  return { kind: KIND[tag], raw: concat(Uint8Array.of(tag), r.take(KEY_LENGTH[tag])) };
}

/** `Keys::Keys`: the account's keys, home key first. */
export function decodeKeys(bytes) {
  if (!bytes) return [];
  const r = new Reader(bytes);
  return Array.from({ length: r.compact() }, () => deviceKey(r));
}

/** `Keys::Nonce` (missing = 0). */
export const decodeNonce = (bytes) => (bytes ? new Reader(bytes).u32() : 0);

/** `Keys::HomeClaim`: { key, since } (seconds), or null. */
export function decodeHomeClaim(bytes) {
  if (!bytes) return null;
  const r = new Reader(bytes);
  return { key: deviceKey(r), since: r.u64() };
}

/** `Credential::Credentials`: { level, born, issuedAt, expiresAt } of the claim held, or null. */
export function decodeCredential(bytes) {
  if (!bytes) return null;
  const r = new Reader(bytes);
  r.take(4 + 16 + 32 + 32); // issuer, serial, subject, person key
  r.option(() => r.take(32)); // replaces
  const level = r.u8();
  r.option(() => r.take(2 + 8)); // residence: country, proven at
  const born = r.option(() => r.date());
  for (let n = r.compact(); n > 0; n--) r.take(2 + 8); // attributes
  const issuedAt = r.u64();
  const expiresAt = r.option(() => r.u64());
  return { level, born, issuedAt, expiresAt };
}

// ---------------------------------------------------------------- money (ADR 0027)

/** `Capita::Balances`: cents (missing = 0). */
export function decodeBalance(bytes) {
  if (!bytes) return 0;
  const r = new Reader(bytes);
  const b = r.take(8);
  r.end("balance");
  let v = 0n;
  for (let k = 7; k >= 0; k--) v = (v << 8n) | BigInt(b[k]);
  return Number(BigInt.asIntN(64, v));
}

const STATUS = ["active", "frozen", "closed"];

/**
 * `Capita::Accounts`: what the ledger remembers of an account, as far as
 * the screens need it – or null if the account is not open. Times in
 * seconds, amounts in cents.
 */
export function decodeAccount(bytes) {
  if (!bytes) return null;
  const r = new Reader(bytes);
  r.take(32); // the account itself
  const person = r.option(() => r.take(32)) !== null;
  const status = STATUS[r.u8()];
  if (!status) throw new RangeError("account: unknown status");
  const openedAt = r.u64();
  r.take(2 + 8); // step of the ladder, and since when
  const level = r.u8();
  const levelUntil = r.option(() => r.u64());
  const country = r.option(() => { const c = r.country(); r.take(8); return c; });
  const born = r.option(() => r.date());
  const firstPaid = r.u8() === 1;
  r.take(16 + 8); // seniority ramp of the cap
  const reservedCents = r.u64();
  r.take(8 + 16 + 8); // issuance by rate: anchor, rate, paid since the anchor
  const paidUntil = r.u64();
  const lastClaim = r.u64();
  const carriedCents = r.u64(); // set aside at a change of the verification, minted by the next claim …
  const carriedRipeAt = r.u64(); // … from this moment on
  r.take(8 + 8); // credit line, debt
  r.option(() => r.take(32)); // lender
  r.take(8 + 8 + 8); // demurrage settled at, its remainder; latest time seen
  const moves = r.u64(); // ledger operations of this account so far
  r.take(8); // revision
  r.end("account");
  return { person, status, openedAt, level, levelUntil, country, born, firstPaid, reservedCents, paidUntil, lastClaim, carriedCents, carriedRipeAt, moves };
}

/** One version of the rules: the values the money screens show or count with. */
function rulesVersion(r) {
  const version = r.u32();
  const validFrom = r.u64();
  const startGrantCents = r.u64();
  for (let n = r.compact(); n > 0; n--) r.take(2 + 1 + 16); // the ladder
  const age = { minLevel: r.u8(), perYearCents: r.u64(), atVerificationCents: r.u64(), byCountry: [] };
  for (let n = r.compact(); n > 0; n--) age.byCountry.push({ country: r.country(), perYearCents: r.u64() });
  const claimWindow = r.u64();
  r.take(8 + 8); // caps: period, global cap
  for (let n = r.compact(); n > 0; n--) r.take(2 + 8 + 4); // caps per country
  r.take(8); // seniority ramp
  r.take(8 + 8 + 4); // lending
  r.take(4 + 8 + 8); // demurrage
  const ripening = r.u64();
  for (let n = r.compact(); n > 0; n--) r.take(2); // sponsor keys
  r.take(1); // balance of a closed account
  return { version, validFrom, startGrantCents, age, claimWindow, ripening };
}

/** `Capita::ParamsHistory`: every version of the rules, oldest first. */
export function decodeRules(bytes) {
  if (!bytes) return [];
  const r = new Reader(bytes);
  const list = Array.from({ length: r.compact() }, () => rulesVersion(r));
  r.end("rules");
  return list;
}

/** The kind of device a key stands for, in the words the screens use. */
export const deviceOf = (keyRaw) => (keyRaw[0] === 2 ? "phone" : "browser");

/** The SCALE bytes of the app's own key object (chain name of the key). */
export const rawOf = (key) => keyBytes(key);

export { hex, fromHex, bytesVec };
