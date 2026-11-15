# ADR 0022 – Helpers, stations and assisted accounts

Status: proposed for the alpha (owner 2026-11-14, idea; design by Claude). Concept: [app-full.md](../app-full.md).

## Context
Owner: in poorer countries one helper verifies many people; women come to a station to get verified and take out their money by identifying each time – where states hold letters to women back. Many such people have no phone of their own, or share one with a man who controls it.

## Decision

### Helper mode
Any installed app can switch to **helper mode** ("Help others verify"). In helper mode the app runs the document check (ADR 0021) for another person: her document on the helper's phone, her face in front of the helper's camera. The claim goes to *her* account:
- she has a phone: her app shows a QR code, the helper scans it – the claim names her account;
- she has none: the claim names a new **assisted account** (below).

The helper proves nothing about her and can fake nothing the service checks: the chip data carry the state's signature; the face must perform the random head movements live. The claim carries the proof kind `via-helper` and the helper's account, so an identity service can count, limit and stop a helper.

### Assisted accounts – identifying each time
An assisted account has **no device key at all**. It is bound to the person key of its holder. To pay from it, the holder identifies at any helper's phone, every time: document chip + live face, as for a verification. The identity service, satisfied, signs an **authorisation of this one payment** (account, payee, amount, nonce, a few minutes valid); the chain checks the service's signature, that the service is listed and active, and that the person key in the authorisation is the account's. Nobody holds her key – not the helper, not the service, not a relative.

"Taking out money" is such a payment, to whoever gives her goods or cash for Capita – often the station itself. Capita is not exchanged by the system; the station is a shop.

### Limits
- `assisted.daily_limit` (proposal: 20.00 Capita a day) per assisted account.
- An identity service may authorise only accounts whose claim it (or a service of the same realm) issued.
- The count of checks per helper is public per day.

## Consequences
+ A person without a phone, without a reliable postal address, can hold and use Capita.
+ No custody: nothing to steal at the station or at the service except one minute's authorisation.
− Each payment needs her document and her face: a confiscated document blocks her (and the one who took it).
− An identity service broken into could authorise payments from assisted accounts it serves, up to the daily limit; the public event and suspension of the service are the brakes.
− Coercion at a station cannot be prevented by software; the limit bounds it.
- Offene Frage: a face template kept at the service, by her consent, as a second way when the document is taken – breaks "keeps nothing"; owner decision (and GDPR Art. 9, P04.3).
- Offene Frage: who may be a helper – anybody, or listed by an identity service.
- Offene Frage: switching an assisted account to a phone later (proposal: yes, verify on the phone, the account gets a key).
- Built 2026-11-15 (build plan step 5, ADR 0025): helper mode for people with the app – "Help someone verify" scans their code; the identity service stores the helper's account with the claim (`helper`, proof kind `via-helper`).
- Not built: assisted accounts need a call `assisted.pay` in `pallet-capita-keys` and the authorisation in the identity service – after the owner's answers to the open questions above.
