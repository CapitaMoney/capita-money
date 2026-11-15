// SPDX-License-Identifier: AGPL-3.0-only
// The money key of a person, on this device (ADR 0020).
//
// A key is a passkey (WebAuthn, P-256, kept by the phone, confirmed by
// fingerprint or face) or – where no passkey is possible – an Ed25519 key
// made by the browser, not extractable. The account is derived from the
// first key; every action is signed by one of the account's keys and sent
// to the chain without signer and without fee (`keys.act`).
//
// The bytes here are the bytes of `capita_keys::encoding` and of the calls
// of `pallet-capita-keys`. Both sides test against the same vectors
// (`crates/capita-keys/tests/vectors.json`, made by
// `app/tests/make-key-vectors.mjs`).
//
// Nothing of this is ever shown to the person.

import { blake2b } from "./blake2b.js";

const enc = new TextEncoder();
export const TAG_ACCOUNT = enc.encode("capita/account/v1");
export const TAG_ACT = enc.encode("capita/act/v1");
export const TAG_ADD = enc.encode("capita/add/v1");

/** Index of `pallet-capita-keys` in the runtime and its calls. */
export const KEYS = Object.freeze({ pallet: 24, act: 0, open: 1, addKey: 2, removeKey: 3, claimHome: 4, takeHome: 5, refuseHome: 6 });

// ---------------------------------------------------------------- bytes

export function concat(...parts) {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let off = 0;
  for (const p of parts) { out.set(p, off); off += p.length; }
  return out;
}

export function u32le(n) {
  const b = new Uint8Array(4);
  new DataView(b.buffer).setUint32(0, n, true);
  return b;
}

/** SCALE compact integer (lengths of byte strings), up to 2^30 - 1. */
export function compact(n) {
  if (n < 0 || !Number.isInteger(n)) throw new RangeError("compact: natural number");
  if (n < 1 << 6) return Uint8Array.of(n << 2);
  if (n < 1 << 14) { const v = (n << 2) | 1; return Uint8Array.of(v & 0xff, v >> 8); }
  if (n < 1 << 30) return u32le(((n << 2) | 2) >>> 0);
  throw new RangeError("compact: too large");
}

/** SCALE `Vec<u8>`. */
export const bytesVec = (b) => concat(compact(b.length), b);

export const hex = (b) => Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
export function fromHex(s) {
  const t = s.startsWith("0x") ? s.slice(2) : s;
  return Uint8Array.from(t.match(/../g) ?? [], (x) => parseInt(x, 16));
}

const B64U = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
/** base64url without padding – how WebAuthn writes the challenge. */
export function base64url(bytes) {
  let s = "";
  for (let i = 0; i < bytes.length; i += 3) {
    const n = (bytes[i] << 16) | ((bytes[i + 1] ?? 0) << 8) | (bytes[i + 2] ?? 0);
    const chars = Math.min(4, bytes.length - i + 1);
    for (let k = 0; k < chars; k++) s += B64U[(n >> (18 - 6 * k)) & 63];
  }
  return s;
}

// ---------------------------------------------------------------- keys

/**
 * `DeviceKey` in SCALE: 0 ‖ Ed25519 key (32), 1 ‖ compressed P-256 passkey (33)
 * or 2 ‖ compressed P-256 hardware key of the phone shell (33, ADR 0024).
 */
export function keyBytes(key) {
  if (key.kind === "ed25519" && key.public.length === 32) return concat(Uint8Array.of(0), key.public);
  if (key.kind === "passkey" && key.public.length === 33) return concat(Uint8Array.of(1), key.public);
  if (key.kind === "hardware" && key.public.length === 33) return concat(Uint8Array.of(2), key.public);
  throw new TypeError("unknown key");
}

/** The account a first key opens: BLAKE2b-256("capita/account/v1" ‖ key). */
export const accountOf = (key) => blake2b(concat(TAG_ACCOUNT, keyBytes(key)));

