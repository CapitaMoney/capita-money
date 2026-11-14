# The Capita app – concept

Status: alpha concept, 2026-11-14. Prototype: [`app/`](../app) (clickable, installable, demo money only). Decision on the form: [ADR 0015](adr/0015-app-web-first.md).

## What the app must do
A person who has never heard of Capita opens the app and, within one minute, holds Capita and knows what to do next. Everything else waits.

The lesson behind it: the first sixty seconds decide. Good per-capita currencies exist; they lose people at the door – no store listing, a key to write down, a day of setting up, a separate desktop program, a different name on every screen (user tests of encointer and Monnaie Libre, 2026-11-08).

## Principles
1. **One thing per screen.** One sentence, one main button.
2. **Nothing technical is ever shown.** No keys, addresses, ids, hashes, JSON, block numbers, "claims", "tiers", "issuers". The machinery runs in the background. Payment codes are shown as QR pictures, never as text.
3. **Plain words.** Capita, receive, send, verify, gift. Amounts as `4.29 Capita`.
4. **No debt.** There is no minus, no credit, no "borrow" button. What cannot be paid is not offered.
5. **One way in.** Capita can later be bought with euros; there is no button to sell Capita for euros.
6. **No secret words.** A lost phone is replaced by verifying again (rules: key recovery). The app never asks anyone to write down twelve words.
7. **Light on the phone.** No timers, no polling, no proof of work, no animation loops, no background sync. Money is collected when the app comes to the screen.
8. **One app everywhere.** The same app on phone and desktop, one name: *Capita*.
9. **Honest.** Capita is called an experiment, everywhere.
10. **Calm look.** The P02 sci-fi design, reduced to its colours, type and the glow of the ink: dark green glass, light from the top right. No circuit board drawing, no blur.

## The first sixty seconds
| Time | Screen | The person sees | The person does | In the background |
|---|---|---|---|---|
| 0–10 s | Welcome | "Capita – Money everyone receives, equally." "An experiment. No sign-up, no e-mail, no password." | taps **Start** | – |
| 10–25 s | Name | "What is your name? People see it when you pay them." | types a name, **Continue** | key pair made on the device; account opened (`capita.open`); start grant reserved |
| 25–40 s | Gift | "Welcome, Anna" · **1.00 Capita** · "Your first Capita. A gift – everyone gets one. It pays for your verification. After that, Capita arrives every month." | taps **Open Capita** | – |
| 40–60 s | Home | balance **1.00 Capita**, "Kept for your verification", card "Receive 10.00 Capita every month – Verify" | reads, may tap **Verify** | – |

After one minute the person has: the app, a name, 1.00 Capita, and one clear next step. Verification is offered, not forced, and happens after the first minute.

## Screens
| Screen | Purpose | Content |
|---|---|---|
| Welcome | promise | mark, name, one sentence, Start |
| Name | the only question | one field |
| Gift | the moment | 1.00 Capita, what it is for |
| Home | every day | balance; next-step card (verify / verify again / "You receive 10.00 Capita a month"); Receive, Send; four recent lines |
| Verify | the step to monthly Capita | what is checked, that nothing is kept, what it costs (the gift), how (document or letter – not built) |
| Receive | get paid | QR code, own name, optional amount |
| Send | pay | scan a code, or pick from people paid before |
| Amount | how much | keypad; "Available: …"; Send button carries the amount; refuses more than is there |
| Sent | confirmation | tick, amount, name |
| History | all lines | "Welcome gift", "Verification", "Your Capita", "From Anna", "To Ben" with day and amount |
| Settings | the rest | name; lost phone; about the experiment; days left of the verification |

Later, not in the alpha: buy Capita with euros (GNU Taler, ADR 0004, 2discuss N); market for digital goods; chat.

## Words
| Shown | Never shown |
|---|---|
| Capita | token, coin, CPT, unit |
| Welcome gift | start grant, reserved balance |
| Your Capita | issuance, claim, accrual, UBI payout |
| Verify, verified | credential, claim, issuer, level, membership, DID |
| Receive / Send | transfer, transaction, extrinsic |
| Your payment code (picture) | address, account id, public key |
| Lost your phone? Verify again | seed phrase, recovery key, key move |

## Rules the app follows
From [`rules/alpha.toml`](../rules/alpha.toml); the prototype mirrors them in `app/logic.js`.
- Start grant 1.00, earmarked: it pays the own verification and cannot be sent.
- Verification costs 1.00 (1 € = 1 verification); the gift pays the first one; it holds 365 days.
- Verified: 10.00 Capita a month (example value, 2discuss A), by the second, collected on opening.
- Claim window 14 days: what is not collected within 14 days lapses. The app says so once, plainly.
- After a verification the account moves to the paying step by its own call (`step-by-holder`): the app makes that call itself, unseen.
- No balance below zero.

## Platforms
One web app, installable (PWA): Android, iPhone, desktop browsers – one code, no separate desktop program. Store presence later by thin wrappers around the same code: Google Play as a Trusted Web Activity, F-Droid and the App Store as a wrapper (for example Capacitor). Reasons: ADR 0015.

## Open questions
- **Collecting without opening:** the claim window means money lapses if the app is not opened for 14 days. A reminder needs either a push service (a server) or a wider window. Proposal: a window of at least 31 days, so a monthly habit never loses money (rules chat).
- **Verification flow:** document check not chosen (2discuss F); the Verify screen is a placeholder.
- **Name:** shown to people who are paid. Is a name chosen freely, or the verified one? Proposal: freely chosen display name, the verified name stays with the identity service.
- **Scan a code:** camera access in a web app works on Android and iPhone (getUserMedia); to be built with a small QR reader library.
- **Languages:** alpha in Oxford English only; German and others later.
- **Demo settings** (a friend pays, one week later) exist only in the prototype.

## Next steps
1. Five non-technical people try the prototype; measure the time to the home screen and ask what they think happens next (plan step 4: play-money demo = UX test).
2. Camera scan, Verify flow once F is decided (Opus: keys, NFC, face).
3. Connect to the test chain: key on the device, `capita.open`, transfers, collection (Sonnet plumbing).
4. Store wrappers (Play, F-Droid, App Store).
