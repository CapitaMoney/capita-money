# ADR 0010 – Test chain: runtime and node

Status: accepted (M2, 2026-11-13). Runtime built and tested natively, node type-checked. The WebAssembly build and a running node are checked by CI only (see "Checked how").

## Context
M2: a local chain on which the rules run next to Duniter's identity pallets. `pallet-capita` exists (ADR 0009). Starting point: the solo-chain template of the Polkadot SDK.

## Decision
- **Composition only.** `runtime/` configures pallets, `node/` runs the runtime. No rule and no ledger logic in either.
- **Pallets:** system, timestamp, Aura (block production), GRANDPA (finality), sudo, `pallet-capita`, Duniter's `pallet-identity` and `pallet-membership`. Not included: `pallet-balances`, transaction payment, Duniter's web of trust (certification, distance, `duniter-wot`), universal dividend.
- **One source for the SDK: Duniter's fork** (`duniter-polkadot-sdk`, branch `duniter-substrate-v1.22.3`). Duniter's pallets are built against it, and a runtime can hold one copy of the SDK only. Every outside source of the chain side stands once, in `[workspace.dependencies]` of the root `Cargo.toml`; `Cargo.lock` fixes the exact commits. `capita-core` is not affected. `pallet-capita` uses nothing particular to the fork: replacing that block by crates.io versions (release line of `frame-support` 46) builds it without Duniter.
- **Duniter's pallets as a dependency, unchanged,** from the GitHub mirror of `duniter-v2s` at a fixed commit. Its two fixes for dependencies of the SDK are taken over through `[patch]`: `multihash` without the withdrawn crate `core2` (RUSTSEC-2026-0105), tracing with `tracing-subscriber` 0.3.20 (RUSTSEC-2025-0055).
- **No fees.** Transaction payment needs a currency with the `fungible` interface; the ledger of `pallet-capita` is none (ADR 0009). Whether transactions cost Capita at all is a rule question.
- **Own nonce check** (`runtime/src/nonce.rs`). The check of the SDK refuses a sender for whom nobody has paid storage; without fees and deposits that is every new key, which could then never open its account. Ours keeps the replay protection and drops that condition. Same name and encoding, so wallets need no change.
- **An open Capita account makes a key exist.** `open` registers a provider with the system pallet. Other pallets ask there: Duniter's identity pallet creates an identity only for an existing account. Order for a person: open the account, then receive an identity.
- **Authority = root through the sudo key** for all four origins of `pallet-capita` (governance, identity, purchase, credit). E1, F3 and C1 stay open.
- **Duniter's pallets without a web of trust.** Their handlers are set to `()` (allow everything, do nothing), except one small type, `NoWebOfTrust`, which keeps the two pallets in step as Duniter's runtime does: membership created → identity becomes member; membership removed → identity is no member; identity revoked or removed → membership removed. No call grants a membership yet: members come from the genesis block. The credential pallet (M3) will grant them and report to `pallet-capita` through the same handlers.
- **Genesis presets** `development` (one node, Alice) and `local_testnet` (Alice, Bob): sudo key Alice, the block authors as member identities, a first version of the rules. Every number in it is an example.
- **Node:** the template without benchmarking commands and without the fee interface for wallets. Database ParityDB; RocksDB behind the feature `rocksdb` (C++ build, needs libclang).

## Consequences
+ The whole chain side changes its sources in one place; a fork of our own under `CapitaMoney` is a change of two addresses.
+ The Duniter pallets stay unmodified: updates are a new commit hash.
− The SDK comes from a branch Duniter maintains. The lockfile fixes the commit; an update is a deliberate step, and its changes should be read.
− No protection against spam: any key may send transactions and write a nonce to the state. Before any public network: fees in Capita or a quota per identity (Duniter: `pallet-quota`).
− Any identity may create further identities, and nothing limits who is a member. Harmless while membership grants nothing on the Capita side; M3 closes it.
− The sudo key can do everything, including replacing the runtime.
− Building needs the Protocol Buffers compiler (`protoc`) and the target `wasm32v1-none`; a first build takes long.

## Checked how
- Native tests of the runtime (`runtime/src/tests.rs`): both presets build a state on which the invariants of `pallet-capita` hold; signed transactions pass every check of the chain, run once, and are refused when replayed, early or altered; the rules from opening an account to claim and payment; identity and membership in step.
- Not possible in the build environment used so far: the runtime as WebAssembly, linking the node, a running chain. CI builds the node and lets it produce blocks.

## Open (technical)
Runtime interface for balance and claimable amount · weights by benchmark · fees or quota · own address prefix (now 42, generic) · symbol and decimals in the chain specification · credential pallet (M3).
