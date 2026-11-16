// SPDX-License-Identifier: AGPL-3.0-only
// A play network for the money screens (ADR 0027): the app, a stand-in for
// the node and a stand-in for the identity service, on one address. For
// trying the screens on a computer that has no chain – NOT a chain: it
// checks no signature and keeps everything in memory. The values it serves
// are laid out as the runtime lays them out (the rules are the bytes of
// `state-vectors.json`); what the real chain does is tested in
// `chain-smoke.mjs`.
//
//   node tests/play-network.mjs [port]      (from app/, default port 8000)
//
// Then open the address it prints. Besides the app:
//   /play/verify?born=1990-05-17&ago=3   the newest account is verified, as if `ago` days ago
//   /play/pay?cents=250                  somebody pays the newest account
//   /play/code                           the payment code of "Ben", an account that can be paid

import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { readFileSync } from "node:fs";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
import * as K from "../chain/keys.js";
import * as C from "../chain/calls.js";
import * as M from "../money.js";
import { ageAt } from "../logic.js";

const port = Number(process.argv[2] ?? 8000);
const root = join(fileURLToPath(new URL(".", import.meta.url)), "..");
const origin = `http://localhost:${port}`;
const GENESIS = "0x" + "c0".repeat(32);
const rulesBytes = K.fromHex(JSON.parse(readFileSync(new URL("./state-vectors.json", import.meta.url), "utf8")).rules.bytes);
const rules = C.decodeRules(rulesBytes);
const secs = () => Math.floor(Date.now() / 1000);

// ---------------------------------------------------------------- the ledger, in memory
const accounts = new Map(); // account hex -> state
const storage = new Map(); // address hex -> () => bytes or null
let newest = null;

const u64 = (n) => { const b = new Uint8Array(8); new DataView(b.buffer).setBigUint64(0, BigInt(n), true); return b; };
const u32 = (n) => { const b = new Uint8Array(4); new DataView(b.buffer).setUint32(0, n, true); return b; };
const zero = (n) => new Uint8Array(n);
const some = (bytes) => (bytes ? K.concat(Uint8Array.of(1), bytes) : Uint8Array.of(0));

/** `AccountState` as the runtime stores it (read by `decodeAccount`). */
function accountBytes(a) {
  const [y, m, d] = a.born ? a.born.split("-").map(Number) : [];
  return K.concat(
    a.id, some(a.person ? zero(32) : null), Uint8Array.of(0), u64(a.openedAt), zero(2 + 8),
    Uint8Array.of(a.level), some(a.levelUntil === null ? null : u64(a.levelUntil)), Uint8Array.of(0),
    some(a.born ? Uint8Array.of(y & 255, y >> 8, m, d) : null), Uint8Array.of(a.firstPaid ? 1 : 0), zero(16 + 8),
    u64(a.reserved), zero(8 + 16 + 8), u64(a.paidUntil), u64(a.paidUntil), zero(8 + 8), zero(8 + 8), Uint8Array.of(0),
    zero(8 + 8 + 8), u64(a.moves), zero(8),
  );
}
const viewOf = (a) => ({ cents: a.cents, account: C.decodeAccount(accountBytes(a)), rules });

function open(id, keyRaw = null) {
  const hex = K.hex(id);
  if (accounts.has(hex)) return accounts.get(hex);
  const grant = M.rulesAt(rules, secs()).startGrantCents;
  const a = { id, cents: grant, reserved: grant, person: false, level: 0, levelUntil: null, born: null, firstPaid: false,
    openedAt: secs(), paidUntil: secs(), moves: 1, nonce: 0 };
  accounts.set(hex, a);
  storage.set(K.hex(C.at.capitaAccount(id)), () => accountBytes(a));
  storage.set(K.hex(C.at.balance(id)), () => u64(a.cents));
  storage.set(K.hex(C.at.nonce(id)), () => u32(a.nonce));
  if (keyRaw) {
    storage.set(K.hex(C.at.keys(id)), () => K.concat(Uint8Array.of(4), keyRaw));
    storage.set(K.hex(C.at.keyOwner(keyRaw)), () => id);
    newest = a;
  }
  return a;
}
storage.set(K.hex(C.at.rules()), () => rulesBytes);
const ben = open(Uint8Array.from({ length: 32 }, () => 0x42));

