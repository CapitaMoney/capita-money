# ADR 0021 – Ways to verify; the document check

Status: accepted for the alpha (owner 2026-11-14: all ways offered, none mandatory). Builds on rule 18 (document check first: chip + selfie), ADR 0013 (person key), ADR 0014 (identity service), ADR 0017 (face check). Concept: [app-full.md](../app-full.md).

## Context
One person, one account: an identity service must establish name and date of birth (the person key, rule 17) and that the person is present. People differ in what they have: a passport, an ID card, a phone with NFC, a laptop, an electronic ID of their state, an address that receives mail – or little of this. The owner: take every way a country offers, make none mandatory, "else people in countries without eID are left out".

## Decision
Each way is an **adapter** of the identity service. All end in the same claim (ADR 0014); the claim states the kind of proof (`proof`), never the data.

| Adapter | Proof kind | Alpha |
|---|---|---|
| `chip` – chip of a passport or ID card + filmed head movements | `document-chip` | built (this ADR) |
| `chip` through a helper's phone or from a laptop (QR hand-over) | `document-chip` + `via-helper` | same adapter; ADR 0022 |
| `eid-de` – German eID (card + PIN), through the AusweisApp SDK and an eID service | `eid` | after the e.V. (authorisation certificate) |
| `eudi` – EU wallet, person data (PID) by OpenID4VP | `eudi-pid` | sandbox test; live with the German wallet (about 2027-01-02) and registration (German seat) |
| `eid-ee` and other national eIDs | `eid` | after the e.V., one by one |
| `letter` – Challenger letter to a postal address | `letter` | exists (M3b) |

### The document check (`chip`)
1. The app asks the service for a session: a random challenge of head movements (ADR 0017) and an 8-byte nonce.
2. The phone reads the chip: access by PACE (or BAC on old passports) with the key printed on the document – document number, date of birth, expiry from the machine-readable zone, or the 6-digit card access number. It reads EF.SOD, DG1 (the machine-readable zone), DG2 (the photo), and DG14/DG15 where present.
3. The app films the head movements and sends data groups and frames to the service (TLS, nothing stored on the phone).
4. The service checks:
   - **passive authentication**: the hashes of DG1, DG2 in the security object (EF.SOD) match; the security object is signed by the document signer certificate it carries; that certificate is signed by a country signing certificate (CSCA) of the trust list. Trust list: the ICAO master list as the German BSI publishes it (a file the operator renews), or own CSCA files.
   - DG1: check digits; name, date of birth, nationality, document number, expiry. Expired documents are refused; age below `identity.minimum_age` is refused.
   - DG2: the photo (JPEG or JPEG 2000) against the filmed face, with the liveness challenge (ADR 0017).
   - Active authentication with the nonce where DG15 exists: Offene Frage, after the alpha (proves the chip itself was there, not a copy of its data; the live face already binds the data to the person present).
5. Person key from name + date of birth (+ place of birth where the document states it, ADR 0013); the claim is signed. Pictures, frames and data groups are dropped at the end of the call.

### The German Personalausweis
Its chip photo is readable by authorities only ([fimportal.de](https://fimportal.de/api/v0/xzufi-services/L100042/30611/de/pdf): service providers cannot read the biometric data). It verifies through `eid-de` or `eudi` – both prove the holder by card and PIN, so no selfie is needed. Until then the app says, when it meets a German ID card: "This card can verify you once the online ID is connected. A passport works now."

## Consequences
+ Nobody is left out by one missing piece: no NFC → helper or laptop hand-over; no document chip → letter; a state eID → no selfie.
+ The chain sees only claims: which way was used stays at the service (and in the claim's proof kind).
− The trust list is a file the operator must keep fresh; an old list refuses new documents.
− JPEG 2000 photos need a decoder in the service (OpenJPEG).
− National eIDs and the EU wallet need a legal body (e.V.) for certificates and registration – not a software question.
- Offene Frage: minimum age (rules C).
- Vermutung (Claude, ungeprüft): ID cards of several EU states (not Germany) let the chip photo be read with the CAN.
