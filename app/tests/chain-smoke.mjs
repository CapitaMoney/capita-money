// SPDX-License-Identifier: AGPL-3.0-only
// The app's key bytes against a running node (CI, job "node"):
//   node app/tests/chain-smoke.mjs http://127.0.0.1:9977
// A device key and a passkey (played by WebCrypto, as a phone would sign)
// each open their account with `keys.open`; the node's pool takes them only
// if the runtime – compiled to WebAssembly – accepts the signature. A forged
// signature must be refused.

import { webcrypto as crypto } from "node:crypto";
import * as K from "../chain/keys.js";
import * as C from "../chain/calls.js";

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
// the devices screen (app/device.js): open, link a second device, the
// linked one asks to become the main one, the main one says no and removes it
{
  const read = async (address) => {
    const j = await call("state_getStorage", ["0x" + K.hex(address)]);
    return j.result ? K.fromHex(j.result) : null;
  };
  const until = async (label, check, seconds = 90) => {
    for (const end = Date.now() + seconds * 1000; Date.now() < end;) {
      if (await check().catch(() => false)) return console.log(`${label}: yes`);
      await new Promise((r) => setTimeout(r, 1500));
    }
    console.error(`FAILED: ${label}`);
    process.exit(1);
  };
  const device = async () => {
    const pair = await subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
    const key = { kind: "ed25519", public: new Uint8Array(await subtle.exportKey("raw", pair.publicKey)) };
    const sign = async (challenge) => ({ kind: "ed25519", signature: new Uint8Array(await subtle.sign("Ed25519", pair.privateKey, challenge)) });
    return { key, sign };
  };
  const phone = await device();
  const laptop = await device();
  const account = K.accountOf(phone.key);
  const nonce = async () => C.decodeNonce(await read(C.at.nonce(account)));
  const act = async (label, who, inner) => {
    const n = await nonce();
    const proof = await who.sign(K.actChallenge(audience, account, n, inner));
    await send(label, K.bareExtrinsic(K.actCall(account, who.key, inner, n, proof)), true);
    await until(`${label} counted`, async () => (await nonce()) > n);
  };
  const open = C.openAccount();
  await send("open the account", K.bareExtrinsic(K.openCall(phone.key, open, await phone.sign(K.actChallenge(audience, account, 0, open)))), true);
  await until("the ledger knows the account", async () => !!(await read(C.at.capitaAccount(account))));
  const laptopRaw = K.keyBytes(laptop.key);
  const possession = await laptop.sign(K.addChallenge(audience, account, laptop.key));
  await act("add the laptop", phone, C.addKeyRaw(laptopRaw, K.proofBytes(possession)));
  await until("two devices, the phone first", async () => {
    const keys = C.decodeKeys(await read(C.at.keys(account)));
    return keys.length === 2 && K.hex(keys[0].raw) === K.hex(K.keyBytes(phone.key)) && K.hex(keys[1].raw) === K.hex(laptopRaw);
  });
  await until("the laptop acts for the account", async () => K.hex((await read(C.at.keyOwner(laptopRaw))) ?? []) === K.hex(account));
  await act("the laptop asks to become the main device", laptop, C.claimHome());
  await until("the claim is pending", async () => K.hex(C.decodeHomeClaim(await read(C.at.homeClaim(account)))?.key.raw ?? []) === K.hex(laptopRaw));
  await act("the phone says no", phone, C.refuseHome());
  await until("no claim left", async () => C.decodeHomeClaim(await read(C.at.homeClaim(account))) === null);
  await act("the phone removes the laptop", phone, C.removeKeyRaw(laptopRaw));
  await until("one device left", async () => C.decodeKeys(await read(C.at.keys(account))).length === 1);
}
console.log("RESULT: OK");
