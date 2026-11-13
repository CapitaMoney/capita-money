// Capita Money – capita-core
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, version 3.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// for more details: <https://www.gnu.org/licenses/>.

//! # capita-core
//!
//! The monetary rules of [Capita Money](https://capitamoney.com) as one pure
//! library: equal issuance per person, interest-free lending, optional
//! demurrage.
//!
//! Properties (see `docs/adr/`):
//! - `no_std` (+ `alloc`), no I/O, no clock, no floats, no `unsafe`;
//! - integer cents with checked arithmetic – errors, never silent saturation;
//! - deterministic: same inputs, same result, on a server or inside a chain
//!   runtime;
//! - no rule is hard-coded: every number and policy is a field of
//!   [`params::Params`].
//!
//! The outside world is reached only through the traits in [`ports`].
//!
//! ## How to use it
//!
//! Every rule is a pure function `plan_*`: account state, parameters in
//! force and the time go in, a [`plan::Plan`] comes out – the new state, the
//! ledger operations and the public events. The host applies a plan as a
//! whole or not at all.
//!
//! | Module        | Rules |
//! |---------------|-------|
//! | [`lifecycle`] | open an account, verification, ladder step, freeze and close |
//! | [`issuance`]  | claim per-capita money; mint purchased money |
//! | [`loans`]     | payments, credit lines, recorded loans, repayment |
//! | [`demurrage`] | optional holding fee (off by default) |
//! | [`totals`]    | public sums over the event record |
//! | [`params`]    | versioned rules and their validation |

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod account;
pub mod attributes;
pub mod demurrage;
pub mod errors;
pub mod issuance;
pub mod lifecycle;
pub mod loans;
pub mod params;
pub mod plan;
pub mod ports;
pub mod totals;
pub mod types;
