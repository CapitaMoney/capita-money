// SPDX-License-Identifier: AGPL-3.0-only
// The app's key bytes against a running node (CI, job "node"):
//   node app/tests/chain-smoke.mjs http://127.0.0.1:9977
// A device key and a passkey (played by WebCrypto, as a phone would sign)
// each open their account with `keys.open`; the node's pool takes them only
// if the runtime – compiled to WebAssembly – accepts the signature. A forged
// signature must be refused.

import { webcrypto as crypto } from "node:crypto";
import * as K from "../chain/keys.js";

const rpc = process.argv[2] ?? "http://127.0.0.1:9944";
const subtle = crypto.subtle;
const enc = new TextEncoder();

async function call(method, params) {
  const r = await fetch(rpc, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });
  return r.json();
}

const audience = K.fromHex((await call("chain_getBlockHash", [0])).result);
// system.remark(b"capita") – System is pallet 0, remark call 0
const note = enc.encode("capita");
const remark = K.concat(Uint8Array.of(0, 0), K.compact(note.length), note);
const sha256 = async (b) => new Uint8Array(await subtle.digest("SHA-256", b));

async function send(label, extrinsic, expectOk) {
  const j = await call("author_submitExtrinsic", ["0x" + K.hex(extrinsic)]);
  const ok = typeof j.result === "string";
  console.log(`${label}: ${ok ? "taken " + j.result : "refused – " + (j.error?.message ?? "?") + " " + (j.error?.data ?? "")}`);
  if (ok !== expectOk) { console.error(`FAILED: ${label}`); process.exit(1); }
}

// a device key
{
  const pair = await subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  const key = { kind: "ed25519", public: new Uint8Array(await subtle.exportKey("raw", pair.publicKey)) };
  const account = K.accountOf(key);
  const challenge = K.actChallenge(audience, account, 0, remark);
  const signature = new Uint8Array(await subtle.sign("Ed25519", pair.privateKey, challenge));
  const forged = signature.slice(); forged[0] ^= 1;
  await send("device key, forged", K.bareExtrinsic(K.openCall(key, remark, { kind: "ed25519", signature: forged })), false);
  await send("device key", K.bareExtrinsic(K.openCall(key, remark, { kind: "ed25519", signature })), true);
}

// a passkey
{
  const pair = await subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
  const key = { kind: "passkey", public: K.compressP256(new Uint8Array(await subtle.exportKey("raw", pair.publicKey))) };
  const account = K.accountOf(key);
  const challenge = K.actChallenge(audience, account, 0, remark);
  const authenticatorData = K.concat(await sha256(enc.encode("app.capitamoney.com")), Uint8Array.of(0x05, 0, 0, 0, 0));
  const assertion = async (origin) => {
    const clientDataJSON = enc.encode(JSON.stringify({ type: "webauthn.get", challenge: K.base64url(challenge), origin, crossOrigin: false }));
    const signature = new Uint8Array(await subtle.sign({ name: "ECDSA", hash: "SHA-256" }, pair.privateKey,
      K.concat(authenticatorData, await sha256(clientDataJSON))));
    return { kind: "passkey", authenticatorData, clientDataJSON, signature };
  };
  await send("passkey from another site", K.bareExtrinsic(K.openCall(key, remark, await assertion("https://evil.example"))), false);
  await send("passkey", K.bareExtrinsic(K.openCall(key, remark, await assertion("https://app.capitamoney.com"))), true);
}
console.log("RESULT: OK");
