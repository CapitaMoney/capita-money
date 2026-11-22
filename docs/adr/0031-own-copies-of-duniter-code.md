# ADR 0031 – Own copies of the Duniter code

Status: accepted (owner 2026-11-22). Replaces the dependency source in ADR 0010.

## Context
The chain used Duniter's code straight from Duniter's GitHub repositories: seven crates of `duniter-v2s` at a fixed commit and 41 direct SDK dependencies from a branch of `duniter-polkadot-sdk`. On 2026-11-22 a Duniter developer answered our forum post: fork the pallets and maintain the fork, do not depend on the Duniter repository, and do not contribute back, because Duniter wants to stay free to change or remove this code. Conditions: respect the AGPLv3, credit the Duniter project, use a different name for our software.

## Decision
- **`CapitaMoney/capita-chain-pallets`** is our fork of `duniter-v2s`. Branch `capita` starts at Duniter commit `71884a4`. It changes only the address of the SDK and adds `CAPITA-FORK.md`. No Rust file differs.
- **`CapitaMoney/capita-polkadot-sdk`** is an unchanged copy of Duniter's SDK fork. We use commit `4fd8081` (tag `capita-base`), now fixed in `Cargo.toml` and not only in `Cargo.lock`.
- Both are referenced by commit. We take Duniter updates only when we decide to.
- We send no merge requests to Duniter.

## Consequences
+ Capita builds even if Duniter moves, rewrites or deletes its code.
+ We can always give users the complete source (AGPL section 13, ADR 0005).
+ The resolved code is the same as before: `Cargo.lock` differs only in the two addresses and the fork's commit.
- Security fixes in this code are now ours to notice and apply.
- Internal crate names such as `duniter-primitives` stay, so the origin remains visible and the code unchanged.
