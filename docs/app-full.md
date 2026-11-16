# The full Capita app – every device, keys, verification

Status: concept for the alpha, 2026-11-15. Owner decisions of 2026-11-14 are marked *(owner)*. Builds on [the app concept](app-concept.md) (first sixty seconds, words, principles) and [ADR 0015](adr/0015-app-web-first.md) (one web app first). Decisions: [ADR 0020](adr/0020-keys-passkeys-linked-devices.md) keys, [ADR 0024](adr/0024-hardware-keys-checked-phones.md) hardware keys + phone check (changes 0020, 0021, 0023, owner 2026-11-15), [ADR 0021](adr/0021-verification-ways.md) verification, [ADR 0022](adr/0022-helper-stations.md) helper stations, [ADR 0023](adr/0023-app-shells-and-hosting.md) shells and hosting, [ADR 0025](adr/0025-hand-over-between-devices.md) hand-over between two devices.

## In one paragraph
One app, *Capita*, the same code on every device. It runs as a web app at `app.capitamoney.com` on every system – Android, iPhone, Windows, macOS, Linux – and is wrapped in a thin native shell where a phone must do what a browser cannot: read the chip of a passport, keep the money key inside the phone's security chip, prove that phone and app are genuine. A person's money key lives in that chip and never leaves it – a fingerprint or a look at the phone confirms a payment; a laptop joins as a second device with a passkey *(owner 2026-11-15)*. One account may live on up to **three devices**. A person is verified in whichever way they can: their own phone reading their passport, a helper's phone, an electronic ID of their country, a letter. None of these ways is the only one *(owner)*, so that nobody is left out because their country has no electronic ID.

## What runs where
| | Web app (browser) | Android shell | iPhone shell |
|---|---|---|---|
| Where from | `app.capitamoney.com`, "Install" in the browser | APK on GitHub releases now; F-Droid prepared; Google Play after the e.V. | after the e.V. and an Apple account *(owner)* |
| Systems | Android, iPhone, Windows, macOS, Linux (Chrome, Edge, Brave, Firefox, Safari) | Android 9 and later | iOS 16 and later |
| Receive, send, history | yes | yes | yes |
| Money key | passkey, else a key the browser keeps | hardware key (StrongBox / trusted environment) | hardware key (Secure Enclave) |
| Read a passport chip (NFC) | no – browsers cannot | yes | yes (Core NFC) |
| Selfie with head movements | no – a checked phone films (ADR 0024) | yes | yes |
| Electronic ID / EU wallet | yes – hands over to the ID app and back | yes | yes |
| Phone check (genuine app and phone), required to verify | no | key attestation, no Google service | App Attest (needs Apple account) |

A laptop is a web app, never a separate program *(owner: app and website for every system)*. What a laptop cannot do itself – read a chip – it hands to a phone by a QR code.

## Keys (ADR 0020, ADR 0024)
- **On a phone the key lives in the security chip** *(owner 2026-11-15)*. The Capita app asks the chip (Android StrongBox or trusted environment, Apple Secure Enclave) to make a key that can never be read out, copied or synced. Every payment asks for the fingerprint, face or screen lock – nothing to remember, nothing to write down, no Google or Apple account involved.
- **In a browser** (a laptop, or an iPhone until the iPhone app is out) the key is a passkey; where no passkey is possible, a key the browser keeps and never lets out. A browser key is a second device of the phone's account; a person who starts in a browser links the app later, and its chip key becomes the home key after seven days.
- **The account is made from the first key.** No name, no e-mail, no password. The chain checks every signature itself; nobody can move money without the device.
- **Up to three devices.** "Use Capita on another device": the new device shows a QR code, the phone scans it and confirms. The first device is the *home* device: only it can remove the others. Linked devices can remove only themselves.
- **Lost phone:** with a second device – it becomes the home device after seven days, unless the old phone objects. Without – verify again; the account moves to the new phone's key (ADR 0013, seven days). The old phone can stop that move: then the account is locked for both until the board has decided (ADR 0028). Removing the app from a phone removes its key; Settings says so.
- What the person sees: "Confirm with your fingerprint", "Use Capita on another device", "Your devices" (phone, laptop – with the day each was added), "Lost your phone?". Never: key, passkey, credential, signature.

