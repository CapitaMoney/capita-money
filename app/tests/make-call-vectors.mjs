// SPDX-License-Identifier: AGPL-3.0-only
// Writes app/tests/call-vectors.json: the calls and storage addresses of
// app/chain/calls.js for fixed inputs. runtime/src/tests.rs decodes the
// same bytes with the runtime's own types; chain-read.test.mjs checks the
// file is what this script makes. Run: node app/tests/make-call-vectors.mjs
import { writeFileSync } from "node:fs";
import * as C from "../chain/calls.js";
import { hex } from "../chain/keys.js";

const h = (b) => "0x" + hex(b);
const fill = (n, v) => "0x" + v.repeat(n);
export function vectors() {
  const claim = {
    issuer: 7, serial: fill(16, "11"), subject: fill(32, "22"), subjectRef: fill(32, "33"),
    replaces: null, level: 1, residence: null, born: "1990-05-17", attributes: [], issuedAt: 1800000000, expiresAt: 1831536000,
  };
  const present = { call: "credential.present", from: null, claim, signature: fill(64, "44") };
  const moved = { call: "credential.requestMove", from: fill(32, "55"), claim: { ...claim, replaces: fill(32, "66"), born: null, expiresAt: null }, signature: fill(64, "77") };
  const account = Uint8Array.from({ length: 32 }, (_, i) => i);
  const hardware = Uint8Array.of(2, ...Array.from({ length: 33 }, (_, i) => (i === 0 ? 3 : i)));
  const ed = Uint8Array.of(0, ...new Uint8Array(32).fill(9));
  const edProof = Uint8Array.of(0, ...new Uint8Array(64).fill(8));
  return {
    account: h(account),
    calls: {
      open: h(C.openAccount()),
      claim: h(C.claim()),
      transfer: { to: h(account), cents: 1250, call: h(C.transfer(account, 1250)) },
      present: { answer: present, call: h(C.claimCall(present)) },
      requestMove: { answer: moved, call: h(C.claimCall(moved)) },
      addKey: { key: h(ed), possession: h(edProof), call: h(C.addKeyRaw(ed, edProof)) },
      removeKey: { key: h(hardware), call: h(C.removeKeyRaw(hardware)) },
      claimHome: h(C.claimHome()),
      takeHome: h(C.takeHome()),
      refuseHome: h(C.refuseHome()),
    },
    storage: {
      keys: h(C.at.keys(account)),
      nonce: h(C.at.nonce(account)),
      homeClaim: h(C.at.homeClaim(account)),
      keyOwner: h(C.at.keyOwner(hardware)),
      credential: h(C.at.credential(account)),
      capitaAccount: h(C.at.capitaAccount(account)),
      balance: h(C.at.balance(account)),
      movedTo: h(C.at.movedTo(account)),
      rules: h(C.at.rules()),
    },
  };
}
if (import.meta.url === `file://${process.argv[1]}`) {
  writeFileSync(new URL("./call-vectors.json", import.meta.url), JSON.stringify(vectors(), null, 2) + "\n");
  console.log("written app/tests/call-vectors.json");
}
