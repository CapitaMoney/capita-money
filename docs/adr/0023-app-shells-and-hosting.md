# ADR 0023 – App shells, phone check, hosting

Status: accepted for the alpha (owner 2026-11-14: "PWA + Android APK", "app.capitamoney.com", phone check "when available", server creationHub, CI builds). Extends ADR 0015. Concept: [app-full.md](../app-full.md).

## Context
ADR 0015: one web app first, store listings as thin wrappers later. The full app needs three things no browser on a phone gives: reading a document chip by NFC (web NFC reads tags, not passports; none on iPhone), passkeys inside an installed app with the app's identity, and asking the phone maker whether app and phone are genuine.

## Decision
- **One code** (`app/`), three ways to run it: browser and installed web app; an Android shell; later an iPhone shell. The shells are **Capacitor** projects around the same files – no second app, no other framework.
- The **Android shell** (`app/android/`) adds three native pieces as small Capacitor plugins in Kotlin:
  - `CapitaChip` – reads EF.SOD, DG1, DG2, DG14, DG15 with JMRTD (LGPL) over PACE or BAC; returns the bytes, nothing interpreted.
  - passkeys: the web view's WebAuthn through Credential Manager (`androidx.webkit` 1.12 or later, `setWebAuthenticationSupport`); the app serves its files under `https://app.capitamoney.com` and the site lists the shell in `/.well-known/assetlinks.json`.
  - `CapitaPhoneCheck` – Play Integrity token with the service's nonce, where Google services exist; off until the operator has a Google Cloud project.
- **Distribution:** APK built by GitHub Actions and attached to a GitHub release; F-Droid metadata prepared (the shell avoids non-free libraries except the optional Play Integrity, left out of an F-Droid build). Google Play and the App Store after the e.V. Vermutung (Claude, ungeprüft): Apple accepts wallet apps from organisations only; new private Google Play accounts must test with 12 testers for 14 days first.
- **Signing:** the alpha APK is signed with a *development* key kept in the repository and named so – like the development issuer and door, whose secrets are public. Its fingerprint stands in `assetlinks.json`. Before real money, the operator makes a release key that never enters the repository and replaces the fingerprint.
- **Web app hosting:** GitHub Pages of this repository, address `app.capitamoney.com` (DNS `CNAME app → capitamoney.github.io`); deployed by a workflow from `app/` on `main`. The passkey's relying party id is that name (ADR 0020).
- **Phone check** during a verification, never a condition: Play Integrity (Android), App Attest (iPhone shell). Without it the liveness challenge has more movements (ADR 0017 settings) – stricter, not refused.
- **Servers for the alpha:** test chain node and identity service on creationHub; the app takes their addresses from its settings (default: the same network).

## Consequences
+ The web app stays the product; the shells add only what browsers lack.
+ An Android phone verifies today, from an APK, without a store.
− iPhone people can use the web app but verify only by helper, laptop hand-over or letter until the iPhone shell exists.
− The development signing key is public: anybody can build an APK the site trusts. Acceptable with test money only; replaced before real use.
− The DNS entry is the owner's; until it exists Pages serves the app at `capitamoney.github.io/capita-money/`, where passkeys for `app.capitamoney.com` do not work.
