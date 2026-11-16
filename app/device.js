// SPDX-License-Identifier: AGPL-3.0-only
// This device on a test network (ADR 0020, ADR 0024, ADR 0025): its money
// key, the account the key acts for, the node and the identity service it
// talks to, and the hand-over box between two devices. No screens here.
//
// Without a test network the app stays the demo of app.js: nothing leaves
// the device. With one, verification, devices and money are real: the
// balance is the ledger's, a payment is signed by this device's key
// (ADR 0027).

import * as K from "./chain/keys.js";
import * as C from "./chain/calls.js";

const SAVED = "capita-device-v1";
/** `keys.home_delay` of `rules/alpha.toml`: a linked device takes the home after this. */
export const HOME_DELAY_SECS = 7 * 86400;
/** Sites whose passkeys the test chains accept (`runtime/src/genesis_config_presets.rs`). */
const PASSKEY_SITES = ["app.capitamoney.com", "capitamoney.github.io", "localhost"];

// ---------------------------------------------------------------- saved state

let saved = null;
function storage() { try { return globalThis.localStorage ?? null; } catch { return null; } }
export function load() {
  try { saved = JSON.parse(storage()?.getItem(SAVED) ?? "null"); } catch { saved = null; }
  return saved;
}
function keep() { try { storage()?.setItem(SAVED, JSON.stringify(saved)); } catch { /* memory only */ } }
export const current = () => saved;
export const on = () => !!saved?.node && !!saved?.service;
export const hasKey = () => !!saved?.key;
export const account = () => K.fromHex(saved.account);
export const hex = K.hex;
export const fromHexAccount = (text) => K.fromHex(text);
export const keyRaw = () => K.fromHex(saved.key.raw);

/** Forgets the network, this device's key and account (the demo stays). */
export async function forget() {
  saved = null;
  try { storage()?.removeItem(SAVED); } catch { /* nothing kept */ }
  await vault("delete").catch(() => {});
}

// The Ed25519 key a browser keeps cannot be read out; IndexedDB keeps it as it is.
function vault(op, value) {
  return new Promise((resolve, reject) => {
    if (!globalThis.indexedDB) return reject(new Error("no IndexedDB"));
    const open = indexedDB.open("capita", 1);
    open.onupgradeneeded = () => open.result.createObjectStore("keys");
    open.onerror = () => reject(open.error);
    open.onsuccess = () => {
      const tx = open.result.transaction("keys", op === "get" ? "readonly" : "readwrite");
      const s = tx.objectStore("keys");
      const r = op === "get" ? s.get("device") : op === "put" ? s.put(value, "device") : s.delete("device");
      r.onsuccess = () => resolve(r.result);
      r.onerror = () => reject(r.error);
    };
  });
}

// ---------------------------------------------------------------- network

async function rpc(method, params = []) {
  const r = await fetch(saved.node, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });
  const j = await r.json();
  if (j.error) throw new Error(j.error.message);
  return j.result;
}
async function read(address) {
  const v = await rpc("state_getStorage", ["0x" + K.hex(address)]);
  return v ? K.fromHex(v) : null;
}
async function service(path, body) {
  const r = await fetch(new URL(path, saved.service), body === undefined ? {} : {
    method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body),
  });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw Object.assign(new Error(j.message ?? `identity service: ${r.status}`), { code: j.error ?? "service" });
  return j;
}

const clean = (url) => String(url).trim().replace(/\/+$/, "");

/**
 * Joins a test network: the node and the identity service must belong to
 * the same chain. Keeps a key and account made before.
 */
export async function connect(node, serviceUrl) {
  const before = saved;
  saved = { ...(saved ?? {}), node: clean(node), service: clean(serviceUrl) + "/" };
  try {
    const genesis = await rpc("chain_getBlockHash", [0]);
    const info = await service("v1/info");
    if (info.audience?.toLowerCase() !== genesis.toLowerCase()) throw new Error("The node and the identity service belong to different networks.");
    if (before?.audience && before.audience !== genesis.toLowerCase()) {
      throw new Error("This device belongs to another test network. Leave it first.");
    }
    saved.audience = genesis.toLowerCase();
    saved.checks = info.checks ?? [];
    keep();
    return info;
  } catch (e) {
    saved = before;
    throw e;
  }
}

const audience = () => K.fromHex(saved.audience);

// ---------------------------------------------------------------- the key

/**
 * Makes this device's money key – in the phone's security chip, else a
 * passkey, else a key the browser keeps (ADR 0024) – and remembers it.
 * `account` is the account it is linked to; absent = the account it opens.
 */