/** What a key signs to act: BLAKE2b-256(tag ‖ audience ‖ account ‖ nonce LE ‖ BLAKE2b-256(call)). */
export const actChallenge = (audience, account, nonce, call) =>
  blake2b(concat(TAG_ACT, audience, account, u32le(nonce), blake2b(call)));

/** What a new key signs to be linked to an account. */
export const addChallenge = (audience, account, key) => blake2b(concat(TAG_ADD, audience, account, keyBytes(key)));

/** `Proof` in SCALE. */
export function proofBytes(proof) {
  if (proof.kind === "ed25519") return concat(Uint8Array.of(0), proof.signature);
  if (proof.kind === "passkey") {
    return concat(Uint8Array.of(1), bytesVec(proof.authenticatorData), bytesVec(proof.clientDataJSON), proof.signature);
  }
  if (proof.kind === "hardware" && proof.signature.length === 64) return concat(Uint8Array.of(2), proof.signature);
  throw new TypeError("unknown proof");
}

// ---------------------------------------------------------------- calls

/** `keys.act(account, key, call, nonce, proof)`. */
export const actCall = (account, key, call, nonce, proof, pallet = KEYS.pallet) =>
  concat(Uint8Array.of(pallet, KEYS.act), account, keyBytes(key), call, u32le(nonce), proofBytes(proof));

/** `keys.open(key, call, proof)`: first key of a new account, and its first action (nonce 0). */
export const openCall = (key, call, proof, pallet = KEYS.pallet) =>
  concat(Uint8Array.of(pallet, KEYS.open), keyBytes(key), call, proofBytes(proof));

/** `keys.add_key(key, possession)` – sent through `act`. */
export const addKeyCall = (key, possession, pallet = KEYS.pallet) =>
  concat(Uint8Array.of(pallet, KEYS.addKey), keyBytes(key), proofBytes(possession));

/** `keys.remove_key(key)` – sent through `act`. */
export const removeKeyCall = (key, pallet = KEYS.pallet) => concat(Uint8Array.of(pallet, KEYS.removeKey), keyBytes(key));

/** A transaction without signer (format 4): compact length ‖ 0x04 ‖ call. */
export const bareExtrinsic = (call) => concat(compact(call.length + 1), Uint8Array.of(0x04), call);

// ---------------------------------------------------------------- P-256 forms

/** ECDSA signature: ASN.1 DER (what WebAuthn returns) -> 64 bytes r ‖ s. */
export function derToRaw(der) {
  let i = 0;
  const expect = (tag) => { if (der[i++] !== tag) throw new Error("not a DER signature"); };
  const len = () => { const l = der[i++]; if (l & 0x80) throw new Error("long DER length"); return l; };
  expect(0x30); len();
  const int = () => {
    expect(0x02);
    let l = len(); let v = der.subarray(i, i + l); i += l;
    while (v.length > 32 && v[0] === 0) v = v.subarray(1);
    if (v.length > 32) throw new Error("integer too long");
    const out = new Uint8Array(32); out.set(v, 32 - v.length); return out;
  };
  return concat(int(), int());
}

/** P-256 public key: SubjectPublicKeyInfo or raw 65 bytes -> compressed 33 bytes. */
export function compressP256(key) {
  const raw = key.length === 65 ? key : key.subarray(key.length - 65);
  if (raw[0] !== 4) throw new Error("not an uncompressed P-256 point");
  return concat(Uint8Array.of(2 | (raw[64] & 1)), raw.subarray(1, 33));
}

// ---------------------------------------------------------------- devices

/** Whether this device can make a passkey that asks for fingerprint, face or screen lock. */
export async function passkeyPossible() {
  try {
    return typeof PublicKeyCredential !== "undefined"
      && await PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable();
  } catch { return false; }
}

