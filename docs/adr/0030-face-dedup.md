# ADR 0030 – Face dedup: one person, two documents

Status: built, switched off (owner 2026-11-20 „Build now, off“): `identity.face_dedup = false` in `rules/alpha.toml` until the legal check (P04.3). Built and tested: `services/issuer/src/dedup.rs` (face store, keys, comparison, the board's cases), the step in `Service::issue`, the board's commands of `capita-issuer`, `capita-face <models> measure`. Not built: the app's consent screen and its screen for a held verification; a measured `same_person` value; private matching.

## Context
A person key is made from family name, first given name and date of birth (diary 17, ADR 0013). One person with two documents whose names are written differently – another transliteration of a non-Latin script, a married name in one and the birth name in the other, given names in another order, a typing error of an authority – gets two keys and could hold two accounts. Rule 21 closes the younger account when the double is found later; face dedup finds it at the verification.

Owner 2026-11-15 (diary 33, 2discuss B): face dedup wanted; a match holds the new account and the Verein board decides; no refusal by a machine. Owner 2026-11-20: build now, switched off until the legal check. Concept: `P04.7_technical-building-details/face-dedup.md` of the project.

## Decision

### When
At every document check (ADR 0021), after phone, chip and face check have passed (ADR 0024, 0017), before anything is signed – and only when the rules switch it on. Off, the print of the chip photo is dropped with the call, as before.

### What is compared with what
The face print of the **chip photo** (SFace, 128 numbers), not of the film: chip photos are taken under the same rules and compare well with each other. It is compared with **everybody of the same date of birth**, which both documents of one person carry. At 1 million accounts a group holds about 27 people (36 525 days of 100 years) instead of 1 million: fewer false holds, less work for the board.

### What is kept – only when on
One line per person in the file of the birthday group: the person key, the sealed print, the day. No photo, no name, no account, no date of birth in clear.
- The **group** is named by a keyed hash (HMAC-SHA256) of the date of birth, under a key made from the face store's **own secret** – not the secret of the person keys: whoever holds that one can test names, whoever holds this one can read faces. The service refuses to start with the person-key secret as face secret.
- The **print** is sealed (AES-256-GCM, a fresh nonce each time) under a second key made from the same secret; person key and group are bound into the seal, so a print moved to another line no longer opens. A print that does not open stops every check of its group (fails closed).
- Folders and files can be read by their owner only. Every change writes a new file and puts it in place of the old one: a power cut leaves the old file or the new one. A forgotten print is gone from the file, not marked.
- The line is replaced at every verification (yearly, diary 9) and passes to the new key when the name or the secret changes (with the board's verdicts and an open case).
- Kept while the account exists, inactive too (diary 20: else a returning double escapes); forgotten when the account is closed (diary 20, 21): `capita-issuer <configuration> forget 0x<account>`.

### A match
Likeness at or above `same_person`: the verification is **held** – nothing is signed, no ticket is used. The person reads: „your verification needs a short check by a person before it counts; please contact …“ (HTTP 409, `held`). The person sees no number and no id (diary 16): the board finds the case from the document it is shown.

The board, after a video call with both documents (as the 2nd check of diary 15):
- `decide <case> same` – one person: the new verification is refused from then on (`double-account`), the account that exists stays (diary 21).
- `decide <case> different` – twins, a lookalike, an error of the model: the pair is never held for each other again; the person verifies again and passes.

While a case of a person is open, that person stays held; asking again opens no second case. The board's commands run beside the service: every reading and every change of the face store takes a lock.

```text
capita-issuer <configuration> cases                      the verifications held, oldest first
capita-issuer <configuration> case-of <family> <given> <YYYY-MM-DD>
capita-issuer <configuration> decide <case> same|different
capita-issuer <configuration> forget 0x<account>
```

### The switch and the configuration
`identity.face_dedup` in the rules file (a rule: diary 33). On, the service needs `[checks.document.dedup]` – store folder, secret file, `same_person`, contact – or it does not start. `GET /v1/info` states `faceDedup`, so that the app can ask for consent before the document check.

### The value `same_person`
A technical value of the configuration, **without a default**: two chip photos of one person lie years apart, so the one-to-one value 0.363 of ADR 0017 does not carry over, and a stricter value has to be measured. `capita-face <models> measure <folder>` takes one folder per person with several photos each and prints, for the values 0.30 to 0.80, the share of missed doubles and the false holds per verification for groups of 1, 27 and 274 people (about 1 000, 1 million, 10 million accounts). Which photo set may be used (licence) is the owner's decision; no photo of a real person is part of the repository – the tests use random prints.

## Consequences
+ A second account under another spelling is found at the verification, not years later.
+ No machine refuses anybody (GDPR Art. 22): a person decides.
+ Off, nothing changes: nothing is kept (ADR 0017).
− On, the service keeps biometric data (GDPR Art. 9): explicit consent before the check, a data protection impact assessment (Art. 35), a legal view – P04.3. ⚠ Widerspruch diary 24 (a service keeps only person key, account, day, kind of proof): an exception is needed, decided with P04.3; the register entry `service-forgets` names it.
− A search inside a group is identification, not verification: the reason ADR 0017 gives for „not high-risk AI“ does not cover this part. Vermutung (Claude, ungeprüft): dedup at enrolment, with the person taking part, is no „remote biometric identification“ (AI Act Art. 3(41)); the class is open (P04.3).
− Whoever holds the face store's secret and its files can read prints: keep the secret apart from the files (another disk, a secret store).
− Accounts verified before the switch is turned on have no print; they get one at their next verification.
− Only the document check brings a print; the test check of test chains is not compared. A later check by a provider (diary 26) has to bring the chip photo too.
− Vermutung (Claude, ungeprüft): some documents carry a default date of birth (1 January) for people whose birthday is unknown; such dates make big groups and more false holds there – measure.
− A person who lost the key and comes back with the second document is held and refused as double; the way back is the first document (diary 15). Offene Frage (owner): the face of a new verification as extra proof before a move (concept point 8).
− A new secret of the face store means sealing every print anew: not built.

## Alternatives
- No dedup: rejected by the owner.
- Compare with all accounts: more false holds, more work for the board.
- Private matching (prints split over several services, none can read them): research level, later; fits behind the same trait (`FaceStore`).
- Dedup by a provider: Vermutung (Claude, ungeprüft): Didit may offer a face search – check when its adapter is built (diary 26).

## Try it
```sh
cargo test -p capita-issuer --test dedup
services/face/fetch-models.sh models
cargo run --release -p capita-face -- models measure <folder with one folder per person>
```
