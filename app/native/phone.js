// SPDX-License-Identifier: AGPL-3.0-only
// The phone check of a verification (docs/adr/0024-hardware-keys-checked-phones.md).
//
// The shell makes a fresh key for the session's nonce – Android: a Keystore
// key whose attestation challenge is the nonce; iPhone: an App Attest key
// Apple certifies for SHA-256(nonce) – and at the end that key signs the
// request digest. Browsers cannot do this: a laptop hands the check to a phone.

const plugin = () => globalThis.Capacitor?.Plugins?.CapitaPhone;
const platform = () => globalThis.Capacitor?.getPlatform?.() ?? "web";

export const b64 = (bytes) => {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s);
};
export const unb64 = (text) => {
  const data = text.includes("base64,") ? text.slice(text.indexOf("base64,") + 7) : text;
  return Uint8Array.from(atob(data.trim()), (c) => c.charCodeAt(0));
};

/** Whether this device can take part in a document check. */
export const phoneCheckPossible = () => !!plugin() && ["android", "ios"].includes(platform());

/**
 * Makes the session's key for `nonce` (32 bytes). Answers `prove(digest)`,
 * which signs the request digest and answers the `phone` part of
 * `POST /v1/document/finish`.
 */
export async function preparePhone(nonce) {
  const p = plugin();
  if (!p) throw new Error("This device cannot do the phone check.");
  if (platform() === "ios") {
    const { keyId, attestation } = await p.attest({ nonce: b64(nonce) });
    return {
      prove: async (digest) => {
        const { assertion } = await p.assert({ keyId, digest: b64(digest) });
        return { kind: "apple", keyId, attestation, assertion };
      },
    };
  }
  const { chain } = await p.sessionKey({ nonce: b64(nonce) });
  return {
    prove: async (digest) => {
      const { signature } = await p.signSession({ digest: b64(digest) });
      return { kind: "android", chain, signature };
    },
  };
}

const enc = new TextEncoder();
const sha256 = async (b) => new Uint8Array(await crypto.subtle.digest("SHA-256", b));

/**
 * What the phone's key signs – the same bytes as capita_issuer::phone::request_digest:
 * SHA-256("capita/document/v1" ‖ nonce ‖ SHA-256(session) ‖ SHA-256 of sod, dg1,
 * dg2, dg14, dg15, active (absent = empty) ‖ chip authenticated (1 byte) ‖
 * frame count (4 bytes big-endian) ‖ SHA-256 of each frame).
 */
export async function requestDigest(nonce, session, parts) {
  const pieces = [enc.encode("capita/document/v1"), nonce, await sha256(enc.encode(session))];
  for (const f of [parts.sod, parts.dg1, parts.dg2, parts.dg14, parts.dg15, parts.active]) {
    pieces.push(await sha256(f ?? new Uint8Array(0)));
  }
  pieces.push(Uint8Array.of(parts.chipAuthenticated ? 1 : 0));
  const n = parts.frames.length;
  pieces.push(Uint8Array.of(n >>> 24, (n >>> 16) & 255, (n >>> 8) & 255, n & 255));
  for (const frame of parts.frames) pieces.push(await sha256(frame));
  const all = new Uint8Array(pieces.reduce((s, p) => s + p.length, 0));
  let i = 0;
  for (const p of pieces) { all.set(p, i); i += p.length; }
  return sha256(all);
}
