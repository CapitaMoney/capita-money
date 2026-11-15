// SPDX-License-Identifier: AGPL-3.0-only
// Adds the hardware-key vectors to the shared key vectors (ADR 0024):
//   node app/tests/make-hardware-vectors.mjs
// Reads crates/capita-keys/tests/vectors.json, writes its "hardware" part
// anew and keeps every other part. Run once; the keys are random.
//
// A hardware key is a P-256 key inside the phone's security chip (Android
// StrongBox / trusted environment, Apple Secure Enclave). The shell signs the
// 32-byte challenge with ECDSA and SHA-256 and turns the DER signature into
// r ‖ s – exactly what WebCrypto's ECDSA does here. The part also links a
// laptop's passkey to the phone's account: phone = home key, laptop = linked.

import { readFileSync, writeFileSync } from "node:fs";
import { webcrypto as crypto } from "node:crypto";
import * as K from "../chain/keys.js";

const subtle = crypto.subtle;
const enc = new TextEncoder();
const file = new URL("../../crates/capita-keys/tests/vectors.json", import.meta.url);
const V = JSON.parse(readFileSync(file, "utf8"));
const audience = K.fromHex(V.audience);
const remark = K.fromHex(V.remark_call);
const MOCK_KEYS = V.mock_keys_pallet;
const N = 0xffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551n;
const sha256 = async (b) => new Uint8Array(await subtle.digest("SHA-256", b));
const big = (b) => BigInt("0x" + K.hex(b));
const bytes32 = (n) => K.fromHex(n.toString(16).padStart(64, "0"));

async function p256() {
  const pair = await subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
  const raw = new Uint8Array(await subtle.exportKey("raw", pair.publicKey));
  const sign = async (m) => new Uint8Array(await subtle.sign({ name: "ECDSA", hash: "SHA-256" }, pair.privateKey, m));
  return { raw, public: K.compressP256(raw), sign };
}

const phone = await p256();
const key = { kind: "hardware", public: phone.public };
const account = K.accountOf(key);
const c0 = K.actChallenge(audience, account, 0, remark);
const open = { kind: "hardware", signature: await phone.sign(c0) };
const s = big(open.signature.subarray(32));
const flipped = K.concat(open.signature.subarray(0, 32), bytes32(N - s));
const c1 = K.actChallenge(audience, account, 1, remark);
const act = { kind: "hardware", signature: await phone.sign(c1) };

// The laptop's passkey proves possession; the phone sends the link.
const laptop = await p256();
const laptopKey = { kind: "passkey", public: laptop.public };
const cAdd = K.addChallenge(audience, account, laptopKey);
const authenticatorData = K.concat(await sha256(enc.encode(V.rp_id)), Uint8Array.of(0x05, 0, 0, 0, 0));
const clientDataJSON = enc.encode(JSON.stringify({ type: "webauthn.get", challenge: K.base64url(cAdd), origin: V.origin, crossOrigin: false }));
const possession = {
  kind: "passkey", authenticatorData, clientDataJSON,
  signature: await laptop.sign(K.concat(authenticatorData, await sha256(clientDataJSON))),
};
const addCall = K.addKeyCall(laptopKey, possession, MOCK_KEYS);
const c2 = K.actChallenge(audience, account, 2, addCall);
const act2 = { kind: "hardware", signature: await phone.sign(c2) };

V.hardware = {
  public: K.hex(key.public), uncompressed: K.hex(phone.raw), account: K.hex(account),
  open_challenge: K.hex(c0), open: { signature: K.hex(open.signature) }, open_other_s: K.hex(flipped),
  open_call: K.hex(K.openCall(key, remark, open, MOCK_KEYS)),
  act_challenge: K.hex(c1), act: { signature: K.hex(act.signature) },
  act_call: K.hex(K.actCall(account, key, remark, 1, act, MOCK_KEYS)),
  add_passkey: K.hex(laptopKey.public), add_challenge: K.hex(cAdd),
  add_possession: { authenticator_data: K.hex(authenticatorData), client_data_json: new TextDecoder().decode(clientDataJSON), signature: K.hex(possession.signature) },
  add_key_call: K.hex(addCall), add_act_challenge: K.hex(c2), add_act: { signature: K.hex(act2.signature) },
  add_act_call: K.hex(K.actCall(account, key, addCall, 2, act2, MOCK_KEYS)),
};
writeFileSync(file, JSON.stringify(V, null, 2) + "\n");
