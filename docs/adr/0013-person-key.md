# ADR 0013 – The person key: one identifier per person

Status: accepted as technical frame (M3b, 2026-11-14). Built and tested: `capita-person-key`, the field `replaces` of a claim, rebinding in `capita-core` and `pallet-capita`. What the key is made from is a rule and not decided (2discuss K): `identity.person_key_basis` of the rules file, default `name-birthdate`.

## Context
The host refuses a second account for the same person (ADR 0011). For that every identity service has to name the same person the same way – whichever service checks, in whichever year – and everybody else differently, and the name must not tell who the person is: the ledger is public and permanent. ADR 0011 left open how ("pseudonym", realm).

## Decision

### Two steps
1. **Canonical form.** What a check established becomes one fixed sequence of bytes (`capita_person_key::canonical`, with test vectors). Names are reduced to the letters A–Z as the machine-readable zone of a document prints them, then folded so that the forms different states print for one letter coincide (`MÜLLER`, `MUELLER`, `MULLER`, `MUXXLLER`); the family name counts with 24 letters, the first given name with 12.
2. **Keyed function.** The person key is a keyed function of the canonical form and a counter: 32 bytes, the field `subject_ref` of a claim. Without the secret nobody can compute the key of a person from a list of names and birthdays.

### What it is made from: a rule
| `identity.person_key_basis` | made from | proven by | holds across | weak |
|---|---|---|---|---|
| `name-birthdate` (default) | family name, first given name, date of birth | an identity document | new documents, moves, mostly two passports | namesakes born the same day (below); a changed name (rebind) |
| `name-address` | name, country, postcode, street | a letter with a code | – | invented names at one letterbox; every move makes a new person |
| `document` | issuing state, kind, number | an identity document | – | every new document makes a new person; two passports are two persons |

Every basis has its own first byte: keys of different bases never coincide. Changing the rule changes no code.

### The keyed function: an oblivious pseudorandom function
RFC 9497, OPRF(ristretto255, SHA-512), mode 0; the key of the function derived from the secret as the RFC says, with the info `capita-person-key-v1`; input = canonical form, `0x00`, counter; person key = first 32 bytes of the output. Not a plain keyed hash, because of who holds the secret:
- **a. In the service itself** (`LocalKey`) – alpha: one service, the secret in a file.
- **b. In a separate key service**, asked blindly: the identity service sends blinded data, the key service never sees a name, the identity service never sees the secret; a rate limit stops the testing of names.
- **c. Shared among several services** (threshold): no centre.

All three compute the same function. Going from a to b to c changes no person key, no claim and no chain code: one interface (`KeyService`). b and c are not built.

### Namesakes
Two people with the same canonical form (same name, same birthday). The service keeps, next to a person key, a keyed tag of a further fact – the place of birth, where the document states it. A request with the same canonical form and another tag is another person and gets the key with the next counter (up to four). Without the further fact a namesake counts as the same person. The tag is never published.

### Rebinding
The data a key is made from can change: a new name, or a new secret after a leak. A claim then names the key it replaces (`replaces`, claim format v2). An account bound to that key is bound to the new one; the old key keeps pointing at the account, so nobody opens a second account under it (`capita_core::lifecycle::plan_attest`, `PersonAccount` of `pallet-capita`).
- New secret: the service computes the key under the earlier secrets too and finds the account by itself.
- New name: the person states the former name; it counts only if this very account is bound to it and the date of birth is the same. A setting of the service (`allow_name_change`).

### The service forgets
After signing a claim the service keeps: person key, account, number of the claim, day, kind of proof, the namesake tag. No name, no date of birth, no document, no address.

## Consequences
+ One person, one key, across services and years – as good as the basis.
+ No personal data on the ledger; no list of persons at the service.
+ The way the secret is held can grow from one service to none without anybody verifying again.
− Whoever holds the secret (a: every service of the realm) can test whether a known person has an account, and which.
− A keyed hash of personal data is pseudonymised personal data in the sense of data-protection law; on a permanent public ledger it cannot be erased. Legal view needed before real persons (P04.3).
− Folding lets different names fall together (`MICHAEL`, `MICHAL`); with the same birthday they are namesakes.

## Open
Offene Frage: 2discuss K – the basis; which document check first; minimum age (`identity.minimum_age`, default none).
Vermutung (Claude, ungeprüft): 24 and 12 letters are within every name field of identity documents; every document check (card, passport, EUDI wallet) yields name and date of birth – check both before real persons.
Offene Frage: F3 – realms: issuers that share a secret form one; how realms are kept from overlapping.
