# Capita app – prototype

Clickable, installable prototype of the Capita app. Demo money only: nothing leaves the device, no chain, no server. Concept: [`docs/app-concept.md`](../docs/app-concept.md). Why a web app: [ADR 0015](../docs/adr/0015-app-web-first.md).

## Try it
```
cd app
python3 -m http.server 8000
```
Open http://localhost:8000 – on a phone: same network, the computer's address instead of `localhost`. Settings → Demo has "A friend pays you 2.00", "One week later" and "Start again".

## Test
```
cd app
npm test
```
Tests cover `logic.js`: start grant, verification, monthly Capita, claim window, no debt, amounts.

## Files
| File | What |
|---|---|
| `index.html` | all screens |
| `app.css` | look (P02 sci-fi colours and type, reduced) |
| `app.js` | screens, navigation, storage |
| `logic.js` | the rules as far as the app shows them; mirrors `rules/alpha.toml` |
| `sw.js`, `manifest.webmanifest`, `icons/` | install and offline |
| `vendor/qrcode.mjs` | QR code generator, MIT, Kazuhiko Arase |
| `fonts/` | Chakra Petch, IBM Plex Sans (SIL OFL 1.1, licences beside them) |

Changing a rule value: change it in `rules/alpha.toml` and in `RULES` in `logic.js`.

## Android shell (ADR 0023)
The same files, wrapped by Capacitor (`android/`), plus the chip reader (`CapitaChipPlugin`, JMRTD) and passkeys inside the app. CI builds the APK (`android` job) and, on `main`, puts it on the pre-release `android-alpha`. Local build (needs the Android SDK): `npm ci && npm run android && cd android && ./gradlew assembleDebug`.

Signed with a **public development key** (`android/dev-signing.jks`, password `capita-development`): anybody can sign an APK the site trusts – test money only. Its fingerprint stands in `well-known/assetlinks.json` (served as `/.well-known/assetlinks.json`) and as origin `android:apk-key-hash:tBJj4apsvlOD17ABfX121JoOTV102WbBwb8DU1HhcO4` on the test chains. A release key replaces both before real use.

`npm run web` copies what the app ships into `www/` – for the shell and for GitHub Pages (`.github/workflows/pages.yml`).
