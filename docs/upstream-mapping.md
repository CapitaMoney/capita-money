# Upstream mapping – what we reuse, and how

Rule: ideas are reimplemented; code is copied only with licence notice (ADR 0005). As of M1 **no upstream code is in this repository**.
Licences marked ✓ were read from the `LICENSE` file and manifest of the source snapshot of 2026-11-11. Marked ✓web: checked against Taler/Parity project pages on 2026-11-13 (not the licence files themselves); re-check the `COPYING` files before any code reuse. Unmarked = unverified.

| Upstream | What | Used where | How | Licence |
|---|---|---|---|---|
| Duniter v2 `pallet-universal-dividend` | claim-based dividend: nothing written until the member claims | `issuance`, ADR 0002 | idea | AGPL-3.0 ✓ |
| Duniter v2 `identity`, `membership` pallets | identity life cycle on chain | chain host (M2) | code, as dependency or fork; certification/distance replaced by credential pallet | AGPL-3.0 ✓ |
| Duniter v2 dividend formula (re-evaluation by money mass and members) | – | not used: our rates come from the parameter ladder and caps | – | – |
| encointer `balances` | demurrage computed lazily from balance + last update | `demurrage`, ADR 0007: linear integer formula instead of exponential fixed-point | idea | GPL-3.0-or-later ✓ |
| encointer ceremonies | personhood by physical meetings | not used (remote newcomers) | – | – |
| Circles | retroactive claim limited to a window (14 days) | `claim_window`, ADR 0002 | idea | AGPL-3.0 ✓ (contracts v2) |
| GNU Taler libeufin-bank | account ledger, debit limits, registration bonus | `Ledger` adapter (M4) | used as a service over its API. Idempotency: `request_uid` field on bank transactions (since 2024); same uid with different details is rejected. Check at M4 whether it is mandatory by now | AGPL-3.0-or-later ✓web |
| GNU Taler Challenger | proof of e-mail, phone, postal address | `Identity` adapter (M3/M4) | used as a service | AGPL-3.0-or-later ✓web |
| GNU Taler exchange, merchant, wallet | payments, shop | M4, M5 | used as services; wallet fork later | exchange AGPL-3.0-or-later; merchant backend AGPL, merchant library LGPL, frontend GPL; wallet GPL-3.0-or-later, some parts LGPL ✓web |
| Polkadot SDK (FRAME) | runtime framework, solo-chain template | chain host (M2) | dependency | template: MIT-0 / Unlicense ✓; Substrate GPL-3.0 with classpath exception ✓web; other SDK crates (Apache-2.0?) unverified |
| SpruceID `ssi` | DID and verifiable credentials | `Identity` adapter (M3) | dependency | Apache-2.0 ✓ |
| Ğecko | Flutter wallet | app (M5) | fork | AGPL-3.0 ✓ |

Source locations: Duniter https://git.duniter.org/nodes/rust/duniter-v2s · Substrate https://github.com/paritytech/polkadot-sdk · ssi https://github.com/spruceid/ssi · Taler https://git.taler.net
