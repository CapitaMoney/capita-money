# Capita app – prototype

Clickable, installable prototype of the Capita app. On its own it is a demo: nothing leaves the device, no chain, no server. On a test network the money is the ledger's ([ADR 0027](../docs/adr/0027-money-on-chain-in-the-app.md)). Concept: [`docs/app-concept.md`](../docs/app-concept.md). Why a web app: [ADR 0015](../docs/adr/0015-app-web-first.md).

## Try it
```
cd app
python3 -m http.server 8000
```
Open http://localhost:8000 – on a phone: same network, the computer's address instead of `localhost`. Settings → Demo has "A friend pays you 2.00", "One week later" and "Start again".

The money screens with a ledger, without building a chain: `node tests/play-network.mjs` and open the address it prints (a stand-in for node and identity service – it checks no signature and forgets everything when stopped). `/play/verify?born=1990-05-17&ago=3` verifies the newest account as if three days ago, `/play/pay?cents=250` pays it.

## Test
```
cd app
npm test
```
Tests cover `logic.js` (the demo: start grant, verification, Capita by age, claim window, no debt, amounts), `money.js` (the ledger's money as the screens show it) and `chain/` (the calls sent and the values read; the same bytes are checked by the runtime).

## Files
| File | What |
|---|---|
| `index.html` | all screens |
| `app.css` | look (P02 sci-fi colours and type, reduced) |
| `app.js` | screens, navigation, storage |
| `logic.js` | the demo's money and rules; mirrors `rules/alpha.toml` |
| `money.js` | the ledger's money as the screens show it: what may be sent, birthdays due, the device's own history (ADR 0027) |
| `device.js` | this device on a test network: its key, its account, acting on the chain, reading and paying money, the hand-over box (ADR 0025) |
| `screens.js` | verify (own phone, another phone, helper), devices, linking, test network |
| `scan.js` | reading QR codes with the camera (the browser's detector, else `vendor/jsQR.js`) |
| `chain/calls.js`, `chain/xxhash.js` | the calls these screens send and the chain state they read; vectors `tests/call-vectors.json` (checked by the runtime) and `tests/state-vectors.json` (written by the runtime) |
| `sw.js`, `manifest.webmanifest`, `icons/` | install and offline |
| `vendor/qrcode.mjs` | QR code generator, MIT, Kazuhiko Arase |
| `vendor/jsQR.js` | QR code reader jsQR 1.4.0, Apache-2.0, Cosmo Wolfe (licence beside it) |
| `fonts/` | Chakra Petch, IBM Plex Sans (SIL OFL 1.1, licences beside them) |

Changing a rule value: change it in `rules/alpha.toml` and in `RULES` in `logic.js` (the demo only – on a test network the app reads the rules from the chain).

## Test network (build plan step 5)
Without one the app is the demo above. Settings → *Test network* (or a link `…/?node=http://HOST:9944&service=http://HOST:8787`) joins a test chain (`capita-node --dev --rpc-cors all`) and an identity service (`capita-issuer`, with `[checks.document]` for passports) on a computer in the same network. Then:
- the device makes its money key (security chip in the phone apps, else a passkey, else a key the browser keeps) and opens its account on the chain;
- **Verify**: on a checked phone with NFC – passport number, birth and expiry date (or the six digits of an ID card), the chip, the face; elsewhere – a QR code for a phone (own or a helper's);
- **Help someone verify** (Settings): scan their code, their document, their face; the claim goes back to their device;
- **Your devices** (Settings): the account's devices, "Use Capita on another device" (the new device chooses "I already use Capita on another device" and shows a code), remove, "Make this the main device" after a lost phone, "Keep this phone as main".
- **money** (ADR 0027): the balance is the ledger's; **Send** scans the code on the other person's Receive screen (people paid before are listed) and this device's key signs the payment; **Receive** shows the code and says when the payment has come; birthday Capita are claimed when the app opens. The history is what this device did and saw.
`tests/chain-smoke.mjs` runs the device and money calls against the real chain in CI.

## Android shell (ADR 0023, ADR 0024)
The same files, wrapped by Capacitor (`android/`), plus three native pieces (ADR 0023, ADR 0024): the chip reader (`CapitaChipPlugin`, JMRTD – PACE/BAC, chip authentication, active authentication with the service's chip nonce), the money key in the phone's security chip (`CapitaKeyPlugin`: P-256 in StrongBox or the trusted environment, each signature confirmed by fingerprint, face or screen lock; `chain/keys.js` `createHardwareKey`, `hardwareProof`), and the phone check (`CapitaPhonePlugin`: a fresh session key whose attestation carries the service's nonce; `native/phone.js`). Passkeys still work inside the shell, for accounts that started in a browser. CI builds the APK (`android` job) and, on `main`, puts it on the pre-release `android-alpha`. Local build (needs the Android SDK): `npm ci && npm run android && cd android && ./gradlew assembleDebug`.

Signed with a **public development key** (`android/dev-signing.jks`, password `capita-development`): anybody can sign an APK the site trusts – test money only. Its fingerprint stands in `well-known/assetlinks.json` (served as `/.well-known/assetlinks.json`) and as origin `android:apk-key-hash:tBJj4apsvlOD17ABfX121JoOTV102WbBwb8DU1HhcO4` on the test chains. A release key replaces both before real use.

`npm run web` copies what the app ships into `www/` – for the shell and for GitHub Pages (`.github/workflows/pages.yml`).

## iPhone shell (ADR 0024)
The same files in `ios/` (Capacitor, Swift Package Manager), with the three native pieces of the Android shell under the same names and calls, in the local package `plugins/capita-ios` (`npm run ios` links it): `CapitaKeyPlugin` – the money key in the Secure Enclave (P-256, each use confirmed by Face ID, Touch ID or the code; DER signatures like Android); `CapitaPhonePlugin` – App Attest: a fresh key per verification, certified for SHA-256 of the service's nonce, then an assertion over the request digest; `CapitaChipPlugin` – Core NFC with NFCPassportReader 3.0.0 (MIT): PACE or BAC, chip authentication, active authentication with the service's chip nonce. iPhones open a chip only with document number and dates, not with the card access number (`no-can`). Entitlements in `ios/App/App/App.entitlements` (NFC tags, App Attest).
CI builds it for the simulator, unsigned (`ios` job, macOS). Running it on an iPhone, App Attest and the NFC entitlement need an Apple developer team (owner, after the e.V.); then the team id goes into the identity service's `apple_app_ids` (`TEAMID.com.capitamoney.app`). Local build (a Mac with Xcode): `npm ci && npm run ios && open ios/App/App.xcodeproj`.

Alpha only: the shell allows plain-http requests (`android.allowMixedContent`), so that the app reaches the test chain and the identity service on a computer in the same network (creationHub). Before real use: both behind TLS, the setting off.
