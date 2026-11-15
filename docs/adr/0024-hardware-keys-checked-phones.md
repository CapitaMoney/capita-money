# ADR 0024 – Hardware keys in the phone apps; verification only from a checked phone

Status: accepted for the alpha (owner 2026-11-15: "hardware key + check, laptop passkey"; phone check "required, no Google service"; "always the best solution, regardless of the work"). Changes [ADR 0020](0020-keys-passkeys-linked-devices.md) (which key a phone uses), [ADR 0021](0021-verification-ways.md) (laptop hand-over, proof that the chip was there), [ADR 0023](0023-app-shells-and-hosting.md) (phone check). Concept: [app-full.md](../app-full.md).

⚠ Widerspruch, settled: on 2026-11-14 the owner chose "passkey + fallback" and a phone check "when available, never a condition" (ADRs 0020, 0023). On 2026-11-15 the owner chose, after an explanation of both, the hardware key and the required check. This ADR follows the later decision; passkeys stay for browsers.

## Context
- A **synced passkey** (ADR 0020) lives in the person's Google or Apple account: whoever takes that account over – a phishing mail, a stolen SIM card – can spend the person's Capita. The key is also stored in their cloud, against rule 15 ("the key itself is stored nowhere").
- An **optional phone check** (ADR 0023) leaves the door open that ADR 0017 names: a changed app feeds a film made for the challenge (deepfake) instead of the camera. One person could then hold many accounts – the end of "one person, one account".
- Play Integrity, the phone check of ADR 0023, needs Google's services on the phone; phones without them (F-Droid users, de-Googled phones) could never pass.

## Decision

### The key of a phone app is a hardware key
- The Android and iPhone apps make the money key **inside the phone's security chip**: Android Keystore (StrongBox where the phone has one, else the trusted environment), Apple Secure Enclave. Curve P-256, signing only, SHA-256. The key cannot be read out, copied, backed up or synced – not by the person, the app, us, the phone maker or malware. Each use asks for fingerprint, face or screen lock.
  - Android: `KeyGenParameterSpec` with `PURPOSE_SIGN`, `DIGEST_SHA256`, `setIsStrongBoxBacked` (falls back to the trusted environment), `setUserAuthenticationRequired(true)` with `AUTH_BIOMETRIC_STRONG | AUTH_DEVICE_CREDENTIAL` for every use, not invalidated by a new fingerprint (else adding a finger would lose the account).
  - iPhone: `SecKeyCreateRandomKey` with `kSecAttrTokenIDSecureEnclave`, access control `privateKeyUsage` + `userPresence`, `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`.
- On chain the key is a new kind, `DeviceKey::Hardware` (SCALE index 2, compressed P-256 point); its proof `Proof::Hardware` is the ECDSA signature over the 32 bytes of the challenge, as `r ‖ s` (`capita_keys::check::verify_hardware`). Account, challenges and the rest of ADR 0020 are unchanged: `2 ‖ key` is the key's byte form, the account is `BLAKE2b-256("capita/account/v1" ‖ 2 ‖ key)`.
- Only the compressed form of a P-256 point counts (first byte 2 or 3), for passkeys too: the library also reads a "compact" form of the same point, which would let one key stand under two names.
- **Browsers and laptops** keep the keys of ADR 0020: a passkey, else a key the browser keeps. A laptop is a **linked device**: it can pay, it cannot remove the phone's key (ADR 0020: only the home key removes others). A person who starts in a browser (an iPhone person until the iPhone app is published) starts with a passkey; installing the app later links its hardware key, which may become the home key after seven days (`claim_home`).
- Lost phone: a linked laptop becomes the home device after seven days; without one, verify again and the account moves to the new phone's key (ADR 0013, rule 15). Uninstalling the app deletes its key – the app says so in Settings: "Removing Capita from this phone removes its key."

### A verification counts only from a checked phone
A document check (`document-chip`, also through a helper, ADR 0022) counts only if the identity service has checked **the phone that read the chip and filmed the face**: a real phone, its system unchanged, running the Capita app. No Google service is asked by the phone.

- **Android – key attestation.** For each verification the app makes a fresh, short-lived *session key* in the Keystore with the service's nonce as attestation challenge. Android answers with a certificate chain from the phone's security chip up to Google's hardware attestation root. The service checks, offline:
  - the chain up to a root on its list (Google publishes the roots); no certificate on Google's revocation list (the service fetches the list, the phone does not);
  - attestation security level *trusted environment* or *StrongBox*, not software;
  - attestation challenge = the session nonce (fresh, used once);
  - `attestationApplicationId`: package `com.capitamoney.app`, signing certificate on the service's list;
  - root of trust: bootloader locked; boot state *verified*, or *self-signed* with a verified-boot key on the service's list (for example GrapheneOS) – phones without Google services pass;
  - the session key signs the request that carries chip data and film: changed data or a second sender are refused.
