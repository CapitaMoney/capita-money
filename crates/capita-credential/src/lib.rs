// Capita Money – capita-credential
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

//! # capita-credential
//!
//! How [Capita Money](https://capitamoney.com) learns who is behind an
//! account: a trusted identity service checks a person – by e-mail, by an
//! identity document, by a letter to the home address – and signs a short
//! statement about the result. This crate defines that statement and checks
//! it. Design: `docs/adr/0011-identity-by-credentials.md`.
//!
//! - A [`types::Claim`] is the statement: which account, which person
//!   (a pseudonym, never a name), which verification level, optionally a
//!   residence and numbered attributes, and how long it holds. It carries no
//!   personal data; the verifiable credential it was derived from stays with
//!   the person.
//! - An [`types::Issuer`] is an entry of the public list of trusted services:
//!   a decentralised identifier (DID), the keys it signs with, and what it
//!   may state at most.
//! - [`check::check_claim`] decides whether a claim counts and turns it into
//!   the [`capita_core::attributes::Attestation`] the rules understand.
//!
//! Like `capita-core` the crate computes and nothing else: no storage, no
//! network, no clock. Signature schemes and the hash function come from the
//! host through [`check::Crypto`], so the same check runs inside a chain
//! runtime and next to a bank.
//!
//! ## The bytes an issuer signs
//!
//! Fixed by [`encoding`], independent of any serialization library, so that
//! an issuer written in any language can produce them. All integers are
//! big-endian.
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
pub mod did;
pub mod encoding;
pub mod errors;
pub mod types;
