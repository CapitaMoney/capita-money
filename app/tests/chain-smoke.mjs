// SPDX-License-Identifier: AGPL-3.0-only
// The app's key bytes against a running node (CI, job "node"):
//   node app/tests/chain-smoke.mjs http://127.0.0.1:9977
// A device key and a passkey (played by WebCrypto, as a phone would sign)
// each open their account with `keys.open`; the node's pool takes them only
// if the runtime – compiled to WebAssembly – accepts the signature. A forged
// signature must be refused. Then the screens' own calls: devices, and
// money (ADR 0027) – a person is verified, receives the age in Capita, pays
// another account, and the app's readers see both balances.
// No fees, a free quota instead (ADR 0029): a key without a person opens its
// account and presents its claim; anything else the node must refuse.

import { webcrypto as crypto, createPrivateKey, sign as signWith } from "node:crypto";
import * as K from "../chain/keys.js";
import * as C from "../chain/calls.js";
import * as M from "../money.js";
import { ageAt } from "../logic.js";
import { twox128 } from "../chain/xxhash.js";

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
// capita.open – what a new key opens its account with
const open = C.openAccount();
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
  const sign = async (inner) => new Uint8Array(await subtle.sign("Ed25519", pair.privateKey, K.actChallenge(audience, account, 0, inner)));
  const signature = await sign(open);
  const forged = signature.slice(); forged[0] ^= 1;
  await send("device key, forged", K.bareExtrinsic(K.openCall(key, open, { kind: "ed25519", signature: forged })), false);
  // rightly signed, but not what a key without a person may send
  await send("device key, a remark instead of opening", K.bareExtrinsic(K.openCall(key, remark, { kind: "ed25519", signature: await sign(remark) })), false);
  await send("device key", K.bareExtrinsic(K.openCall(key, open, { kind: "ed25519", signature })), true);
}

// a passkey
{
  const pair = await subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, true, ["sign", "verify"]);
  const key = { kind: "passkey", public: K.compressP256(new Uint8Array(await subtle.exportKey("raw", pair.publicKey))) };
  const account = K.accountOf(key);
  const challenge = K.actChallenge(audience, account, 0, open);
  const authenticatorData = K.concat(await sha256(enc.encode("app.capitamoney.com")), Uint8Array.of(0x05, 0, 0, 0, 0));
  const assertion = async (origin) => {
    const clientDataJSON = enc.encode(JSON.stringify({ type: "webauthn.get", challenge: K.base64url(challenge), origin, crossOrigin: false }));
    const signature = new Uint8Array(await subtle.sign({ name: "ECDSA", hash: "SHA-256" }, pair.privateKey,
      K.concat(authenticatorData, await sha256(clientDataJSON))));
    return { kind: "passkey", authenticatorData, clientDataJSON, signature };
  };
  await send("passkey from another site", K.bareExtrinsic(K.openCall(key, open, await assertion("https://evil.example"))), false);
  await send("passkey", K.bareExtrinsic(K.openCall(key, open, await assertion("https://app.capitamoney.com"))), true);
}
// What the screens' code does (app/device.js), with keys made here.
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
const sure = (label, ok) => {
  console.log(`${label}: ${ok ? "yes" : "NO"}`);
  if (!ok) { console.error(`FAILED: ${label}`); process.exit(1); }
};
const device = async () => {
  const pair = await subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  const key = { kind: "ed25519", public: new Uint8Array(await subtle.exportKey("raw", pair.publicKey)) };
  const sign = async (challenge) => ({ kind: "ed25519", signature: new Uint8Array(await subtle.sign("Ed25519", pair.privateKey, challenge)) });
  return { key, sign, account: K.accountOf(key) };
};
const nonceOf = async (account) => C.decodeNonce(await read(C.at.nonce(account)));
/** `who` acts for its own account (or for `account`, as a linked device). */
const act = async (label, who, inner, account = who.account) => {
  const n = await nonceOf(account);
  const proof = await who.sign(K.actChallenge(audience, account, n, inner));
  await send(label, K.bareExtrinsic(K.actCall(account, who.key, inner, n, proof)), true);
  await until(`${label} counted`, async () => (await nonceOf(account)) > n);
};
/** The node does not take `inner` from `who`: not into the pool, nothing counted. */
const refused = async (label, who, inner, account = who.account) => {
  const n = await nonceOf(account);
  const proof = await who.sign(K.actChallenge(audience, account, n, inner));
  await send(label, K.bareExtrinsic(K.actCall(account, who.key, inner, n, proof)), false);
  sure(`${label}: nothing counted`, (await nonceOf(account)) === n);
};
const openLedgerAccount = async (label, who) => {
  await send(label, K.bareExtrinsic(K.openCall(who.key, open, await who.sign(K.actChallenge(audience, who.account, 0, open)))), true);
  await until(`${label}: the ledger knows the account`, async () => !!(await read(C.at.capitaAccount(who.account))));
};
/** The account's money as app/device.js `money()` reads it. */
const money = async (account) => ({
  cents: C.decodeBalance(await read(C.at.balance(account))),
  account: C.decodeAccount(await read(C.at.capitaAccount(account))),
  rules: C.decodeRules(await read(C.at.rules())),
});

