# ADR 0001 – Rules in one pure core

Status: accepted (M0, 2026-11-13)

## Context
The rules must run on a central ledger now and inside a blockchain runtime later, and must stay checkable by outsiders. Rules are still moving.

## Decision
- One Rust library `capita-core`: `#![no_std]` + `alloc`, no I/O, no clock, no randomness, no floats, `unsafe` forbidden.
- Functions are pure: inputs in, plan out (new state, ledger operations, events). Hosts apply plans (architecture, section 3).
- The outside world only through ports (traits). Synchronous traits; asynchronous hosts execute plans themselves.
- No rule hard-coded: every number and policy is a field of `Params`.
- Extra port beyond ledger, identity, governance and events: `AccountStore`. Reason: tier, anchor and loan state are rule data, not balances; no ledger stores them.
- Enforced by lints in the workspace manifest (`arithmetic_side_effects`, `float_arithmetic`, casts, `unwrap`, `panic`, indexing).

## Consequences
+ Same code and tests for Taler service and Substrate pallet. + Deterministic, easy to property-test. + Ledger and identity provider replaceable.
− `alloc` needed (lists in parameters); a pallet wraps them in bounded types. − Hosts carry the atomicity work (outbox on the service host).

## Alternatives
Rules inside the Taler service only: fast, but a rewrite for any chain. Rules inside a pallet only: ties the start to a blockchain (ADR 0004).
