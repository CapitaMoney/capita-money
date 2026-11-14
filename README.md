# Capita Money

**An interest-free per-capita currency and universal basic income.**
Every verified person receives Capita continuously, at the same rate, worldwide.

Website: [capitamoney.com](https://capitamoney.com)

## Status
Rules as code with tests (M1): `capita-core`. The same rules inside a Substrate pallet (M2): `pallet-capita`. A local test chain around it (M2): `capita-runtime` and `capita-node`, with Duniter's identity and membership pallets, without its web of trust. Identity (M3): trusted identity services sign short claims about the holders of accounts; `capita-credential` checks them, `pallet-capita-credential` keeps the list of services and the claims on chain. Lost key (M3b): a person verifies again and the account moves to the new key. Person key (M3b): `capita-person-key` – one identifier per person, without personal data. Identity service (M3b): `capita-issuer` checks a person and signs the claim. Euro door (M4): `capita-euro-door` takes euros through a GNU Taler merchant backend and hands out verification tickets; Capita itself lives only on the chain ([ADR 0016](docs/adr/0016-chain-ledger-taler-euro-door.md)). Next: a check of identity documents. Rules are being finalized in parallel: every rule value stands in one file, [`rules/alpha.toml`](rules/alpha.toml), with its status – decided, default or example.

## Layout
- [`docs/architecture.md`](docs/architecture.md) – components, data flows, trust boundaries, threat model
- [`docs/adr/`](docs/adr/) – design decisions
- [`docs/upstream-mapping.md`](docs/upstream-mapping.md) – what is reused from which project
- [`rules/`](rules) – the rules as values: `alpha.toml` (the rules), `dev-fast.toml` (fast numbers for developers)
- [`crates/capita-core`](crates/capita-core) – the rules library (Rust, `no_std`)
- [`crates/capita-credential`](crates/capita-credential) – claims of identity services: format, signed bytes, check (Rust, `no_std`)
- [`crates/capita-person-key`](crates/capita-person-key) – the person key: canonical form of checked data, keyed function (Rust, `no_std`)
- [`services/issuer`](services/issuer) – the identity service: checks a person, signs the claim, forgets the data
- [`crates/capita-rulefile`](crates/capita-rulefile) – reads and checks a rules file
- [`crates/capita-rules`](crates/capita-rules) – the rules files compiled: what every host is built with (Rust, `no_std`)
- [`pallets/capita`](pallets/capita) – the rules inside a Substrate runtime (FRAME pallet)
- [`pallets/credential`](pallets/credential) – trusted identity services and their claims on chain (FRAME pallet)
- [`runtime`](runtime) – runtime of the test chain: the pallets, Duniter's identity and membership, consensus
- [`node`](node) – node of the test chain

## The core
`capita-core` holds every monetary rule and nothing else – no storage, no network, no clock:
- **Issuance:** every account accrues Capita by the second at the rate of its step on the incentive ladder, limited by a cap (global, later per country and seniority). Nothing is created until claimed; a claim reaches back a limited time.
- **No debt:** the alpha rules grant no credit line and record no loan; no balance goes below zero. (The core knows interest-free credit lines and loans; the rules file switches them off. The data model has no interest field.)
- **Lost key:** after a new verification an account moves to a new key – person, balance, what was accrued.
- **One way in:** Capita can be bought with ordinary money; there is no function for the way back.
- **Optional:** demurrage (a holding fee) and ripening (delayed availability) – both off by default.
- **Transparency:** public totals are sums over the event record and must equal the ledger.

Every rule is a pure function that returns a plan (new account state, ledger operations, public events); the host applies it. Integer cents, checked arithmetic, no floats, `no_std`.

## The rules file
The core takes every number as a parameter; [`rules/alpha.toml`](rules/alpha.toml) is where the numbers stand ([ADR 0012](docs/adr/0012-rules-file.md)). One small table per rule:
```toml
[issuance.start_grant]
value = "1.00"
status = "decided"        # decided · default · example
source = "diary 5"
note = "One Capita for every new account. It can only pay the account's own verification."
```
To change a rule, change the file and build: the chain starts with it, a wrong value stops the build with the name of the rule. Rules that are behaviour, not numbers, stand in the same file as a register with the code that holds them. The open rules, at a glance:
```
cargo run -p capita-rulefile --example overview -- rules/alpha.toml
```
The tests use fixed numbers of their own, so a change of rules breaks none of them; what must hold for any numbers is tested for every rules file.

## Identity
Who is a person is not decided by the chain and not by a web of trust: identity services check people – an e-mail address, an identity document, a letter to the home address – and sign what they found. `capita-credential` defines that statement and checks it ([ADR 0011](docs/adr/0011-identity-by-credentials.md)):
- **A claim** names an account, a person as a person key, a verification level, optionally a country of residence, and how long it holds. No name, no document, no date of birth.
- **The person key** is a keyed function of what a check established – by default name and date of birth – computed by the identity services ([ADR 0013](docs/adr/0013-person-key.md)). Without their secret nobody can tell whose key it is.
- **A public list** holds the trusted services: a decentralised identifier (DID), keys, and what each may state at most.
- **A claim counts** for one host, one account, once; a later one replaces it; its issuer can take it back.
- **One person, one account:** the rules refuse a second account for the same person key.
- **A lost key:** the person verifies again with a new account and asks for the move (`credential.requestMove`). The move waits – seven days by default – and the old key can stop it; then the old account is closed and everything is on the new one.

### The identity service
[`services/issuer`](services/issuer) is the service that issues claims ([ADR 0014](docs/adr/0014-identity-service.md)). It takes its rules from the rules file and its checks are exchangeable: a test check for test chains, a code by letter through GNU Taler Challenger; a check of identity documents is not built yet. After signing it keeps the person key, the account, the day and the kind of proof – nothing else.

[`services/euro-door`](services/euro-door) is the euro door ([ADR 0016](docs/adr/0016-chain-ledger-taler-euro-door.md)): a person or a sponsor pays for verifications through GNU Taler (`KUDOS` demo money for now) and gets one ticket code per verification; the identity service takes them from the same tickets file. The price comes from the rules file. Example: `services/euro-door/euro-door.example.toml`.
```
cargo test -p capita-issuer
cargo run -p capita-issuer -- services/issuer/issuer.example.toml        # after filling in host and secrets
cargo run -p capita-issuer -- services/issuer/issuer.example.toml key    # the key the chain lists for the service
```

On chain, `pallet-capita-credential` accepts a claim from the account it is made out to and makes that account a member identity (Duniter's pallets, unchanged) with the verification level stated.

## Build and test
```
# the crates without the Polkadot SDK (default members of the workspace)
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all --check
# the pallets (build the Polkadot SDK, take longer)
cargo test -p pallet-capita -p pallet-capita-credential
cargo clippy -p pallet-capita -p pallet-capita-credential --all-targets -- -D warnings
# no_std check on a target without an operating system
rustup target add thumbv7em-none-eabi
cargo build -p capita-core -p capita-credential --all-features --target thumbv7em-none-eabi
```
Tests: [`tests/rules.rs`](crates/capita-core/tests/rules.rs) – worked examples with numbers for every rule; [`tests/properties.rs`](crates/capita-core/tests/properties.rs) – invariants on random inputs (claiming often = claiming once; no balance below its credit line; supply = minted − burnt; no overflow at extreme values).

The pallet decides nothing itself: every call asks the core for a plan and applies it. It adds storage, a ledger with credit lines, block time, the origins that hold authority, and consent for loans ([ADR 0009](docs/adr/0009-chain-host-pallet.md)). Its tests run the rules through a mock runtime and check the invariants on chain state after every call of random histories.

## The test chain
A chain for trying things out on one computer: no fees, one key with all authority (sudo) ([ADR 0010](docs/adr/0010-test-chain.md)). Started with `--dev` it runs on the fast numbers of `rules/dev-fast.toml` (1 cent per minute at level 1); started with `--chain local` on the rules of `rules/alpha.toml`.

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
The node listens on `ws://127.0.0.1:9944`; [Polkadot-JS Apps](https://polkadot.js.org/apps/?rpc=ws://127.0.0.1:9944) can connect to it. Alice holds the sudo key. A first walk:
1. Any key calls `capita.open`.
2. Alice names a service account (`sudo` → `capita.setServiceAccount`); the key calls `capita.payVerification`.
3. The identity service of the test chain signs a claim for the key. Its secret key is public, so anybody can play the service:
   ```
   cargo run -p capita-credential --example issue -- \
       --audience 0x<hash of block 0> --subject 0x<the key, 32 bytes> --level 1
   ```
   In Polkadot-JS Apps: the hash under Developer → RPC calls → `chain.getBlockHash(0)`, the key in hexadecimal under Developer → Utilities → Convert address.
4. The key sends what the tool printed: `credential.present`. It is now a member identity with verification level 1.
5. The key chooses its step (`capita.setTier`) and claims (`capita.claim`).

Root can still attest by hand (`sudo` → `capita.attest`), and keeps the list of identity services (`credential.registerIssuer`, `credential.updateIssuer`).

## Approach
- **Modular:** all monetary rules live in one small Rust library (`capita-core`). The same rules can run on a central ledger or on a peer-to-peer blockchain.
- **Built on existing open source** instead of starting from scratch:
  - [Duniter v2](https://git.duniter.org/nodes/rust/duniter-v2s): Substrate modules for identity and membership, and its maintained fork of the Polkadot SDK
  - [Ğecko](https://git.duniter.org/clients/gecko): mobile wallet (Flutter)
  - [GNU Taler](https://taler.net): privacy-friendly payments and shop integration
  - [Polkadot SDK / Substrate](https://github.com/paritytech/polkadot-sdk): blockchain framework
  - [SpruceID ssi](https://github.com/spruceid/ssi): decentralised identifiers and verifiable credentials, for the identity services that issue claims
- **No interest, no exchange into money:** Capita is meant for people, not for speculation.

## Contributing
Developers are welcome. Open an issue or a discussion to get involved.

## Licence
[GNU Affero General Public License v3.0](LICENSE)