// The identity service of the test chains (its key is public) – what it
// signs, and a person born half a year away from today's date.
const be = (n, bytes) => { const out = new Uint8Array(bytes); let v = BigInt(n); for (let k = bytes - 1; k >= 0; k--) { out[k] = Number(v & 255n); v >>= 8n; } return out; };
// What an identity service signs (`capita_credential::encoding::claim_bytes`).
const signedBytes = (c) => K.concat(
  enc.encode("capita-claim-v3"), audience, be(c.issuer, 4), K.fromHex(c.serial), K.fromHex(c.subject), K.fromHex(c.subjectRef),
  Uint8Array.of(0), Uint8Array.of(c.level), Uint8Array.of(0),
  Uint8Array.of(1), be(Number(c.born.slice(0, 4)), 2), Uint8Array.of(Number(c.born.slice(5, 7)), Number(c.born.slice(8, 10))),
  be(0, 2), be(c.issuedAt, 8), Uint8Array.of(1), be(c.expiresAt, 8),
);
const issuer = createPrivateKey({
  key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), Buffer.from("capita-money development issuer!")]),
  format: "der", type: "pkcs8",
});
// The chain's clock, in seconds (`Timestamp::Now`, milliseconds).
const now = Number(new DataView((await read(K.concat(twox128(enc.encode("Timestamp")), twox128(enc.encode("Now"))))).buffer).getBigUint64(0, true) / 1000n);
// Born half a year away from today's date: no birthday falls into this run.
const today = new Date(now * 1000);
const half = new Date(Date.UTC(today.getUTCFullYear() - 30, today.getUTCMonth() + 6, 15));
const born = half.toISOString().slice(0, 10);
const age = ageAt(born, now * 1000);

/** The identity service of the test chains verifies `who`; `who` presents the claim. */
const verify = async (label, who) => {
  const claim = {
    issuer: 0, serial: "0x" + K.hex(crypto.getRandomValues(new Uint8Array(16))), subject: "0x" + K.hex(who.account),
    subjectRef: "0x" + K.hex(crypto.getRandomValues(new Uint8Array(32))), replaces: null, level: 1, residence: null, born,
    attributes: [], issuedAt: now - 30, expiresAt: now + 300 * 86400,
  };
  const answer = { call: "credential.present", claim, signature: "0x" + K.hex(new Uint8Array(signWith(null, signedBytes(claim), issuer))) };
  await act(label, who, C.claimCall(answer));
};

// the devices screen: open, verify (an account without a person links no
// device, ADR 0029), link a second device, the linked one asks to become
// the main one, the main one says no and removes it
{
  const phone = await device();
  const laptop = await device();
  const account = phone.account;
  await openLedgerAccount("open the account", phone);
  const laptopRaw = K.keyBytes(laptop.key);
  const possession = await laptop.sign(K.addChallenge(audience, account, laptop.key));
  await refused("add the laptop before verifying", phone, C.addKeyRaw(laptopRaw, K.proofBytes(possession)));
  await verify("the phone's holder verifies", phone);
  await until("the ledger knows the person", async () => M.isVerified(await money(account), now * 1000));
  await act("add the laptop", phone, C.addKeyRaw(laptopRaw, K.proofBytes(possession)));
  await until("two devices, the phone first", async () => {
    const keys = C.decodeKeys(await read(C.at.keys(account)));
    return keys.length === 2 && K.hex(keys[0].raw) === K.hex(K.keyBytes(phone.key)) && K.hex(keys[1].raw) === K.hex(laptopRaw);
  });
  await until("the laptop acts for the account", async () => K.hex((await read(C.at.keyOwner(laptopRaw))) ?? []) === K.hex(account));
  await act("the laptop asks to become the main device", laptop, C.claimHome(), account);
  await until("the claim is pending", async () => K.hex(C.decodeHomeClaim(await read(C.at.homeClaim(account)))?.key.raw ?? []) === K.hex(laptopRaw));
  await act("the phone says no", phone, C.refuseHome());
  await until("no claim left", async () => C.decodeHomeClaim(await read(C.at.homeClaim(account))) === null);
  await act("the phone removes the laptop", phone, C.removeKeyRaw(laptopRaw));
  await until("one device left", async () => C.decodeKeys(await read(C.at.keys(account))).length === 1);
}

