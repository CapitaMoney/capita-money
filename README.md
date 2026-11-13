# Capita Money

**An interest-free per-capita currency and universal basic income.**
Every verified person receives Capita continuously, at the same rate, worldwide.

Website: [capitamoney.com](https://capitamoney.com)

## Status
Rules as code with tests (M1): `capita-core`. The same rules inside a Substrate pallet (M2): `pallet-capita`. A local test chain around it (M2): `capita-runtime` and `capita-node`, with Duniter's identity and membership pallets, without its web of trust. Next: identity by verifiable credentials (M3). Rules are being finalized in parallel; anything undecided is a parameter, and every number on the test chain is an example.

## Layout
- [`docs/architecture.md`](docs/architecture.md) – components, data flows, trust boundaries, threat model
- [`docs/adr/`](docs/adr/) – design decisions
- [`docs/upstream-mapping.md`](docs/upstream-mapping.md) – what is reused from which project
- [`crates/capita-core`](crates/capita-core) – the rules library (Rust, `no_std`)
- [`pallets/capita`](pallets/capita) – the rules inside a Substrate runtime (FRAME pallet)
- [`runtime`](runtime) – runtime of the test chain: the pallet, Duniter's identity and membership, consensus
- [`node`](node) – node of the test chain

## The core
`capita-core` holds every monetary rule and nothing else – no storage, no network, no clock:
- **Issuance:** every account accrues Capita by the second at the rate of its step on the incentive ladder, limited by a cap (global, later per country and seniority). Nothing is created until claimed; a claim reaches back a limited time.
- **Lending:** interest-free credit lines and loans; a share of each claim repays the lender. The data model has no interest field.
- **One way in:** Capita can be bought with ordinary money; there is no function for the way back.
- **Optional:** demurrage (a holding fee) and ripening (delayed availability) – both off by default.
- **Transparency:** public totals are sums over the event record and must equal the ledger.

Every rule is a pure function that returns a plan (new account state, ledger operations, public events); the host applies it. Integer cents, checked arithmetic, no floats, `no_std`.

## Build and test
```
# the core (default member of the workspace)
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
# the pallet (builds the Polkadot SDK, takes longer)
cargo test -p pallet-capita
cargo clippy -p pallet-capita --all-targets -- -D warnings
# no_std check on a target without an operating system
rustup target add thumbv7em-none-eabi
cargo build -p capita-core --all-features --target thumbv7em-none-eabi
```
Tests: [`tests/rules.rs`](crates/capita-core/tests/rules.rs) – worked examples with numbers for every rule; [`tests/properties.rs`](crates/capita-core/tests/properties.rs) – invariants on random inputs (claiming often = claiming once; no balance below its credit line; supply = minted − burnt; no overflow at extreme values).

The pallet decides nothing itself: every call asks the core for a plan and applies it. It adds storage, a ledger with credit lines, block time, the origins that hold authority, and consent for loans ([ADR 0009](docs/adr/0009-chain-host-pallet.md)). Its tests run the rules through a mock runtime and check the invariants on chain state after every call of random histories.

## The test chain
A chain for trying things out on one computer: no fees, one key with all authority (sudo), example rules ([ADR 0010](docs/adr/0010-test-chain.md)).

Needed: Rust (stable) with the target `wasm32v1-none`, and the Protocol Buffers compiler.
```
rustup target add wasm32v1-none
sudo apt install protobuf-compiler        # Debian, Ubuntu
# tests of the runtime, without the WebAssembly build
SKIP_WASM_BUILD=1 cargo test -p capita-runtime --features try-runtime
# build the node (long the first time) and start a chain that forgets everything on exit
cargo build --release -p capita-node
./target/release/capita-node --dev
```
The node listens on `ws://127.0.0.1:9944`; [Polkadot-JS Apps](https://polkadot.js.org/apps/?rpc=ws://127.0.0.1:9944) can connect to it. Alice holds the sudo key. A first walk: any key calls `capita.open`; Alice names a service account (`sudo` → `capita.setServiceAccount`); the key calls `capita.payVerification`; Alice attests a level (`sudo` → `capita.attest`); the key chooses its step (`capita.setTier`) and claims (`capita.claim`).

## Approach
- **Modular:** all monetary rules live in one small Rust library (`capita-core`). The same rules can run on a central ledger or on a peer-to-peer blockchain.
- **Built on existing open source** instead of starting from scratch:
  - [Duniter v2](https://git.duniter.org/nodes/rust/duniter-v2s): Substrate modules for identity and membership, and its maintained fork of the Polkadot SDK
  - [Ğecko](https://git.duniter.org/clients/gecko): mobile wallet (Flutter)
  - [GNU Taler](https://taler.net): privacy-friendly payments and shop integration
  - [Polkadot SDK / Substrate](https://github.com/paritytech/polkadot-sdk): blockchain framework
  - [SpruceID ssi](https://github.com/spruceid/ssi): decentralised identifiers and verifiable credentials
- **No interest, no exchange into money:** Capita is meant for people, not for speculation.

## Contributing
Developers are welcome. Open an issue or a discussion to get involved.

## Licence
[GNU Affero General Public License v3.0](LICENSE)
