# ADR 0011 – Identity by credentials

Status: accepted (M3, 2026-11-13); changed by M3b (2026-11-14): the pseudonym of a person is the person key of ADR 0013; claims have format v2 with the field `replaces`; an account moves to a new key (below, "A lost key"); an identity service exists (ADR 0014). Built and tested: the check (`capita-credential`), the chain host (`pallet-capita-credential`), the wiring to Duniter's pallets in the runtime. Not built: an identity service that issues claims (see "Not in M3").

## Context
The rules need three facts about the holder of an account: that it is one person with one account, how thoroughly that was checked (verification level), and optionally where the person lives (ADR 0008). Who establishes them is open (F3). ADR 0009 left the place: Duniter's identity and membership pallets, with a credential pallet where Duniter has its web of trust. A web of trust needs people who know each other; newcomers in remote places know nobody. A document, an e-mail address or a letter can be checked from anywhere – by a service.

## Options
1. **Verifiable credentials (W3C) checked on chain.** JSON-LD or JWT parsing and many signature suites inside the runtime; the credential, with whatever personal data it holds, public for ever. Rejected.
2. **One bridge account that calls `attest`.** Simple, but one key, and the chain shows no proof of anything. Rejected as the design; root can still attest by hand on the test chain.
3. **A short signed claim, derived from a credential, checked against a public list of issuers.** Chosen.
4. **Zero-knowledge proofs** (the chain learns "one person, level n" and nothing else). Later: fits behind the same list as another kind of proof.

## Decision

### Roles
- **Issuer** = a trusted identity service. It checks a person by whatever means it has (e-mail or phone challenge, identity document, eID, letter to the home address, a verifiable credential of somebody else) and signs a **claim** about the result.
- **Holder** = the person. Presents the claim to the host from the account it is made out to.
- **Host** = chain or service. Keeps the list of issuers and checks claims with `capita-credential` – one implementation for both hosts, as with the rules (ADR 0001).

### The claim
Issuer (number on the list) · serial (16 bytes, unique per issuer) · subject (the account) · pseudonym of the person (32 bytes) · verification level · residence (country, proven when), optional · numbered attributes, optional · issued at · expires at, optional.
No name, no document number, no date of birth: derived facts only. The verifiable credential a claim was derived from stays with the person.
The issuer signs fixed bytes: a tag (`capita-claim-v1`), the audience, then every field in a fixed layout, integers big-endian (`capita_credential::encoding`, with a test vector). No serialization library needed, so an issuer can be written in any language. Signature schemes: Ed25519, Sr25519; another scheme is another variant of two enums.

### What makes a claim count
- signed by a key of a listed, active issuer, for this audience (chain: hash of the first block) – a claim for one host counts on no other;
- presented by its subject – a claim found or stolen is of no use;
- within its validity, and the validity within what the issuer may state;
- level, residence and attributes within what the issuer may state;
- its serial neither presented before nor taken back;
- not older than the claim the account holds.

### The list of issuers
Per issuer: a decentralised identifier (DID), 1–8 keys (several while a key is replaced), a realm, the highest level it may state, whether it may state a residence, which attributes, the longest validity, a status. The limits say what a service is trusted for: one that checks e-mail addresses gets a low level, one that reads documents a higher one, one that sends letters a residence.
- **Active / suspended / revoked.** Suspended: nothing new, what was stated stands. Revoked: what was stated counts no longer.
- **DID.** A host cannot fetch documents, so it never resolves a DID. It trusts the keys on the list; whoever keeps the list checks that they are the keys of the DID document (`did:web`: the document on the web server of the service). `did:key` is the key itself as text; the host checks that one.
- **Who keeps the list** is an origin of the runtime (`RegistryOrigin`). Test chain: root. Offene Frage: F3, E1.

### One person, one account
The person identifier of the rules = BLAKE2b-256 of tag, realm and pseudonym. The rules refuse a second account for the same person and another person for the same account (`capita-core`).
- A **realm** is a group of issuers that give the same person the same pseudonym. Within a realm a person is found again whichever issuer checks; across realms nothing links two pseudonyms.
- Offene Frage: F3 – how issuers of one realm agree on a pseudonym (for example a keyed hash of document country and number, with the key shared inside the realm, or a common deduplication service), and how realms are kept apart (for example by country of the document). Until decided: one realm, or realms for disjoint groups of people. A person verified in two overlapping realms would hold two accounts.

### One claim per account
A later claim replaces the earlier one. Combining proofs of several services (document here, letter there) is the work of a service, off chain: it accepts the proof of another service and issues one claim. The chain stays simple and sees one statement.

