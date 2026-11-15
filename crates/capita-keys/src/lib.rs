// Capita Money – capita-keys
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

//! # capita-keys
//!
//! The keys that act for an account of [Capita Money](https://capitamoney.com)
//! (`docs/adr/0020-keys-passkeys-linked-devices.md`):
//!
//! - a **passkey** – a WebAuthn credential on the curve P-256, kept by the
//!   phone, unlocked by fingerprint, face or screen lock;
//! - a **device key** – an Ed25519 key the app makes itself where no passkey
//!   is possible.
//!
//! The account of a person who starts in the app is derived from the first
//! key ([`encoding::account_of`]); every action is a signature of one of the
//! account's keys over a challenge ([`encoding::act_challenge`]).
//! [`check::verify_proof`] checks such a signature – for a passkey with
//! everything WebAuthn puts around it: relying party, user verification,
//! type, challenge, origin.
//!
//! Like `capita-core` the crate computes and nothing else: no storage, no
//! network, no clock. Ed25519 comes from the host ([`check::Verify`]); P-256
//! is checked here (RustCrypto `p256`), because hosts have no function for
//! it.
//!
//! The app (`app/chain/keys.js`) makes the same bytes; both test against
//! `tests/vectors.json`.
//!
//! ## Features
//!
//! - `scale`: SCALE encoding and type information (Substrate runtime).
//! - `std`: forwards `std` to the dependencies.

#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(feature = "scale", allow(clippy::cast_possible_truncation))]

extern crate alloc;

pub mod check;
pub mod encoding;
pub mod types;