- **iPhone – App Attest** (Apple's DeviceCheck): the app's attested key is checked once against Apple's App Attest root (app id = team id + bundle id); each verification carries an assertion over the session nonce and the request; the counter must rise. Needs an Apple developer account (team id) – until the owner has one, the iPhone app cannot verify; iPhone people verify at a helper.
- **Laptops cannot be checked.** The laptop hands the whole document check to a phone by QR code: the phone reads the chip *and* films the face (ADR 0021's "selfie by the laptop camera" is dropped).
- **Refused**: emulators, phones with an unlocked bootloader or an unknown system, apps signed by others, phones without hardware attestation (rare, old or uncertified). These people verify on a helper's phone or by the other ways of ADR 0021 that do not film (national eID).
- **Play Integrity is not used** anywhere (ADR 0023's `CapitaPhoneCheck` with Play Integrity is dropped, it was not built).

### Proof that the chip itself was there
Passive authentication (ADR 0021) proves the data are the state's, not that the chip was present: the data of a chip read once can be copied. Now, where the document has it:
- **Active authentication** (DG15): the chip signs the service's 8-byte chip nonce with its private key; the service checks the signature with the public key of DG15 (RSA with ISO/IEC 9796-2, or ECDSA). The service checks this itself.
- **Chip authentication** (DG14): the app runs it while reading (JMRTD on Android, Core NFC on iPhone) and reports the result inside the request signed by the checked phone.
- What the security object lists, the service requires: if EF.SOD carries a hash of DG15, active authentication must succeed; if of DG14, chip authentication must have succeeded. Removing a file to dodge the test breaks the security object. Documents with neither (old passports) are accepted with passive authentication and the face check.

### The order of a document check
1. `POST /v1/document/start` (account): the service answers a session, a 32-byte phone-check nonce and an 8-byte chip nonce, valid 5 minutes.
2. The phone makes its session key (Android) or assertion (iPhone) with the nonce.
3. The phone reads the chip: PACE or BAC, chip authentication, active authentication with the chip nonce, EF.SOD, DG1, DG2, DG14, DG15.
4. `POST /v1/document/moves` (session): the service names the head movements, valid 60 seconds from now – reading a chip may take longer than a minute, filming may not.
5. The phone films and sends `POST /v1/document/finish`: chip files, chip answers, frames, attestation, signature of the session key over the request.
6. The service checks phone, chip, face; signs the claim; keeps nothing.

The session key (Android) or the App Attest key (iPhone, certified for SHA-256 of the nonce) signs the **request digest** `SHA-256("capita/document/v1" ‖ nonce ‖ SHA-256(session) ‖ SHA-256 of EF.SOD, DG1, DG2, DG14, DG15, chip answer – absent = empty ‖ chip authenticated, 1 byte ‖ number of frames, 4 bytes big-endian ‖ SHA-256 of each frame)` – `capita_issuer::phone::request_digest`, `app/native/phone.js`, one shared test vector.

Built: `crates/capita-attest` (Android key attestation, App Attest), `capita_mrtd::check_chip` + `aa` (active authentication RSA ISO/IEC 9796-2 and ECDSA, chip authentication demanded where DG14 offers it), identity service routes `start` → `moves` → `finish`, roots in `services/issuer/trust/attestation/` (README: sources, fingerprints, Google's status list).

## Consequences
+ A phone's key cannot be stolen from a distance; no Google or Apple account is a way in.
+ "One person, one account" holds against faked films: they would need a real, unchanged phone running the Capita app.
+ No Google service on the phone: F-Droid builds and de-Googled phones with a locked bootloader pass.
+ Copied chip data no longer pass where the chip can prove itself.
− A lost phone without a linked device means verifying again and waiting seven days.
− Some phones fail the check; their owners need a helper's phone.
− iPhone verification waits for an Apple developer account (App Attest needs the team id).
− **The alpha's signing key is public** (ADR 0023): anybody can sign a changed app with it, so the "genuine app" part of the check only means something once the operator's release key exists and the development key is taken off the service's list. Owner step before real money.
− The service needs Google's root certificates and fetches Google's revocation list; Apple's App Attest root for iPhones.
− Laptop cameras no longer verify.
- Vermutung (Claude, ungeprüft): StrongBox signing takes about a second on some phones – acceptable for payments; the trusted environment is faster.
