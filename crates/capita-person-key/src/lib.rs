// Capita Money – capita-person-key
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

//! # capita-person-key
//!
//! One person, one account – so every identity service has to give the same
//! person the same identifier, year after year, and a different one to
//! everybody else, without the identifier telling anybody who the person
//! is. That identifier is the **person key** (the field `subject_ref` of a
//! claim, `capita-credential`). Design: `docs/adr/0013-person-key.md`.
//!
//! Two steps:
//! 1. [`canonical`] turns what was checked about a person – for example the
//!    name and the date of birth an identity document states – into one
//!    fixed sequence of bytes, the same whatever the spelling of the source.
//! 2. A [`KeyService`] applies a keyed function to those bytes. Without the
//!    secret nobody can compute the key of a person from a list of names
//!    and birthdays.
//!
//! The keyed function is the oblivious pseudorandom function of RFC 9497
//! (ristretto255, SHA-512). [`LocalKey`] computes it with a secret the
//! service holds itself. The same function can be computed by a separate
//! key service that never sees the data (blinded evaluation), or by several
//! services that each hold a share of the secret – with the same result, so
//! that moving from one arrangement to the next changes no person key.
//!
//! No storage, no network, no clock; `no_std` with `alloc`.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod canonical;
mod key;
pub mod translit;

#[cfg(feature = "local")]
pub use key::LocalKey;
pub use key::{namesake_tag, person_key, KeyError, KeyService};
