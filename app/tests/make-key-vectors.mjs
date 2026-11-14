// SPDX-License-Identifier: AGPL-3.0-only
// Writes the key vectors both sides test against:
//   node app/tests/make-key-vectors.mjs > crates/capita-keys/tests/vectors.json
// Run once; the keys are random, the file is kept. The app's tests
// (keys.test.mjs) and the Rust tests (capita-keys, pallet-capita-keys) check
// that their bytes, hashes and signature checks agree with it.
//
// The calls are those of the test runtime of `pallet-capita-keys`
// (`pallets/keys/src/mock.rs`): System = pallet 0, Keys = pallet 1, and the
// audience is the hash of block 0 there ([0x45; 32]).

import { webcrypto as crypto } from "node:crypto";
import * as K from "../chain/keys.js";

const subtle = crypto.subtle;
const enc = new TextEncoder();
const audience = new Uint8Array(32).fill(0x45);
const rpId = "app.capitamoney.com";
const origin = "https://app.capitamoney.com";
const MOCK_KEYS = 1;
// system.remark([1, 2, 3]) in the test runtime
const remark = Uint8Array.of(0, 0, ...K.compact(3), 1, 2, 3);
const N = 0xffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551n;

const sha256 = async (b) => new Uint8Array(await subtle.digest("SHA-256", b));
const big = (b) => BigInt("0x" + K.hex(b));
const bytes32 = (n) => K.fromHex(n.toString(16).padStart(64, "0"));

async function ed25519() {
  const pair = await subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  const pub = new Uint8Array(await subtle.exportKey("raw", pair.publicKey));
  return { key: { kind: "ed25519", public: pub }, sign: async (m) => new Uint8Array(await subtle.sign("Ed25519", pair.privateKey, m)) };
}

async function passkey() {
  const pair = await subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
  const raw = new Uint8Array(await subtle.exportKey("raw", pair.publicKey));
  const key = { kind: "passkey", public: K.compressP256(raw) };
  const authenticatorData = K.concat(await sha256(enc.encode(rpId)), Uint8Array.of(0x05, 0, 0, 0, 0));
  const prove = async (challenge) => {
    const clientDataJSON = enc.encode(JSON.stringify({
      type: "webauthn.get", challenge: K.base64url(challenge), origin, crossOrigin: false,
    }));
    const msg = K.concat(authenticatorData, await sha256(clientDataJSON));
    const signature = new Uint8Array(await subtle.sign({ name: "ECDSA", hash: "SHA-256" }, pair.privateKey, msg));
    return { kind: "passkey", authenticatorData, clientDataJSON, signature };
  };
  return { key, raw, prove };
}

const p = (proof) => proof.kind === "ed25519"
  ? { signature: K.hex(proof.signature) }
  : { authenticator_data: K.hex(proof.authenticatorData), client_data_json: new TextDecoder().decode(proof.clientDataJSON), signature: K.hex(proof.signature) };

const out = { note: "Made by app/tests/make-key-vectors.mjs. Random keys; keep the file.", audience: K.hex(audience), rp_id: rpId, origin, remark_call: K.hex(remark), mock_keys_pallet: MOCK_KEYS };

// Ed25519: open (nonce 0) and act (nonce 1), then link a second key (nonce 2)
{
  const a = await ed25519();
  const account = K.accountOf(a.key);
  const c0 = K.actChallenge(audience, account, 0, remark);
  const open = { kind: "ed25519", signature: await a.sign(c0) };
  const c1 = K.actChallenge(audience, account, 1, remark);
  const act = { kind: "ed25519", signature: await a.sign(c1) };
  const b = await ed25519();
  const cAdd = K.addChallenge(audience, account, b.key);
  const possession = { kind: "ed25519", signature: await b.sign(cAdd) };
  const addCall = K.addKeyCall(b.key, possession, MOCK_KEYS);
  const c2 = K.actChallenge(audience, account, 2, addCall);
  const act2 = { kind: "ed25519", signature: await a.sign(c2) };
  out.ed25519 = {
    public: K.hex(a.key.public), account: K.hex(account),
    open_challenge: K.hex(c0), open: p(open), open_call: K.hex(K.openCall(a.key, remark, open, MOCK_KEYS)),
    act_challenge: K.hex(c1), act: p(act), act_call: K.hex(K.actCall(account, a.key, remark, 1, act, MOCK_KEYS)),
    add_public: K.hex(b.key.public), add_challenge: K.hex(cAdd), add_possession: p(possession),
    add_key_call: K.hex(addCall), add_act_challenge: K.hex(c2), add_act: p(act2),
    add_act_call: K.hex(K.actCall(account, a.key, addCall, 2, act2, MOCK_KEYS)),
  };
}

// Passkey: open (nonce 0) and act (nonce 1); the open signature also with high s
{
  const k = await passkey();
  const account = K.accountOf(k.key);
  const c0 = K.actChallenge(audience, account, 0, remark);
  const open = await k.prove(c0);
  const s = big(open.signature.subarray(32));
  const flipped = K.concat(open.signature.subarray(0, 32), bytes32(N - s));
  const c1 = K.actChallenge(audience, account, 1, remark);
  const act = await k.prove(c1);
  out.passkey = {
    public: K.hex(k.key.public), uncompressed: K.hex(k.raw), account: K.hex(account),
    open_challenge: K.hex(c0), open: p(open), open_other_s: K.hex(flipped),
    open_call: K.hex(K.openCall(k.key, remark, open, MOCK_KEYS)),
    act_challenge: K.hex(c1), act: p(act), act_call: K.hex(K.actCall(account, k.key, remark, 1, act, MOCK_KEYS)),
  };
}

process.stdout.write(JSON.stringify(out, null, 2) + "\n");
