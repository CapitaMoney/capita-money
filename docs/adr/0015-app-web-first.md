# ADR 0015: The app is one installable web app first

Status: accepted for the alpha, 2026-11-14.

## Context
The app is wanted on Android, iPhone, the web and desktop computers. User tests of other per-capita currencies showed the cost of many separate programs: a mobile app and a different desktop program, no store listing, a day to set up. The owner's requirements: one-tap install, one name everywhere, account in under two minutes, no separate desktop program, no hot phone, no drained battery. Existing wallets (Ğecko, encointer) are Flutter apps built around their own chains and show technical details the owner rejects.

## Decision
- The first app is a **progressive web app** (`app/`): plain HTML, CSS and JavaScript modules, no build step, no framework. It installs from the browser on Android, iPhone and desktop and works offline.
- Money rules stay in Rust (`capita-core`); the app mirrors only what it must show (`app/logic.js`, tested with `node --test`). When the app talks to a chain or Taler, the host decides; the app only displays.
- Store listings come later as thin wrappers around the same code (Trusted Web Activity for Google Play; a wrapper such as Capacitor for F-Droid and the App Store).
- Fonts and libraries are shipped with the app (no requests to font or analytics services). The QR code generator is `qrcode-generator` (MIT); fonts Chakra Petch and IBM Plex Sans (SIL OFL 1.1).
- Battery: no timers or polling; work happens when the app comes to the screen.

## Consequences
- One code for every platform; changes reach everyone at once.
- Web apps on iPhone have limits (push messages only for installed apps, storage may be cleared after long disuse); the key on the device needs a recovery path anyway – verifying again (ADR 0013).
- NFC reading of identity documents is not available to web apps on iPhone; that part will need the native wrapper.
- Forking Ğecko or the encointer wallet is not needed for the alpha; their code stays a reference.