/** Makes a passkey for the app's name. Answers the key and the credential id to ask for later. */
export async function createPasskey({ rpId, userName }) {
  const cred = await navigator.credentials.create({
    publicKey: {
      rp: { id: rpId, name: "Capita" },
      user: { id: crypto.getRandomValues(new Uint8Array(16)), name: userName, displayName: userName },
      challenge: crypto.getRandomValues(new Uint8Array(32)),
      pubKeyCredParams: [{ type: "public-key", alg: -7 }],
      authenticatorSelection: { residentKey: "required", userVerification: "required" },
      attestation: "none",
    },
  });
  const spki = new Uint8Array(cred.response.getPublicKey());
  return { key: { kind: "passkey", public: compressP256(spki) }, credentialId: new Uint8Array(cred.rawId) };
}

/** Signs a challenge with a passkey (the phone asks for fingerprint, face or screen lock). */
export async function passkeyProof({ rpId, credentialId, challenge }) {
  const a = await navigator.credentials.get({
    publicKey: {
      challenge, rpId, userVerification: "required",
      allowCredentials: credentialId ? [{ type: "public-key", id: credentialId }] : [],
    },
  });
  return {
    kind: "passkey",
    authenticatorData: new Uint8Array(a.response.authenticatorData),
    clientDataJSON: new Uint8Array(a.response.clientDataJSON),
    signature: derToRaw(new Uint8Array(a.response.signature)),
  };
}

// ---------------------------------------------------------------- hardware key (phone shells, ADR 0024)

const keyPlugin = () => globalThis.Capacitor?.Plugins?.CapitaKey;
const fromB64 = (t) => Uint8Array.from(atob(t), (c) => c.charCodeAt(0));
const toB64 = (b) => btoa(String.fromCharCode(...b));

/** Whether this device keeps keys in its security chip (the Android or iPhone shell). */
export async function hardwarePossible() {
  const p = keyPlugin();
  if (!p) return false;
  try { return !!(await p.status()).possible; } catch { return false; }
}

/** The phone's hardware key, if it has one. */
export async function hardwareKey() {
  const s = await keyPlugin()?.status();
  return s?.exists ? { kind: "hardware", public: fromB64(s.public) } : null;
}

/** Makes the money key inside the phone's security chip. */
export async function createHardwareKey() {
  const r = await keyPlugin().create({});
  return { key: { kind: "hardware", public: fromB64(r.public) }, strongBox: !!r.strongBox };
}

/** Signs a challenge with the hardware key; the phone asks for fingerprint, face or screen lock. */
export async function hardwareProof(challenge, { title = "Confirm", subtitle = "" } = {}) {
  const { signature } = await keyPlugin().sign({ challenge: toB64(challenge), title, subtitle });
  return { kind: "hardware", signature: derToRaw(fromB64(signature)) };
}

/**
 * The best key this device can make (ADR 0024): the security chip in the
 * phone shells, else a passkey, else a key the browser keeps.
 */
export async function bestKeyKind() {
  if (await hardwarePossible()) return "hardware";
  if (await passkeyPossible()) return "passkey";
  return "ed25519";
}

/** Makes an Ed25519 key in the browser; the secret half cannot be read out. */
export async function createDeviceKey() {
  const pair = await crypto.subtle.generateKey({ name: "Ed25519" }, false, ["sign", "verify"]);
  const pub = new Uint8Array(await crypto.subtle.exportKey("raw", pair.publicKey));
  return { key: { kind: "ed25519", public: pub }, privateKey: pair.privateKey };
}

/** Signs a challenge with a device key. */
export async function deviceKeyProof(privateKey, challenge) {
  const sig = new Uint8Array(await crypto.subtle.sign({ name: "Ed25519" }, privateKey, challenge));
  return { kind: "ed25519", signature: sig };
}

/** Sends a transaction to a node (JSON-RPC over HTTP). Answers its hash. */
export async function submit(rpcUrl, extrinsic) {
  const r = await fetch(rpcUrl, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "author_submitExtrinsic", params: ["0x" + hex(extrinsic)] }),
  });
  const j = await r.json();
  if (j.error) throw new Error(j.error.message);
  return j.result;
}
