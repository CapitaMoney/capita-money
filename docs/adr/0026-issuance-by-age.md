# ADR 0026 – Issuance by age

Status: accepted for the alpha (2026-11-15; rules diary 4, 19, v0.15: owner decisions; design by Claude under the owner's rule "always the best solution"). Changes [ADR 0003](0003-integer-cents.md) (the core now knows one calendar function), [ADR 0011](0011-identity-by-credentials.md) (a claim states the date of birth; claim format v3) and [ADR 0012](0012-rules-file.md) (`issuance.age`). Builds on [ADR 0002](0002-lazy-issuance.md) (nothing is issued until claimed).

## Context
The owner decided how Capita come into being (diary 4): **by age only**. Right at the first verification a person receives the age in Capita, at once ("you get your age in Capita"). From then on every birthday brings the age factor (2) times the age, all at once – a gift, not a daily trickle. No monthly amount, no income test, no minimum age (diary 19); the same worldwide, with an optional value per country.

Until now the core issued by a rate per ladder step and knew neither a birthday nor an age; a claim stated "no date of birth".

## Decision
- **The claim states the date of birth** (`Claim::born`, optional): the identity service reads it from the document – it already needs it for the person key (ADR 0013) – and signs it. Signed bytes: tag `capita-claim-v3`, after the residence one byte (0/1) and, if present, year (2 bytes, big-endian), month, day. A date after the day of issue is refused. The service still stores no date of birth.
- **The core knows a calendar day**: `capita_core::types::Date` (Gregorian, UTC, years 1–9999; only days that exist can be decoded). A birthday begins at 00:00 UTC, everywhere. Born on 29 February, the birthday of a year without that day is 1 March. This is the only calendar in the core; rates keep their fixed months and years (ADR 0003).
- **Rules** (`Params::age`, rules file `issuance.age`): `per_year` (2.00; the age factor), `at_verification` (1.00), `min_level` (1), optional amounts per country. All zero = off; the ladder stays as it was (alpha: amount 0, the step only marks "verified").
- **First verification**: the first statement that verifies an account and names a date of birth sets `age × at_verification` aside for the next claim, once per person (`AccountState::first_payout_made`; it follows the person to a new key). The chain claims at once inside the same transaction: the person presents the claim and the money is there.
- **Birthdays**: a claim pays every birthday since the last claim – `age completed × per_year` in force on that day – if the account was **verified on that day** and the birthday lies within the **claim window**. Nothing new in bookkeeping: birthdays ride on the cursor of lazy issuance (`paid_until`), so freezing, reopening, a change of level and a move to a new key treat them exactly like money by rate – accounted before the change, never reaching back.
- **Per country**: the amount of the country of residence, if the rules list one; residence is the only country the core knows.
- **App**: after a verification a screen shows the age in Capita, what the next birthday brings, and asks – by a tap – to allow notifications. The demo money of the prototype follows the same rules.

## Consequences
+ The decided rule is the code: one value changes the factor; a country is one table in the rules file.
+ A birthday can never pay twice: the age on the day of the last claim counts every birthday before it.
+ An issuer that states no date of birth (a letter check) gives no money by age – in line with diary 30: only a document check verifies.
− **The date of birth is public** on the chain, next to the account (not next to a name). The payment day and the amount would give it away within the claim window anyway; the claim states it exactly. Offene Frage (diary 4, P04.3): acceptable until zero-knowledge proofs? A date of birth narrows down who is behind a person key.
− **A birthday is lost** if the verification had run out on that day (diary 9: inactive until verified again), or if the money is not claimed within the claim window – with the decided 14 days (diary 14) that means: open the app within two weeks of the birthday. Offene Frage: a longer window for birthdays (Claude's proposal in the diary: until the next birthday) – one value, `issuance.claim_window`.
− UTC: east of Greenwich the birthday money arrives some hours into the day, west of it the evening before.
- Offene Frage: whether the first birthday after the verification already pays the full factor (built: yes; a person verified the day before the birthday gets 1 × and 2 × the age within two days).
- Offene Frage: country of a country amount – residence (built) or the country of the document (diary 4).
- Not built: the reminder itself (a notification on the birthday needs a push service or a native plugin); a cap from the work-willingness research (2discuss Y); showing the real balance in the app (next build step: money on chain).
- Vermutung (Claude, ungeprüft): the launch-day spike (many verify at once, each receiving the age) is bounded only by the pace of verification (diary 4, 2discuss R).
