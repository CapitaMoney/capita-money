// SPDX-License-Identifier: AGPL-3.0-only
// The document check from the app's side (ADR 0021): ask the identity
// service for a challenge, read the chip, film the movements, send both.
// The answer is the claim as the call the app sends to the chain
// (`credential.present`), or a refusal in plain words.

import { readChip } from "./chip.js";
import { filmMoves } from "./film.js";

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

/** `POST /v1/document/start`. Answers { session, moves, validSeconds }. */
export const documentStart = (service, account, ticket) =>
  post(new URL("/v1/document/start", service), { account: hex(account), ...(ticket ? { ticket } : {}) });

/** `POST /v1/document/finish` with chip data (base64) and pictures (data addresses). */
export const documentFinish = (service, session, chip, frames) =>
  post(new URL("/v1/document/finish", service), {
    session, sod: chip.sod, dg1: chip.dg1, dg2: chip.dg2,
    ...(chip.dg14 ? { dg14: chip.dg14 } : {}), ...(chip.dg15 ? { dg15: chip.dg15 } : {}),
    frames,
  });

/**
 * The whole check on one device: start, chip (key = what is printed on the
 * document, or the CAN), film, finish. `say` shows requests on the screen.
 */
export async function verifyWithDocument({ service, account, ticket, key, video, say }) {
  // The chip first: the movements must be filmed within a minute of the
  // challenge (capita-face, Settings::valid_for), holding a document may take longer.
  say("Hold your document to the back of the phone", "chip");
  const chip = await readChip(key);
  const { session, moves } = await documentStart(service, account, ticket);
  const frames = await filmMoves({ video, moves, say });
  say("Checking …", "wait");
  return documentFinish(service, session, chip, frames);
}