export async function makeKey(name, linkTo = null) {
  let kind = await K.bestKeyKind();
  if (kind === "passkey" && !PASSKEY_SITES.includes(location.hostname)) kind = "ed25519";
  let made;
  if (kind === "hardware") {
    made = { key: (await K.hardwareKey()) ?? (await K.createHardwareKey()).key };
  } else if (kind === "passkey") {
    const p = await K.createPasskey({ rpId: location.hostname, userName: name || "Capita" });
    made = { key: p.key, credentialId: K.hex(p.credentialId), rpId: location.hostname };
  } else {
    const d = await K.createDeviceKey();
    await vault("put", d.privateKey);
    made = { key: d.key };
  }
  const raw = K.keyBytes(made.key);
  saved = {
    ...saved,
    key: { kind: made.key.kind, public: K.hex(made.key.public), raw: K.hex(raw) },
    credentialId: made.credentialId ?? null,
    rpId: made.rpId ?? null,
    account: K.hex(linkTo ?? K.accountOf(made.key)),
    opened: !!linkTo,
    added: { [K.hex(raw)]: Date.now() },
  };
  keep();
  return saved;
}

const myKey = () => ({ kind: saved.key.kind, public: K.fromHex(saved.key.public) });

/** Signs `challenge` with this device's key; phones ask for fingerprint, face or screen lock. */
export async function prove(challenge, title = "Confirm") {
  const kind = saved.key.kind;
  if (kind === "hardware") return K.hardwareProof(challenge, { title });
  if (kind === "passkey") return K.passkeyProof({ rpId: saved.rpId, credentialId: K.fromHex(saved.credentialId), challenge });
  const secret = await vault("get");
  if (!secret) throw new Error("This browser lost its key.");
  return K.deviceKeyProof(secret, challenge);
}

// ---------------------------------------------------------------- reading the chain

export const keysOf = async () => C.decodeKeys(await read(C.at.keys(account())));
export const homeClaim = async () => C.decodeHomeClaim(await read(C.at.homeClaim(account())));
export const credential = async () => C.decodeCredential(await read(C.at.credential(account())));
export async function ownerOf(raw) {
  const v = await read(C.at.keyOwner(raw));
  return v ? K.hex(v) : null;
}
const nonce = async () => C.decodeNonce(await read(C.at.nonce(account())));

/** Asks `check` every 1.5 s (or `every` ms) until it answers something, for `seconds`. */
export async function waitFor(check, seconds = 45, signal, every = 1500) {
  const end = Date.now() + seconds * 1000;
  for (;;) {
    if (signal?.aborted) throw new DOMException("stopped", "AbortError");
    const v = await check().catch(() => null);
    if (v) return v;
    if (Date.now() > end) return null;
    await new Promise((r) => setTimeout(r, every));
  }
}

// ---------------------------------------------------------------- acting

/**
 * Sends `call` for this device's account, signed by its key, without fee
 * (`keys.act`; the first action opens the account with `keys.open`).
 * Answers once the chain has counted the action; whether the call itself
 * did what it should, the caller checks in the chain's state.
 */
export async function act(call, title) {
  const acc = account();
  let opening = !saved.opened && K.hex(acc) === K.hex(K.accountOf(myKey()));
  if (opening && (await ownerOf(keyRaw()))) {
    saved.opened = true; keep(); opening = false;
  }
  const n = opening ? 0 : await nonce();
  const proof = await prove(K.actChallenge(audience(), acc, n, call), title);
  const tx = opening ? K.openCall(myKey(), call, proof) : K.actCall(acc, myKey(), call, n, proof);
  await rpc("author_submitExtrinsic", ["0x" + K.hex(K.bareExtrinsic(tx))]);
  const counted = await waitFor(async () => (await nonce()) > n);
  if (!counted) throw new Error("The network did not take it. Try again.");
  if (opening) { saved.opened = true; keep(); }
}

/** Opens the account on the ledger (the start grant), if it is not open. */
export async function ensureOpen() {
  if (await read(C.at.capitaAccount(account()))) { if (!saved.opened) { saved.opened = true; keep(); } return; }
  await act(C.openAccount(), "Open your account");
  if (!(await read(C.at.capitaAccount(account())))) throw new Error("The account was not opened.");
}

/** Presents an identity service's claim (`credential.present` or `requestMove`). */
export async function present(answer) {
  await ensureOpen();
  await act(C.claimCall(answer), "Confirm your verification");
  if (answer.call === "credential.present") {
    const held = await credential();
    if (!held || held.issuedAt !== answer.claim.issuedAt) throw new Error("The network did not accept the verification.");
  }
}

// ---------------------------------------------------------------- money (ADR 0027)

/**
 * The account's money as the ledger has it: { cents, account, rules } –
 * `account` null while the ledger does not know it (app/chain/calls.js).
 */
export async function money() {
  const acc = account();
  const [balance, state, rules] = await Promise.all([read(C.at.balance(acc)), read(C.at.capitaAccount(acc)), read(C.at.rules())]);
  return { cents: C.decodeBalance(balance), account: C.decodeAccount(state), rules: C.decodeRules(rules) };
}

