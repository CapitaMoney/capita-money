# The full Capita app – every device, keys, verification

Status: concept for the alpha, 2026-11-15. Owner decisions of 2026-11-14 are marked *(owner)*. Builds on [the app concept](app-concept.md) (first sixty seconds, words, principles) and [ADR 0015](adr/0015-app-web-first.md) (one web app first). Decisions: [ADR 0020](adr/0020-keys-passkeys-linked-devices.md) keys, [ADR 0021](adr/0021-verification-ways.md) verification, [ADR 0022](adr/0022-helper-stations.md) helper stations, [ADR 0023](adr/0023-app-shells-and-hosting.md) shells and hosting.

## In one paragraph
One app, *Capita*, the same code on every device. It runs as a web app at `app.capitamoney.com` on every system – Android, iPhone, Windows, macOS, Linux – and is wrapped in a thin native shell where a phone must do what a browser cannot: read the chip of a passport, keep a passkey inside the app, ask the phone maker whether the phone is genuine. A person's money key is a **passkey** – a fingerprint or a look at the phone confirms a payment – with a key kept by the app where a phone has no passkeys. One account may live on up to **three devices**. A person is verified in whichever way they can: their own phone reading their passport, a helper's phone, an electronic ID of their country, a letter. None of these ways is the only one *(owner)*, so that nobody is left out because their country has no electronic ID.

## What runs where
| | Web app (browser) | Android shell | iPhone shell |
|---|---|---|---|
| Where from | `app.capitamoney.com`, "Install" in the browser | APK on GitHub releases now; F-Droid prepared; Google Play after the e.V. | after the e.V. and an Apple account *(owner)* |
| Systems | Android, iPhone, Windows, macOS, Linux (Chrome, Edge, Brave, Firefox, Safari) | Android 9 and later | iOS 16 and later |
| Receive, send, history | yes | yes | yes |
| Passkey | yes | yes (Credential Manager) | yes |
| Read a passport chip (NFC) | no – browsers cannot | yes | yes (Core NFC) |
| Selfie with head movements | yes (camera in the browser) | yes | yes |
| Electronic ID / EU wallet | yes – hands over to the ID app and back | yes | yes |
| Phone check (genuine app and phone) | no | Play Integrity, where present | App Attest |

A laptop is a web app, never a separate program *(owner: app and website for every system)*. What a laptop cannot do itself – read a chip – it hands to a phone by a QR code.

## Keys (ADR 0020)
- **The passkey is the key.** The app asks the phone for a passkey for `app.capitamoney.com`. The phone keeps it in its secure chip and, if the person uses one, in their password manager (Google, Apple, Bitwarden, KeePassXC …), which carries it to a new phone. Every payment asks for the fingerprint, face or screen lock – nothing to remember, nothing to write down.
- **Where no passkey is possible** (an old phone, a phone without Google services and without a password manager), the app makes its own key inside the browser storage, which never leaves it. The person sees no difference, except that a lost phone then means verifying again.
- **The account is made from the first key.** No name, no e-mail, no password. The chain checks the passkey's signature itself; nobody can move money without the device.
- **Up to three devices.** "Use Capita on another device": the new device shows a QR code, the phone scans it and confirms. The first device is the *home* device: only it can remove the others. Linked devices can remove only themselves.
- **Lost phone:** with a second device – it becomes the home device after seven days, unless the old phone objects. With a synced passkey – sign in to the password manager on the new phone, done. With neither – verify again; the account moves to the new key (ADR 0013, seven days).
- What the person sees: "Confirm with your fingerprint", "Use Capita on another device", "Your devices" (phone, laptop – with the day each was added), "Lost your phone?". Never: key, passkey, credential, signature.

## Verification (ADR 0021)
Every way ends the same: an identity service signs a claim *this account belongs to one verified person*, with the person key from name and date of birth (rules 17, 18). The ways differ in what proves the person.

