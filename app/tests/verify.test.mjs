// SPDX-License-Identifier: AGPL-3.0-only
// The request digest the phone signs is the identity service's
// (services/issuer/tests/phone.rs checks the same vector), and the finish
// request carries what the service reads.

import { test } from "node:test";
import assert from "node:assert/strict";
import { requestDigest, b64, unb64 } from "../native/phone.js";
import { documentFinish } from "../native/verify.js";

const enc = new TextEncoder();
const hex = (b) => Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");

test("the request digest is the identity service's", async () => {
  const nonce = Uint8Array.from({ length: 32 }, (_, i) => i + 1);
  const d = await requestDigest(nonce, "0123456789abcdef0123456789abcdef", {
    sod: enc.encode("sod"), dg1: enc.encode("dg1"), dg2: enc.encode("dg2"),
    dg14: undefined, dg15: enc.encode("dg15"), active: enc.encode("aa"),
    chipAuthenticated: true, frames: [enc.encode("f1"), enc.encode("f2")],
  });
  assert.equal(hex(d), "750324a761902b2b4af429a920946d9a85cd61d5dda2584cbd98968e30f825b0");
});

test("base64 both ways, data addresses too", () => {
  const b = Uint8Array.of(0, 1, 2, 250, 251, 255);
  assert.deepEqual(unb64(b64(b)), b);
  assert.deepEqual(unb64("data:image/jpeg;base64," + b64(b)), b);
});

test("the finish request carries chip proof and phone check", async () => {
  let sent;
  globalThis.fetch = async (url, init) => {
    sent = { url: String(url), body: JSON.parse(init.body) };
    return { ok: true, json: async () => ({ call: "0x00" }) };
  };
  const chip = { sod: "c29k", dg1: "ZGcx", dg2: "ZGcy", dg15: "ZGcxNQ==", active: "YWE=", chipAuthenticated: true };
  const phone = { kind: "android", chain: ["AAA=", "BBB="], signature: "CCC=" };
  await documentFinish("http://127.0.0.1:8787", "s1", chip, ["data:image/jpeg;base64,AA=="], phone);
  assert.equal(sent.url, "http://127.0.0.1:8787/v1/document/finish");
  assert.deepEqual(Object.keys(sent.body).sort(), ["active", "chipAuthenticated", "dg1", "dg15", "dg2", "frames", "phone", "session", "sod"]);
  assert.equal(sent.body.chipAuthenticated, true);
  assert.deepEqual(sent.body.phone, phone);
});