### Taking back
- **The issuer** signs a revocation (tag `capita-revocation-v1`, audience, issuer, serial); anybody may hand it in. The account loses membership and level at once. A claim not yet presented can no longer be.
- **Expiry and struck-off issuers** need no transaction to take effect on the money: the rules stop counting an expired level by themselves, and `renew` refuses. The leftover membership is cleared away by anybody (`drop_invalid`).
- The account itself is never closed or emptied by any of this: it falls back to the unverified step.

### Duniter's pallets, unchanged
Wiring in `runtime/src/identity.rs`, through calls, public functions and the handler types named in ADR 0009.
- **Creating identities.** Duniter lets only a member create an identity (the first certifier). Here that member is an account nobody has a key for (`cap/cred`), identity 1 of the genesis block with a membership that never runs out. When a claim is accepted the runtime creates the identity in its name, confirms it in the name of the holder, and adds the membership – one transaction for the person. Nobody else can create identities (`CheckIdtyCallAllowed`); this closes the gap noted in ADR 0010.
- **Names.** Duniter's identities carry a unique public name. The holder may choose one; otherwise it is `0x` plus the first 20 bytes of the account. Chosen names may not start with `0x`. Offene Frage: whether names are wanted at all – a real name typed here is public for ever.
- **Membership** lasts one period (test chain: 365 days) and is renewed by the holder with the claim held (`renew`, no new signature) or by a new claim. When it runs out the level is withdrawn until renewed. It keeps the public number of members honest: an account nobody uses any more stops counting within a period.
- **Membership ended** (expired, claim taken back) → `do_attest(None)`. **Identity revoked** (by the holder with a signature of the key, or by root) → the claim is dropped and the account frozen (`do_set_status`): a revoked key should move no money.
- **No automatic revocation.** Duniter revokes an identity that stays without membership (one year). A revoked identity is final and its key cannot get another; the account would stay frozen. The period is set to 100 years.
- **No change of key.** Duniter can move an identity to a new key; the account and its money would stay with the old one. Refused (`OnKeyChange`). Offene Frage: B5 – a lost or stolen key: moving the account to a new key after a new verification (threat table of `architecture.md`, "Lost device").
- The credential pallet itself knows neither Duniter nor `pallet-capita`: two small traits (`Memberships`, `Rules`), implemented in the runtime. Another identity pallet, or none, is a change there.

### A lost key
A key is stored nowhere but with its holder. Whoever loses it opens an account with a new key, is verified again, and asks for the old account to move there (`credential.requestMove`, with the new claim). The move waits (`identity.key_move_delay` of the rules file, default 7 days); until it is completed the old key can stop it (`credential.stopMove`), and whoever asked can withdraw. Then anybody completes it (`credential.completeMove`): person, balance, what was accrued, step and seniority go to the new account (`capita_core::recovery`); the old account is closed, its Duniter identity removed – not revoked, nothing went wrong – and the new account becomes a member identity with the new claim. What is paid to the old account afterwards reaches the new one. A move whose claim counts no longer does not happen.
Offene Frage: 2discuss M – the waiting time, and who wins when the old key objects.

### Paying for the verification
Not enforced on chain. The start grant can be paid only to the verification service (`pay_verification`); an issuer sees the payment in the public events before it issues.

## Not in M3
- **An issuer.** The service that verifies people and signs claims: accepts a proof (OpenID Connect from GNU Taler Challenger for e-mail, phone, postal address; a verifiable credential, checked with SpruceID `ssi`; later an Idena-style test for people without documents), derives the pseudonym, signs. `ssi` belongs there, not into the runtime. For test chains: `cargo run -p capita-credential --example issue` signs anything with a key everybody knows.
- **The service host** (M4) uses `check_claim` unchanged; its audience is an identifier of the service.

## Consequences
+ The chain holds proof for every verified account: which service stated what, under which limits, and until when.
+ Services are replaceable and can be many; one compromised key is bounded by its limits and removed by one change of the list.
+ No personal data on chain.
− Public and linkable: issuer, level, country and attributes of an account, and the same pseudonym on every host. Zero-knowledge proofs would remove that.
− Uniqueness is as good as the issuers of a realm, and open across realms.
− The list is kept by one authority until governance exists.
− A claim is bound to a key. A lost key costs a new verification and the waiting time of the move.

## Open (technical)
Weights by benchmark (a signature check per claim) · `sp-io` and a library may judge malformed Ed25519 signatures differently: hosts must agree before two hosts serve one currency · in a block hook the block time is that of the previous block, so a level withdrawn because a membership ran out is withdrawn as of that time · P-256 (EUDI wallet, eIDAS) as a further signature scheme · serde form of signatures (JSON for the service host) · status of single attributes (a claim is replaced as a whole).