/** Whether Capita can be paid to `toHex`: the ledger knows the account. */
export const canReceive = async (toHex) => !!(await read(C.at.capitaAccount(K.fromHex(toHex))));

// A ledger operation of the account itself – a payment made, Capita minted –
// counts up `moves`; a payment received does not. So this tells whether the
// own call did something, whatever else happened in the same block.
async function moved(call, title) {
  const before = await money();
  await act(call, title);
  const after = await money();
  return after.account && after.account.moves > (before.account?.moves ?? 0) ? after : null;
}

/** Pays `cents` to the account `toHex`, signed by this device. Answers the money after. */
export async function pay(toHex, cents, title) {
  const after = await moved(C.transfer(K.fromHex(toHex), cents), title);
  if (!after) throw new Error("The network did not make the payment.");
  return after;
}

/** Claims what fell due (birthdays). Answers the money after, or null if nothing came. */
export const claimDue = (title) => moved(C.claim(), title);

// ---------------------------------------------------------------- verification

/** The account to verify and, in helper mode, this device's own account. */
export const checkFor = (target) => ({
  service: saved.service,
  account: target,
  helper: K.hex(target) === saved.account ? null : saved.account,
});

// ---------------------------------------------------------------- the hand-over box (ADR 0025)

export const newBox = () => K.hex(crypto.getRandomValues(new Uint8Array(16)));
export const put = (box, slot, value) => service("v1/relay/put", { box, slot, data: JSON.stringify(value) });
export async function take(box, slot) {
  const { data } = await service("v1/relay/take", { box, slot });
  return data ? JSON.parse(data) : null;
}

/** QR words. The service is named only to compare: each device uses its own. */
export const verifyCode = (box, name) =>
  `capita:verify?b=${box}&a=${saved.account}&s=${encodeURIComponent(saved.service)}&n=${encodeURIComponent(name ?? "")}`;
export const linkCode = (box) => `capita:link?b=${box}&s=${encodeURIComponent(saved.service)}`;
/** The code a payer scans: this account, the name to show, and the amount asked for, if any. */
export const payCode = (name, cents = null) =>
  `capita:pay?a=${saved.account}&s=${encodeURIComponent(saved.service)}&n=${encodeURIComponent(name ?? "")}${cents ? `&c=${cents}` : ""}`;

/** Reads a scanned code: { what: "verify"|"link"|"pay"|"demo", ... } or null. */
export function readCode(text) {
  let u;
  try { u = new URL(text); } catch { return null; }
  if (u.protocol !== "capita:") return null;
  const q = u.searchParams;
  const what = u.pathname.replace(/^\/+/, "").split("/")[0];
  const box = q.get("b");
  const sameService = !q.get("s") || !saved?.service || clean(q.get("s")) === clean(saved.service);
  if (what === "verify" && /^[0-9a-f]{32}$/.test(box) && /^[0-9a-f]{64}$/.test(q.get("a") ?? "")) {
    return { what, box, account: q.get("a"), name: q.get("n") ?? "", sameService };
  }
  if (what === "link" && /^[0-9a-f]{32}$/.test(box)) return { what, box, sameService };
  if (what === "pay" && /^[0-9a-f]{64}$/.test(q.get("a") ?? "")) {
    const cents = /^[1-9]\d{0,8}$/.test(q.get("c") ?? "") ? Number(q.get("c")) : null;
    return { what, account: q.get("a"), name: q.get("n") ?? "", cents, sameService };
  }
  if (what.startsWith("demo")) return { what: "demo", sameService: true }; // the unconnected demo's code
  return null;
}

/** New device: signs that its key may join `acc` and hands key and signature over. */
export async function handKey(box, acc) {
  const raw = keyRaw();
  const possession = await prove(K.addChallenge(audience(), K.fromHex(acc), myKey()), "Join your account");
  await put(box, "key", { key: K.hex(raw), possession: K.hex(K.proofBytes(possession)) });
}

/** The phone: links the handed-over key to this account. */
export async function addKey(handed) {
  const raw = K.fromHex(handed.key);
  await act(C.addKeyRaw(raw, K.fromHex(handed.possession)), "Add the device");
  const added = (await keysOf()).some((k) => K.hex(k.raw) === handed.key);
  if (!added) throw new Error("The network did not add the device.");
  saved.added = { ...saved.added, [handed.key]: Date.now() };
  keep();
}

/** New device: it now acts for `acc`. */
export function joined(acc) {
  saved.account = acc;
  saved.opened = true;
  keep();
}

export async function removeKey(raw) {
  await act(C.removeKeyRaw(raw), "Remove the device");
  return !(await keysOf()).some((k) => K.hex(k.raw) === K.hex(raw));
}
export const askHome = () => act(C.claimHome(), "Make this the main device");
export const takeHomeNow = () => act(C.takeHome(), "Become the main device");
export const keepHome = () => act(C.refuseHome(), "Keep this phone as main");