## Verification (ADR 0021)
Every way ends the same: an identity service signs a claim *this account belongs to one verified person*, with the person key from name and date of birth (rules 17, 18). The ways differ in what proves the person.

| Way | Proves | Needs | When |
|---|---|---|---|
| **Own phone: chip + selfie** (base) | the state's signature on the document data; the face in front of the camera is the face on the chip, alive | phone with NFC (Android shell now, iPhone shell later); a passport or ID card whose chip photo may be read | alpha |
| **Helper's phone** | the same, on someone else's phone | a helper with the app; the person and their document present | alpha |
| **Own laptop** | the same: the laptop shows a QR code, a checked phone (own or helper's) reads the chip and films the face (ADR 0024) | any checked NFC phone | alpha |
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

## Phone check (ADR 0024)
*(owner 2026-11-15: required, no Google service)* A verification counts only from a phone that proved it is a real phone with an unchanged system, running the Capita app – the defence against faked camera pictures. Android proves it with **key attestation**: the phone's security chip signs a statement about the phone and the app; the identity service checks it against Google's published root certificates – the phone itself contacts nobody, so phones without Google services pass. The iPhone proves it with **App Attest** (needs the owner's Apple developer account). Laptops cannot prove it; they hand the check to a phone. Phones that fail (unlocked bootloader, unknown system, emulator) verify at a helper. Where the chip can prove itself (active or chip authentication), copied chip data are refused too.

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
| 5 | screens: verify (chip, laptop → phone, helper), devices, test network; hand-over box (ADR 0025) | Opus | built 2026-11-15 (CI runs the device calls on the chain; first phone test = owner). Not yet: assisted payment (`assisted.pay`, owner questions of ADR 0022) |
| 6 | eID adapters (German eID, EU wallet, Estonia), after the e.V. | Opus | later |
| 7 | iPhone shell, store listings | Sonnet | after e.V. + Apple account |
| 8 | hardware key on chain (`DeviceKey::Hardware`), ADR 0024 | Opus | built 2026-11-15 |
| 9 | phone check in the identity service (Android key attestation, App Attest) + chip proof (active / chip authentication); new order start → moves → finish | Opus | built 2026-11-15 |
| 10 | Android shell: hardware key, session key with attestation, chip authentication + active authentication | Opus | built 2026-11-15 (CI compiles; first phone test = owner) |
| 11 | iPhone shell (Capacitor, Core NFC, Secure Enclave, App Attest), built by CI; tests by friends with Macs | Opus | built 2026-11-15 (CI builds for the simulator, unsigned; on a phone = Apple team) |
| 12 | German eID adapter (`eid-de`), built and tested against a stand-in; certificate = owner | Opus | next |
| 13 | Capita by age (ADR 0026): date of birth in the claim, the age at the first verification, twice the age on each birthday; screen "Verified" with the amount and the ask for notifications; demo money by age | Fable | built 2026-11-15 (not yet: the reminder itself, the real balance in the app) |
| 14 | money on chain in the app: balance, send by code, receive, birthdays claimed on opening ([ADR 0027](adr/0027-money-on-chain-in-the-app.md)) | Fable | built 2026-11-16 (CI pays on the chain; first phone test = owner) |
| 15 | no fees, a free quota instead: so many transactions per verified person and day; an account that is not verified only verifies ([ADR 0029](adr/0029-free-quota-instead-of-fees.md)); the app says a refusal in words | Fable | built 2026-11-16 (CI: the node refuses what the rule forbids) |

Tests the owner does: install the APK; verify with the German passport (chip + selfie); try the Personalausweis (expected: chip photo refused → eID later); the web app on the MX Linux laptop.

## Open questions
- Offene Frage: helper accountability – who may be a helper (anybody, or listed by an identity service), and is a helper's count of checks public per helper or only per station?
- Offene Frage: assisted accounts – daily limit (proposal: 20 Capita), and whether a person can switch an assisted account to a phone later (proposal: yes, by verifying on the phone).
- Offene Frage: machine-readable zone – typed by the person (alpha) or read by the camera (needs a text recogniser; Google's ML Kit is not free software, Tesseract is).
- Offene Frage: Estonia and other national eIDs – which first, after the e.V.
