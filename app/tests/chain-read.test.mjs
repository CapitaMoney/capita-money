// SPDX-License-Identifier: AGPL-3.0-only
// The app's storage addresses and values (app/chain/xxhash.js, calls.js).
// The runtime checks the same vectors (runtime/src/tests.rs).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { xxhash64, twox128 } from "../chain/xxhash.js";
import * as C from "../chain/calls.js";
import { hex, concat, compact } from "../chain/keys.js";
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
