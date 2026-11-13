# ADR 0009 – Chain host: `pallet-capita`

Status: accepted (M2, 2026-11-13). Pallet built and tested against a mock runtime. Runtime and node: ADR 0010.

## Context
M2: the same rules inside a Substrate runtime, later next to Duniter's identity pallets. ADR 0001 demands that the rules stay in `capita-core`.

## Decision
- **No rule in the pallet.** Every call loads state, calls a `plan_*` function of the core and applies the plan. The pallet adds only what a host supplies: storage, ledger, time, authority, consent.
- **Core types in storage.** `capita-core` gets an optional feature `scale` (SCALE encoding, type information). No mirrored types, nothing to keep in step.
- **Own ledger, not `pallet-balances`.** Balances are signed 64-bit cents; a balance may go below zero down to the credit line (ADR 0006). `pallet-balances` knows no negative balance. The ledger refuses on its own any debit below the line, whatever a plan says.
- **Time = block time** (`UnixTime`, in practice `pallet-timestamp`), whole seconds. Several calls in one block see the same time; the core allows that.
- **Authority = origins chosen by the runtime:** `GovernanceOrigin` (new rules, service account), `IdentityOrigin` (attest, freeze, close), `PurchaseOrigin` (mint purchased money), `CreditOrigin` (credit lines). Test chain: root. The answers to E1, F3, C1 go here, not into code.
- **Consent for loans:** the lender offers (`offer_loan`), the borrower accepts (`accept_loan`). Neither side can be bound by the other.
- **Start grant:** payable only to the service account named by governance (`pay_verification`).
- **Demurrage:** the pallet settles every account a call moves money on – including the lender a claim repays – before it asks for the plan (ADR 0007).
- **Atomic:** a call is one storage transaction. The operation identifiers of the core (replay protection for service hosts) are not needed and not stored.
- **Transparency:** every event of the core is deposited unchanged (`Event::Rule`); `PublicTotals` keeps their sums on chain; `TotalSupply` is counted separately by the ledger. `try_state` checks after every block: both supplies equal, sum of balances = supply, no balance below its line, no start grant touched, recorded debt = lent − repaid, one person one account.
- **Account identifiers are 32 bytes** (`AccountId32`), the form the core uses.
- **All versions of the rules are kept** (one unbounded storage value): a claim or a demurrage settlement may reach back across versions.
- **SDK line:** `frame-support` 46 – the version of Duniter's SDK fork (`duniter-polkadot-sdk`, branch `duniter-substrate-v1.22.3`). The pallet uses no API particular to the fork; its source is set by the workspace (ADR 0010).
- **An open account is a provider:** `open` registers one provider reference with the system pallet, so that the key exists for other pallets (ADR 0010).

## Duniter pallets: what couples them
Read from the source snapshot of 2026-10-30 (`duniter-v2s`):
- `pallet-identity` and `pallet-membership` depend on FRAME, `duniter-primitives` and `sp-membership` only – not on `pallet-certification`, `pallet-distance` or `pallet-duniter-wot`.
- The web of trust is attached through handler types of their `Config`: `CheckIdtyCallAllowed`, `OnNewIdty`, `OnRemoveIdty`, `CheckAccountWorthiness`, `OnKeyChange` (identity) and `CheckMembershipOpAllowed`, `OnNewMembership`, `OnRemoveMembership` (membership). Each has an implementation for `()`. Duniter's runtime sets them to `Wot` and its handlers.
- Hence: the web of trust can be left out. A credential pallet (M3) implements these handlers and tells `pallet-capita`: membership created or renewed → `do_attest` with a level valid until the membership expires; membership removed → `do_attest(None)`; identity revoked or removed → `do_set_status`.
- `pallet-universal-dividend` is not used: issuance comes from `capita-core`, and that pallet requires `pallet-balances`.
- The Duniter pallets are git dependencies on Duniter's SDK fork. A runtime that contains them must take the whole SDK from that fork (same version, different source).

## Consequences
+ One set of rules, tested once, on both hosts; the tests of the pallet repeat the invariants of the core on chain state.
− No `fungible` interface: pallets that expect one (transaction payment, treasury) cannot use Capita as it is. An adapter over the positive part of a balance is possible later.
− No transaction fees yet; protection against spam is open (Duniter: quota pallet).
− Weights are placeholders until benchmarked.

## Open (technical)
WebAssembly build of core and pallet (`wasm32v1-none`; not possible in the build environment used so far, see CI) · runtime API for balance and claimable amount · benchmarks · fees or quota.