| Way | Proves | Needs | When |
|---|---|---|---|
| **Own phone: chip + selfie** (base) | the state's signature on the document data; the face in front of the camera is the face on the chip, alive | phone with NFC (Android shell now, iPhone shell later); a passport or ID card whose chip photo may be read | alpha |
| **Helper's phone** | the same, on someone else's phone | a helper with the app; the person and their document present | alpha |
| **Own laptop** | the same: the laptop shows a QR code, a phone (own or helper's) reads the chip; the selfie is taken by the laptop camera | laptop camera; any NFC phone | alpha |
| **Electronic ID of a country** (Germany, Estonia, others) | the state's own check, by card and PIN | the country's ID app; a certificate or contract with an eID provider | after the e.V. |
| **EU wallet** (EUDI, person data "PID") | the same, through one interface for every EU state | registration as relying party (German seat → e.V.); German wallet from about 2 January 2027 | test now, live 2027 |
| **Letter** | a postal address where the person receives mail | postage per letter | alpha (exists as "Challenger letter") |

The German **Personalausweis** is the most common document of the owner's first users – but its chip photo is for authorities only; services cannot read it (source: [fimportal.de, eID service description](https://fimportal.de/api/v0/xzufi-services/L100042/30611/de/pdf)). So a Personalausweis verifies through its electronic ID (card + six-digit PIN: possession and knowledge – no selfie needed) or, from 2027, through the German EU wallet that is filled from it. A German **passport** works with chip + selfie today. Vermutung (Claude, ungeprüft): identity cards of other EU states allow the chip photo with the card access number (CAN) – to be tried per country.

The document check itself (*how* chip + selfie is checked): the phone reads the data groups with the key printed on the document (machine-readable zone or CAN) and sends them, with the filmed head movements, to the identity service. The service checks the state's signature up to the country's root certificate (passive authentication; roots from the ICAO master list as the German BSI publishes it), takes name, date of birth, expiry and nationality from the chip, compares the chip photo with the film (ADR 0017), signs the claim – and keeps nothing.

## Helper stations (ADR 0022)
*(owner)* In poorer countries one helper verifies many people: women come to a station to be verified, and take out their money by identifying each time. This answers countries where letters to women are held back.

- A **helper** is anybody with the app in helper mode. The helper lends the phone; the identity service does the check. The helper cannot invent a person: the chip data carry the state's signature, and the face must move live in front of the helper's camera.
- A person **without own phone** gets an *assisted account*: no device key at all. To pay, she shows her document and her face at any station; the identity service checks her live and signs this one payment. Nobody holds her key – not the helper, not the service, not a husband. A daily limit and the public count of each helper's checks are the brakes.
- "Taking out money" is a payment from her account to whoever gives her goods or cash for it – often the station. Capita stays Capita; the station is a shop.
- Offene Frage: a person whose document is taken from her cannot pay (neither can the one who took it). A second way – a face template she agrees to keep at the identity service – would break "keeps nothing"; owner decision.

## Phone check (ADR 0023)
During a verification the Android shell asks Google (Play Integrity) and the iPhone shell asks Apple (App Attest) whether app and phone are genuine – a defence against faked camera pictures. Where a phone has no such service (F-Droid, phones without Google), the head-movement test is longer and stricter instead. Never a condition *(owner: "when available")*. Vermutung (Claude, ungeprüft): Play Integrity needs a Google Cloud project of the operator – owner, later.

## Addresses and servers
- App: `app.capitamoney.com` (GitHub Pages). The passkey belongs to that name; it must never change once real people use it. DNS: a `CNAME` record `app` → `capitamoney.github.io` (owner).
- Test chain and identity service: creationHub *(owner)*; reachable while it runs, from phones in the same network. Later: a rented server, then independent operators (rules diary O).

## Build plan
| # | Part | Model | State |
|---|---|---|---|
| 1 | this concept, ADRs 0020–0023 | Opus | this PR |
| 2 | keys on chain: passkey and device keys, three devices, home device (`capita-keys`, `pallet-capita-keys`); in the app: making keys, signing, the same bytes as the chain | Opus | next |
| 3 | document check in the identity service: chip data, state signature, master list, age, expiry, face (`capita-mrtd`, issuer routes) | Opus | next |
| 4 | Android shell: chip reading (NFC), passkeys in the shell, camera; APK built by CI; web app on GitHub Pages | Opus | next |
| 5 | screens: verify (chip, laptop → phone, helper), devices, assisted payment; connection to the test chain | Sonnet | later |
| 6 | eID adapters (German eID, EU wallet, Estonia), after the e.V. | Opus | later |
| 7 | iPhone shell, store listings | Sonnet | after e.V. + Apple account |

Tests the owner does: install the APK; verify with the German passport (chip + selfie); try the Personalausweis (expected: chip photo refused → eID later); the web app on the MX Linux laptop.

## Open questions
- Offene Frage: helper accountability – who may be a helper (anybody, or listed by an identity service), and is a helper's count of checks public per helper or only per station?
- Offene Frage: assisted accounts – daily limit (proposal: 20 Capita), and whether a person can switch an assisted account to a phone later (proposal: yes, by verifying on the phone).
- Offene Frage: machine-readable zone – typed by the person (alpha) or read by the camera (needs a text recogniser; Google's ML Kit is not free software, Tesseract is).
- Offene Frage: Estonia and other national eIDs – which first, after the e.V.
