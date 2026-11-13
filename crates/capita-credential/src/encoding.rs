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

//! The bytes an issuer signs, and the pseudonym of a person inside the
//! rules.
//!
//! One fixed layout, written out here field by field, so that an issuer in
//! any language can produce it without a serialization library. Integers
//! are big-endian. Every message starts with a tag that says what it is and
//! in which version, followed by the audience: a signature can serve one
//! purpose on one host only.
//!
//! ```text
//! claim       "capita-claim-v1"
//!             audience        32 bytes
//!             issuer           4 bytes
//!             serial          16 bytes
//!             subject         32 bytes   the account
//!             subject_ref     32 bytes   the pseudonym of the person
//!             level            1 byte
//!             residence        1 byte    0 = none, 1 = present; if present:
//!               country        2 bytes   ISO 3166-1 alpha-2, ASCII
//!               proven_at      8 bytes   seconds since the Unix epoch
//!             attributes       2 bytes   how many; for each:
//!               key            2 bytes
//!               value          8 bytes
//!             issued_at        8 bytes
//!             expires_at       1 byte    0 = none, 1 = present; if present:
//!               time           8 bytes
//!
//! revocation  "capita-revocation-v1"
//!             audience        32 bytes
//!             issuer           4 bytes
//!             serial          16 bytes
//!
//! person      "capita-person-v1"         hashed, see `person_bytes`
//!             realm            4 bytes
//!             subject_ref     32 bytes
//! ```

use alloc::vec::Vec;

use crate::types::{Audience, Claim, Realm, Revocation, SubjectRef};

/// Tag of a signed claim.
pub const CLAIM_TAG: &[u8] = b"capita-claim-v1";
/// Tag of a signed revocation.
pub const REVOCATION_TAG: &[u8] = b"capita-revocation-v1";
/// Tag of the bytes a person identifier is the hash of.
pub const PERSON_TAG: &[u8] = b"capita-person-v1";

/// The bytes an issuer signs to make `claim` count on the host `audience`.
///
/// Attributes beyond 65 535 are not encoded; a claim may carry far fewer
/// (`MAX_ATTRIBUTES`), and the check refuses a longer list before it looks
/// at the signature.
#[must_use]
pub fn claim_bytes(audience: &Audience, claim: &Claim) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(CLAIM_TAG);
    out.extend_from_slice(&audience.0);
    out.extend_from_slice(&claim.issuer.0.to_be_bytes());
    out.extend_from_slice(&claim.serial.0);
    out.extend_from_slice(&claim.subject.0);
    out.extend_from_slice(&claim.subject_ref.0);
    out.push(claim.level.0);
    match &claim.residence {
        None => out.push(0),
        Some(residence) => {
            out.push(1);
            out.extend_from_slice(&residence.country.0);
            out.extend_from_slice(&residence.proven_at.secs().to_be_bytes());
        }
    }
    let count = u16::try_from(claim.attributes.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&count.to_be_bytes());
    for attribute in claim.attributes.iter().take(usize::from(count)) {
        out.extend_from_slice(&attribute.key.0.to_be_bytes());
        out.extend_from_slice(&attribute.value.0.to_be_bytes());
    }
    out.extend_from_slice(&claim.issued_at.secs().to_be_bytes());
    match claim.expires_at {
        None => out.push(0),
        Some(end) => {
            out.push(1);
            out.extend_from_slice(&end.secs().to_be_bytes());
        }
    }
    out
}

/// The bytes an issuer signs to take a claim back on the host `audience`.
#[must_use]
pub fn revocation_bytes(audience: &Audience, revocation: &Revocation) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(REVOCATION_TAG);
    out.extend_from_slice(&audience.0);
    out.extend_from_slice(&revocation.issuer.0.to_be_bytes());
    out.extend_from_slice(&revocation.serial.0);
    out
}

/// The bytes whose hash is the identifier of a person inside the rules
/// (`capita_core::types::PersonId`): the same for every issuer of a realm,
/// different across realms.
#[must_use]
pub fn person_bytes(realm: Realm, subject_ref: &SubjectRef) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(PERSON_TAG);
    out.extend_from_slice(&realm.0.to_be_bytes());
    out.extend_from_slice(&subject_ref.0);
    out
}