/** One transaction: `keys.open` or `keys.act` around a call of the money screens. */
function submit(bytes) {
  let i = [1, 2, 4][bytes[0] & 3] ?? 1; // the compact length
  if (bytes[i++] !== 4 || bytes[i++] !== C.PALLETS.keys) throw new Error("not a call this play network knows");
  const kind = bytes[i++];
  let id = null;
  if (kind === K.KEYS.act) { id = bytes.slice(i, i + 32); i += 32; }
  const keyRaw = bytes.slice(i, i + (bytes[i] === 0 ? 33 : 34));
  i += keyRaw.length;
  const a = id ? accounts.get(K.hex(id)) : open(K.accountOf({ kind: ["ed25519", "passkey", "hardware"][keyRaw[0]], public: keyRaw.slice(1) }), keyRaw);
  if (!a) throw new Error("unknown account");
  a.nonce++; // the first action (`keys.open`) counts too
  const [pallet, call] = [bytes[i], bytes[i + 1]];
  if (pallet !== C.PALLETS.capita) return;
  if (call === C.CAPITA.claim) {
    const due = M.claimableCents(viewOf(a), Date.now());
    if (due > 0) { a.cents += due; a.moves++; }
    a.paidUntil = secs();
  }
  if (call === C.CAPITA.transfer) {
    const to = accounts.get(K.hex(bytes.slice(i + 2, i + 34)));
    const cents = Number(new DataView(bytes.buffer, bytes.byteOffset + i + 34, 8).getBigUint64(0, true));
    if (!to || cents <= 0 || cents > a.cents - a.reserved) return; // refused: nothing moves
    a.cents -= cents; a.moves++;
    to.cents += cents;
  }
}

// ---------------------------------------------------------------- the play buttons
function play(path, q) {
  if (path === "code") return `capita:pay?a=${K.hex(ben.id)}&s=${encodeURIComponent(origin + "/svc/")}&n=Ben`;
  if (!newest) return "Open the app first and give a name: there is no account yet.";
  if (path === "pay") {
    const cents = Number(q.get("cents") ?? 250);
    newest.cents += cents;
    return `Paid ${cents} cents to the newest account.`;
  }
  if (path === "verify") {
    const born = q.get("born") ?? "1990-05-17";
    const at = secs() - Number(q.get("ago") ?? 0) * 86400;
    const first = newest.firstPaid ? 0 : ageAt(born, at * 1000) * M.rulesAt(rules, at).age.atVerificationCents;
    Object.assign(newest, { person: true, level: 1, levelUntil: at + 365 * 86400, born, firstPaid: true, paidUntil: at });
    newest.cents += first; newest.moves++;
    return `Verified, born ${born}, ${q.get("ago") ?? 0} days ago; ${first} cents by age.`;
  }
  return null;
}

// ---------------------------------------------------------------- one address for everything
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css", ".json": "application/json",
  ".webmanifest": "application/manifest+json", ".svg": "image/svg+xml", ".png": "image/png", ".woff2": "font/woff2" };
const body = (req) => new Promise((done) => { let s = ""; req.on("data", (c) => (s += c)); req.on("end", () => done(s)); });

createServer(async (req, res) => {
  const url = new URL(req.url, origin);
  const send = (status, type, text) => { res.writeHead(status, { "content-type": type, "cache-control": "no-store" }); res.end(text); };
  const json = (status, value) => send(status, "application/json", JSON.stringify(value));
  try {
    if (url.pathname === "/rpc" && req.method === "POST") {
      const { id, method, params } = JSON.parse(await body(req));
      const answer = (result) => json(200, { jsonrpc: "2.0", id, result });
      if (method === "chain_getBlockHash") return answer(GENESIS);
      if (method === "state_getStorage") {
        const v = storage.get(params[0].slice(2))?.();
        return answer(v ? "0x" + K.hex(v) : null);
      }
      if (method === "author_submitExtrinsic") { submit(K.fromHex(params[0])); return answer("0x" + "00".repeat(32)); }
      return json(200, { jsonrpc: "2.0", id, error: { code: -32601, message: "The play network does not know this." } });
    }
    if (url.pathname === "/svc/v1/info") return json(200, { audience: GENESIS, checks: [] });
    if (url.pathname.startsWith("/svc/")) return json(404, { error: "play", message: "The play network verifies nobody: use /play/verify." });
    if (url.pathname.startsWith("/play/")) {
      const said = play(url.pathname.slice(6), url.searchParams);
      return said ? send(200, "text/plain; charset=utf-8", said + "\n") : send(404, "text/plain", "unknown\n");
    }
    const file = normalize(join(root, url.pathname === "/" ? "index.html" : decodeURIComponent(url.pathname)));
    if (!file.startsWith(root)) return send(403, "text/plain", "no\n");
    return send(200, TYPES[extname(file)] ?? "application/octet-stream", await readFile(file));
  } catch (e) {
    return send(e?.code === "ENOENT" ? 404 : 400, "text/plain", `${e?.message ?? e}\n`);
  }
}).listen(port, "127.0.0.1", () => {
  console.log(`Play network (no chain, no signature checked): ${origin}/?node=${origin}/rpc&service=${origin}/svc`);
  console.log(`Verify the newest account: ${origin}/play/verify?born=1990-05-17 · pay it: ${origin}/play/pay?cents=250`);
});
