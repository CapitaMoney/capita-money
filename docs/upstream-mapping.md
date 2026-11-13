# Upstream mapping – what we reuse, and how

Rule: ideas are reimplemented; code is copied only with licence notice (ADR 0005). As of M2 the repository holds upstream code in two places, each with a notice in the file: `runtime/` and `node/` start from the solo-chain template of the Polkadot SDK (public domain), and `runtime/src/nonce.rs` adapts `frame_system::CheckNonce` (Apache-2.0). Everything else upstream is a dependency: the Polkadot SDK from Duniter's fork, Duniter's pallets from its repository (ADR 0010).
Licences marked ✓ were read from the `LICENSE` file and manifest of the source snapshot of 2026-11-11. Marked ✓web: checked against Taler/Parity project pages on 2026-11-13 (not the licence files themselves); re-check the `COPYING` files before any code reuse. Unmarked = unverified.

| Upstream | What | Used where | How | Licence |
|---|---|---|---|---|
| Duniter v2 `pallet-universal-dividend` | claim-based dividend: nothing written until the member claims | `issuance`, ADR 0002 | idea | AGPL-3.0 ✓ |
| Duniter v2 `identity`, `membership` pallets | identity life cycle on chain | `runtime` (M2), steered by `pallet-capita-credential` (M3) | code, as dependency at a fixed commit, unchanged: coupled to the web of trust only through `Config` handlers (ADR 0009). Certification and distance are replaced by claims of identity services; the handlers are implemented in `runtime/src/identity.rs` (ADR 0011) | AGPL-3.0 ✓ |
| Duniter v2 patches `multihash`, `sdk-tracing` | fixes for two dependencies of the SDK | workspace `[patch]` | code, as dependency, unchanged | MIT (multihash) ✓, Apache-2.0 / GPL-3.0 (tracing) ✓ |
| Duniter `duniter-polkadot-sdk` | Duniter's fork of the Polkadot SDK | source of every SDK crate of the chain side | dependency (branch `duniter-substrate-v1.22.3`, commit fixed by `Cargo.lock`) | as Polkadot SDK |
| Duniter v2 dividend formula (re-evaluation by money mass and members) | – | not used: our rates come from the parameter ladder and caps | – | – |
| encointer `balances` | demurrage computed lazily from balance + last update | `demurrage`, ADR 0007: linear integer formula instead of exponential fixed-point | idea | GPL-3.0-or-later ✓ |
| encointer ceremonies | personhood by physical meetings | not used (remote newcomers) | – | – |
| Circles | retroactive claim limited to a window (14 days) | `claim_window`, ADR 0002 | idea | AGPL-3.0 ✓ (contracts v2) |
| GNU Taler libeufin-bank | account ledger, debit limits, registration bonus | `Ledger` adapter (M4) | used as a service over its API. Idempotency: `request_uid` field on bank transactions (since 2024); same uid with different details is rejected. Check at M4 whether it is mandatory by now | AGPL-3.0-or-later ✓web |
| GNU Taler Challenger | proof of e-mail, phone, postal address | input of an identity service (issuer, after M3) | used as a service | AGPL-3.0-or-later ✓web |
| GNU Taler exchange, merchant, wallet | payments, shop | M4, M5 | used as services; wallet fork later | exchange AGPL-3.0-or-later; merchant backend AGPL, merchant library LGPL, frontend GPL; wallet GPL-3.0-or-later, some parts LGPL ✓web |
| Polkadot SDK (FRAME) | runtime framework, solo-chain template, nonce check | chain host (M2) | dependency; template and nonce check copied and changed, with notice | template: MIT-0 / Unlicense ✓; Substrate GPL-3.0 with classpath exception ✓web; other SDK crates (Apache-2.0?) unverified |
| SpruceID `ssi` | DID and verifiable credentials | identity service (issuer, after M3): checks the credentials people bring. Not in the runtime – the chain checks short claims with `capita-credential` (ADR 0011) | dependency | Apache-2.0 ✓ |
| W3C DID Core, `did:key` | identifiers of identity services | `capita-credential::did` | specification, reimplemented (syntax check, `did:key` of Ed25519 and Sr25519 keys) | W3C document licence, unverified |
| Ğecko | Flutter wallet | app (M5) | fork | AGPL-3.0 ✓ |

Source locations: Duniter https://git.duniter.org/nodes/rust/duniter-v2s (mirror used as dependency: https://github.com/duniter/duniter-v2s) · Duniter's SDK fork https://github.com/duniter/duniter-polkadot-sdk · Substrate https://github.com/paritytech/polkadot-sdk · ssi https://github.com/spruceid/ssi · Taler https://git.taler.net
