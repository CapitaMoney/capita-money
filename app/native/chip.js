// SPDX-License-Identifier: AGPL-3.0-only
// The document chip, as far as the app sees it (ADR 0021, ADR 0023).
// In the Android shell the plugin CapitaChip reads it; in a browser there is
// no chip reader – the app then offers the hand-over to a phone (QR code).
// Nothing read here is shown or kept: the bytes go to the identity service.

const plugin = () => globalThis.Capacitor?.Plugins?.CapitaChip;

/** Whether this device can read document chips: "yes", "off" (NFC switched off) or "no". */
export async function chipReader() {
  const p = plugin();
  if (!p) return "no";
  const s = await p.status();
  if (!s.nfc) return "no";
  return s.enabled ? "yes" : "off";
}

/** YYMMDD from a date "YYYY-MM-DD" (as a date field gives it). */
export const yymmdd = (iso) => iso.slice(2, 4) + iso.slice(5, 7) + iso.slice(8, 10);

/**
 * Waits for the document at the back of the phone and reads it.
 * key: { documentNumber, birthDate: "YYYY-MM-DD", expiryDate: "YYYY-MM-DD" } or { can: "123456" }.
 * Answers { sod, dg1, dg2, dg14?, dg15?, pace } in base64.
 */
export async function readChip(key) {
  const p = plugin();
  if (!p) throw new Error("no chip reader");
  const arg = key.can
    ? { can: key.can }
    : { documentNumber: key.documentNumber.toUpperCase().replace(/[^A-Z0-9]/g, ""), birthDate: yymmdd(key.birthDate), expiryDate: yymmdd(key.expiryDate) };
  return p.read(arg);
}

/** Stops waiting for a document. */
export async function cancelChip() {
  await plugin()?.cancel();
}
