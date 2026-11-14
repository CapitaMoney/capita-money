# ADR 0004 – Start on GNU Taler, keep the data model portable

Status: superseded by ADR 0016 (2026-11-14): the chain is the ledger, GNU Taler only the euro door. Kept for the reasoning and the portable data model, which stays.

## Context
A blockchain gives rules nobody can change alone and a public ledger, but needs independent validators, runtime upgrades and key handling that loses money when a seed phrase is lost. A first community needs one-tap install and an account in two minutes.

## Decision
- First ledger: libeufin-bank (GNU Taler), own currency, conversion off, issuance from a non-admin `centralbank` account with a debit limit (advice of the Taler maintainers).
- Second ledger, built in parallel as proof of portability (M2): Substrate solo chain with Duniter pallets and the same core.
- Portable data model, so that moving ledgers is a data migration, not a redesign:
  - identifiers are 32 opaque bytes (`AccountId`, `PersonId`);
  - operations carry an `OpId` (account + sequence number) – a bank request id on Taler, a nonce on a chain;
  - the event record is the history; any ledger state can be rebuilt from it;
  - all types are plain integers and byte arrays; optional `serde` now, SCALE codec feature in M2.

## Consequences
+ Working payments, shop plugin and wallets from day one. + No validators needed to start.
− One operator must be trusted at first; softened by open source, public events and totals, and the Taler auditor.
− Rule perimeter on Taler: rules apply to bank accounts. Coins withdrawn into a wallet are private cash – demurrage and ripening cannot reach them. If B2 or A1d are switched on, M4 must answer this (for example coin expiry and refresh fees).
− Check against a running bank in M4: idempotency of transactions (`request_uid` of `POST /accounts/$USERNAME/transactions`, per Core Bank API documentation); exact amount limits.