// the money screens (ADR 0027): a person is verified by the identity
// service of the test chains (its key is public), receives the age in
// Capita in that transaction, and pays somebody. The app's readers see it.
{
  const anna = await device();
  const ben = await device();
  await openLedgerAccount("Anna opens her account", anna);
  await openLedgerAccount("Ben opens his account", ben);
  const fresh = await money(anna.account);
  const rules = M.rulesAt(fresh.rules, now);
  sure("the app reads the rules of the chain", !!rules && rules.age.perYearCents > 0 && rules.age.atVerificationCents > 0 && rules.claimWindow > 0);
  sure("a new account holds the welcome gift, earmarked", fresh.cents === rules.startGrantCents && fresh.account.reservedCents === rules.startGrantCents
    && M.spendableCents(fresh) === 0 && !M.isVerified(fresh, now * 1000));

  // Not verified: no payment – the node takes nothing from her but her verification.
  await refused("Anna tries to pay before she is verified", anna, C.transfer(ben.account, 1));
  await verify("Anna presents her verification", anna);
  const first = age * rules.age.atVerificationCents;
  await until("the ledger paid her age in Capita at once", async () => (await money(anna.account)).cents === rules.startGrantCents + first);
  const verified = await money(anna.account);
  sure("the app reads her as verified, with her date of birth", M.isVerified(verified, now * 1000) && verified.account.born === born
    && verified.account.firstPaid && M.spendableCents(verified) === first);
  const next = M.nextBirthday(verified, now * 1000);
  sure("the next birthday is known, and nothing is due before it", next.age === age + 1 && next.cents === (age + 1) * rules.age.perYearCents
    && M.birthdaysDue(verified, now * 1000) === 0);

  const benBefore = await money(ben.account);
  await act("Anna pays Ben 12.50", anna, C.transfer(ben.account, 1250));
  await until("Ben has the money", async () => (await money(ben.account)).cents === rules.startGrantCents + 1250);
  const paid = await money(anna.account);
  sure("Anna's balance went down by 12.50, one ledger operation more", paid.cents === verified.cents - 1250 && paid.account.moves === verified.account.moves + 1);
  sure("a payment received is no operation of the receiver", (await money(ben.account)).account.moves === benBefore.account.moves);

  // No debt: more than she may send changes nothing (the call is counted, the payment refused).
  await act("Anna tries to pay more than she has", anna, C.transfer(ben.account, M.spendableCents(paid) + 1));
  const after = await money(anna.account);
  sure("nothing left her account", after.cents === paid.cents && after.account.moves === paid.account.moves);
  // Ben is not verified: what he received waits until he is.
  await refused("Ben tries to send before he is verified", ben, C.transfer(anna.account, 1250));
  sure("Ben keeps it", (await money(ben.account)).cents === rules.startGrantCents + 1250);
  await verify("Ben presents his verification", ben);
  await until("the ledger paid Ben his age too", async () => (await money(ben.account)).cents === rules.startGrantCents + 1250 + first);
  // The welcome gift cannot be sent on.
  const benVerified = await money(ben.account);
  sure("Ben may send all but the welcome gift", M.spendableCents(benVerified) === 1250 + first);
  await act("Ben tries to send his welcome gift on", ben, C.transfer(anna.account, 1250 + first + 1));
  sure("Ben keeps it all", (await money(ben.account)).cents === benVerified.cents);
  await act("Ben sends what he received", ben, C.transfer(anna.account, 1250));
  await until("Anna has it back", async () => (await money(anna.account)).cents === verified.cents);

  // A claim with no birthday due is counted and brings no birthday money.
  // (The `--dev` chain also issues a cent per minute, which the alpha rules
  // do not have: a few cents may come.)
  await act("Anna claims with no birthday due", anna, C.claim());
  const claimed = await money(anna.account);
  sure("the claim brought no birthday money", claimed.cents >= verified.cents && claimed.cents < verified.cents + 100);
}
console.log("RESULT: OK");
