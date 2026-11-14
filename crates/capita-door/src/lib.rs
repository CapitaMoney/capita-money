// Capita Money – capita-door
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

//! # capita-door
//!
//! The euro door of [Capita Money](https://capitamoney.com): the one way
//! from ordinary money into Capita (diary 3: euros → Capita, never back).
//! A door service takes a payment – through GNU Taler – and signs one of
//! two short statements about it. Design: `docs/adr/0018-euro-door-chain-mints.md`.
//!
//! - A [`types::Voucher`]: "this payment bought this much Capita for this
//!   account". The chain mints the amount once per payment
//!   (`pallet-capita-door`). Bought Capita is recorded apart from issued
//!   Capita, out of sight (`pallet-capita`, storage `Backed`).
//! - A [`types::Ticket`]: "this payment paid one verification". An identity
//!   service accepts each ticket once (`capita-issuer`); a sponsor buys
//!   tickets and passes them on (diary 6).
//!
//! Like `capita-core` and `capita-credential` the crate computes and nothing
//! else: no storage, no network, no clock. The signature check comes from
//! the host through [`check::Verify`].
//!
//! ## The bytes a door signs
//!
//! Fixed by [`encoding`], independent of any serialization library. All
//! integers are big-endian. Every message starts with a tag and the
//! audience (the hash of the first block of the chain): a signature counts
//! for one purpose on one host only.
//!
//! ## Features
//!
//! - `serde`: serialization of the data types.
//! - `scale`: SCALE encoding and type information (Substrate runtime).
//! - `std`: forwards `std` to those optional dependencies.

#![no_std]
#![forbid(unsafe_code)]
// The SCALE derive writes enum indices with an `as u8` cast. Hand-written
// code is still held to the lint: it is checked without the `scale` feature.
#![cfg_attr(feature = "scale", allow(clippy::cast_possible_truncation))]

extern crate alloc;

pub mod check;
pub mod encoding;
pub mod types;
