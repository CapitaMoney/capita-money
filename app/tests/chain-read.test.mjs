// SPDX-License-Identifier: AGPL-3.0-only
// The app's storage addresses and values (app/chain/xxhash.js, calls.js).
// The runtime checks the same vectors (runtime/src/tests.rs).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { xxhash64, twox128 } from "../chain/xxhash.js";
import * as C from "../chain/calls.js";
import { hex, fromHex, concat, compact } from "../chain/keys.js";
import * as M from "../money.js";
import { vectors } from "./make-call-vectors.mjs";

const enc = new TextEncoder();

test("xxHash64 gives the published values", () => {
  assert.equal(xxhash64(new Uint8Array(0)).toString(16), "ef46db3751d8e999");
  assert.equal(xxhash64(enc.encode("abc")).toString(16), "44bc2cf5ad770999");
  assert.equal(xxhash64(enc.encode("Nobody inspects the spammish repetition")).toString(16), "fbcea83c8a378bf1");
});

test("twox128 gives Substrate's well-known addresses", () => {
  assert.equal(hex(twox128(enc.encode("System"))), "26aa394eea5630e07c48ae0c9558cef7");
  assert.equal(hex(twox128(enc.encode("Account"))), "b99d880ec681799c0cf30e8886371da9");
});

test("the vectors file is what the app makes", () => {
  const file = JSON.parse(readFileSync(new URL("./call-vectors.json", import.meta.url), "utf8"));
  assert.deepEqual(file, JSON.parse(JSON.stringify(vectors())));
});

test("a claim held on chain reads back", () => {
  const v = vectors();
  const bytes = C.claimBytes(v.calls.present.answer.claim);
  assert.deepEqual(C.decodeCredential(bytes), { level: 1, born: "1990-05-17", issuedAt: 1800000000, expiresAt: 1831536000 });
  const moved = C.claimBytes(v.calls.requestMove.answer.claim);
  assert.deepEqual(C.decodeCredential(moved), { level: 1, born: null, issuedAt: 1800000000, expiresAt: null });
  // A date of birth is a day of the calendar, written YYYY-MM-DD.
  for (const born of ["1990-02-30", "17.05.1990", "1990-5-17", "0000-01-01"]) {
    assert.throws(() => C.claimBytes({ ...v.calls.present.answer.claim, born }), TypeError, born);
  }
  const leap = C.claimBytes({ ...v.calls.present.answer.claim, born: "1996-02-29" });
  assert.equal(C.decodeCredential(leap).born, "1996-02-29");
  assert.throws(() => C.claimBytes({ ...v.calls.present.answer.claim, residence: { country: "DE" } }));
  assert.throws(() => C.claimCall({ ...v.calls.present.answer, call: "credential.revoke" }));
});

test("keys, nonce and home claim read back", () => {
  const ed = Uint8Array.of(0, ...new Uint8Array(32).fill(1));
  const hw = Uint8Array.of(2, 3, ...new Uint8Array(32).fill(2));
  const keys = C.decodeKeys(concat(compact(2), hw, ed));
  assert.deepEqual(keys.map((k) => [k.kind, hex(k.raw)]), [["hardware", hex(hw)], ["ed25519", hex(ed)]]);
  assert.deepEqual(C.decodeKeys(null), []);
  assert.equal(C.decodeNonce(Uint8Array.of(5, 1, 0, 0)), 261);
  assert.equal(C.decodeNonce(null), 0);
  const since = Uint8Array.of(0x00, 0x2f, 0x48, 0x6b, 0, 0, 0, 0); // 1800941312
  const claim = C.decodeHomeClaim(concat(ed, since));
  assert.equal(hex(claim.key.raw), hex(ed));
  assert.equal(claim.since, 0x6b482f00);
  assert.equal(C.deviceOf(hw), "phone");
  assert.equal(C.deviceOf(ed), "browser");
});

test("a payment and a claim are the calls the runtime decodes", () => {
  const to = Uint8Array.from({ length: 32 }, (_, i) => i);
  assert.equal(hex(C.claim()), "0a01");
  // capita.transfer: pallet 10, call 2, the account, the cents as 8 bytes little-endian
  assert.equal(hex(C.transfer(to, 1250)), "0a02" + hex(to) + "e204000000000000");
  for (const cents of [0, -5, 12.5, NaN, 2 ** 53]) assert.throws(() => C.transfer(to, cents), TypeError, String(cents));
  assert.throws(() => C.transfer(to.slice(1), 100), TypeError);
});

// The runtime wrote these storage values on a chain under the alpha rules
// and says what they mean (runtime/src/tests.rs writes the file).
test("balance, account and rules read as the runtime wrote them", () => {
  const v = JSON.parse(readFileSync(new URL("./state-vectors.json", import.meta.url), "utf8"));
  const ledgerRules = C.decodeRules(fromHex(v.rules.bytes));
  assert.deepEqual(ledgerRules, v.rules.view);
  assert.ok(v.accounts.length >= 5);
  for (const a of v.accounts) {
    const bytes = fromHex(a.account.bytes);
    const account = C.decodeAccount(bytes);
    assert.deepEqual(account, a.account.view, a.name);
    assert.equal(C.decodeBalance(fromHex(a.balance.bytes)), a.balance.cents, a.name);
    const rules = a.rules ? C.decodeRules(fromHex(a.rules.bytes)) : ledgerRules;
    if (a.rules) assert.deepEqual(rules, a.rules.view, a.name);
    // What a claim would bring for birthdays: the app counts as the rules do.
    for (const d of a.due) {
      assert.equal(M.birthdaysDue({ cents: a.balance.cents, account, rules }, d.at * 1000), d.cents, `${a.name}, at ${d.at}`);
    }
    // A value laid out differently is refused, not guessed.
    assert.throws(() => C.decodeAccount(bytes.slice(0, -1)), RangeError);
    assert.throws(() => C.decodeAccount(concat(bytes, Uint8Array.of(0))), RangeError);
  }
  assert.ok(v.accounts.some((a) => a.due.some((d) => d.cents > 0)), "a birthday is due in the vectors");
  assert.equal(C.decodeAccount(null), null);
  assert.equal(C.decodeBalance(null), 0);
  assert.deepEqual(C.decodeRules(null), []);
  assert.throws(() => C.decodeRules(concat(fromHex(v.rules.bytes), Uint8Array.of(0))), RangeError);
});

test("a refusal of the free quota is said in words", () => {
  const invalid = (data) => ({ code: 1010, message: "Invalid Transaction", data });
  assert.match(C.refusalWords(invalid("Custom error: 32")), /enough for today/);
  assert.match(C.refusalWords(invalid("Custom error: 33")), /Verify yourself first/);
  // anything else keeps the node's own words
  assert.equal(C.refusalWords(invalid("Custom error: 3")), "Invalid Transaction");
  assert.equal(C.refusalWords(invalid("Transaction has a bad signature")), "Invalid Transaction");
  assert.equal(C.refusalWords({ message: "Method not found" }), "Method not found");
  assert.equal(C.refusalWords(undefined), "The network did not take it.");
});
