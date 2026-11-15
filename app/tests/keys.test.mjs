// SPDX-License-Identifier: AGPL-3.0-only
// The app's key bytes against references and against the vectors the Rust
// side checks too (crates/capita-keys/tests/vectors.json).
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { webcrypto as crypto } from "node:crypto";
import { blake2b } from "../chain/blake2b.js";
import * as K from "../chain/keys.js";

const subtle = crypto.subtle;
const enc = new TextEncoder();
const V = JSON.parse(readFileSync(new URL("../../crates/capita-keys/tests/vectors.json", import.meta.url), "utf8"));
const H = K.fromHex;

test("BLAKE2b-512 of 'abc' (RFC 7693, appendix A)", () => {
  assert.equal(K.hex(blake2b(enc.encode("abc"), 64)),
    "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923");
});

test("BLAKE2b-256 as Python's hashlib computes it", () => {
  const range = Uint8Array.from({ length: 256 }, (_, i) => i);
  assert.equal(K.hex(blake2b(new Uint8Array(0))), "0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8");
  assert.equal(K.hex(blake2b(enc.encode("abc"))), "bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319");
  assert.equal(K.hex(blake2b(range)), "39a7eb9fedc19aabc83425c6755dd90e6f9d0c804964a1f4aaeea3b9fb599835");
  assert.equal(K.hex(blake2b(new Uint8Array(128))), "378d0caaaa3855f1b38693c1d6ef004fd118691c95c959d4efa950d6d6fcf7c1");
});

test("SCALE compact integers", () => {
  const c = (n) => K.hex(K.compact(n));
  assert.equal(c(0), "00");
  assert.equal(c(1), "04");
  assert.equal(c(63), "fc");
  assert.equal(c(64), "0101");
  assert.equal(c(16383), "fdff");
  assert.equal(c(16384), "02000100");
});

test("base64url without padding", () => {
  const b = (s) => K.base64url(enc.encode(s));
  assert.equal(b(""), "");
  assert.equal(b("f"), "Zg");
  assert.equal(b("fo"), "Zm8");
  assert.equal(b("foo"), "Zm9v");
  assert.equal(K.base64url(Uint8Array.of(0xff, 0xfe)), "__4");
});

test("DER signature and P-256 key forms", async () => {
  const pair = await subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
  const raw = new Uint8Array(await subtle.exportKey("raw", pair.publicKey));
  const spki = new Uint8Array(await subtle.exportKey("spki", pair.publicKey));
  assert.deepEqual(K.compressP256(spki), K.compressP256(raw));
  assert.equal(K.compressP256(raw).length, 33);
  // r with high bit set gets a 0 byte in DER; s short
  const r = new Uint8Array(32).fill(0x80);
  const s = Uint8Array.of(...new Uint8Array(31), 7);
  const der = Uint8Array.of(0x30, 0x26, 0x02, 0x21, 0x00, ...r, 0x02, 0x01, 0x07);
  assert.deepEqual(K.derToRaw(der), K.concat(r, s));
});

test("Ed25519 vectors: account, challenges, calls, signatures", async () => {
  const e = V.ed25519;
  const aud = H(V.audience);
  const remark = H(V.remark_call);
  const key = { kind: "ed25519", public: H(e.public) };
  const account = K.accountOf(key);
  assert.equal(K.hex(account), e.account);
  assert.equal(K.hex(K.actChallenge(aud, account, 0, remark)), e.open_challenge);
  assert.equal(K.hex(K.actChallenge(aud, account, 1, remark)), e.act_challenge);
  const pub = await subtle.importKey("raw", key.public, "Ed25519", false, ["verify"]);
  assert.ok(await subtle.verify("Ed25519", pub, H(e.open.signature), H(e.open_challenge)));
  assert.ok(await subtle.verify("Ed25519", pub, H(e.act.signature), H(e.act_challenge)));
  const open = { kind: "ed25519", signature: H(e.open.signature) };
  assert.equal(K.hex(K.openCall(key, remark, open, V.mock_keys_pallet)), e.open_call);
  const added = { kind: "ed25519", public: H(e.add_public) };
  assert.equal(K.hex(K.addChallenge(aud, account, added)), e.add_challenge);
  const addCall = K.addKeyCall(added, { kind: "ed25519", signature: H(e.add_possession.signature) }, V.mock_keys_pallet);
  assert.equal(K.hex(addCall), e.add_key_call);
  assert.equal(K.hex(K.actChallenge(aud, account, 2, addCall)), e.add_act_challenge);
});

test("passkey vectors: account, client data, signatures", async () => {
  const v = V.passkey;
  const aud = H(V.audience);
  const remark = H(V.remark_call);
  const key = { kind: "passkey", public: H(v.public) };
  assert.deepEqual(K.compressP256(H(v.uncompressed)), key.public);
  const account = K.accountOf(key);
  assert.equal(K.hex(account), v.account);
  assert.equal(K.hex(K.actChallenge(aud, account, 0, remark)), v.open_challenge);
  const cd = JSON.parse(v.open.client_data_json);
  assert.equal(cd.type, "webauthn.get");
  assert.equal(cd.challenge, K.base64url(H(v.open_challenge)));
  assert.equal(cd.origin, V.origin);
  const pub = await subtle.importKey("raw", H(v.uncompressed), { name: "ECDSA", namedCurve: "P-256" }, false, ["verify"]);
  const sha = async (b) => new Uint8Array(await subtle.digest("SHA-256", b));
  const msg = K.concat(H(v.open.authenticator_data), await sha(enc.encode(v.open.client_data_json)));
  assert.ok(await subtle.verify({ name: "ECDSA", hash: "SHA-256" }, pub, H(v.open.signature), msg));
  const proof = { kind: "passkey", authenticatorData: H(v.open.authenticator_data), clientDataJSON: enc.encode(v.open.client_data_json), signature: H(v.open.signature) };
  assert.equal(K.hex(K.openCall(key, remark, proof, V.mock_keys_pallet)), v.open_call);
});

test("a transaction without signer", () => {
  assert.equal(K.hex(K.bareExtrinsic(Uint8Array.of(1, 2))), "0c040102");
});
