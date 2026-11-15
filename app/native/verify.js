// SPDX-License-Identifier: AGPL-3.0-only
// The document check from the app's side (ADR 0021): ask the identity
// service for a challenge, read the chip, film the movements, send both.
// The answer is the claim as the call the app sends to the chain
// (`credential.present`), or a refusal in plain words.

import { readChip } from "./chip.js";
import { filmMoves } from "./film.js";
import { preparePhone, requestDigest, unb64 } from "./phone.js";

async function post(url, body) {
  const r = await fetch(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) {
    const e = new Error(j.message ?? `identity service: ${r.status}`);
    e.code = j.error ?? "service";
    throw e;
  }
  return j;
}

const hex = (b) => Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");

/**
 * `POST /v1/document/start`. Answers { session, nonce, chipNonce, validSeconds } (nonces in base64).
 * `helper`: this phone's own account (hex) when it checks somebody else (ADR 0022).
 */
export const documentStart = (service, account, ticket, helper) =>
  post(new URL("/v1/document/start", service), {
    account: hex(account), ...(ticket ? { ticket } : {}), ...(helper ? { helper } : {}),
  });

/** `POST /v1/document/moves`: the head movements, valid one minute from now. */
export const documentMoves = (service, session) => post(new URL("/v1/document/moves", service), { session });

/** `POST /v1/document/finish` with chip data (base64), pictures (data addresses) and the phone check. */
export const documentFinish = (service, session, chip, frames, phone) =>
  post(new URL("/v1/document/finish", service), {
    session, sod: chip.sod, dg1: chip.dg1, dg2: chip.dg2,
    ...(chip.dg14 ? { dg14: chip.dg14 } : {}), ...(chip.dg15 ? { dg15: chip.dg15 } : {}),
    ...(chip.active ? { active: chip.active } : {}),
    chipAuthenticated: !!chip.chipAuthenticated,
    frames, phone,
  });

const opt = (text) => (text ? unb64(text) : undefined);

/**
 * The whole check on one phone (ADR 0024): start (nonces), the phone's
 * session key, the chip, the movements, the film, the signed finish.
 * `say` shows requests on the screen.
 */
export async function verifyWithDocument({ service, account, ticket, helper, key, video, say, camera }) {
  const { session, nonce, chipNonce } = await documentStart(service, account, ticket, helper);
  const phoneNonce = unb64(nonce);
  const phone = await preparePhone(phoneNonce);
  say("Hold your document to the back of the phone", "chip");
  const chip = await readChip(key, chipNonce);
  // The movements are asked for only now: they hold one minute, holding a
  // document may take longer (capita-face, Settings::valid_for).
  const { moves } = await documentMoves(service, session);
  await camera?.();
  const frames = await filmMoves({ video, moves, say });
  say("Checking …", "wait");
  const digest = await requestDigest(phoneNonce, session, {
    sod: unb64(chip.sod), dg1: unb64(chip.dg1), dg2: unb64(chip.dg2),
    dg14: opt(chip.dg14), dg15: opt(chip.dg15), active: opt(chip.active),
    chipAuthenticated: !!chip.chipAuthenticated,
    frames: frames.map(unb64),
  });
  return documentFinish(service, session, chip, frames, await phone.prove(digest));
}
