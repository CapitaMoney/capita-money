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

//! Why a claim, a revocation or an entry of the issuer list is refused.

use core::fmt;

/// Reasons for refusing. None of them reveals anything about the person.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CredentialError {
    /// The issuer is suspended or revoked.
    IssuerNotActive,
    /// The claim names another issuer than the one it is checked against.
    WrongIssuer,
    /// No key of the issuer made this signature over these bytes – a wrong
    /// key, another host (audience), or content changed after signing.
    BadSignature,
    /// The claim is dated in the future.
    NotYetValid,
    /// The claim has expired.
    Expired,
    /// The claim ends before it begins.
    ExpiryBeforeIssue,
    /// The claim holds longer than the issuer may state, or without an end
    /// where the issuer needs one.
    ValidityTooLong,
    /// Verification level zero, or one above what the issuer may state.
    LevelNotAllowed,
    /// The issuer may not state a residence.
    ResidenceNotAllowed,
    /// The residence was proven after the claim was issued.
    ResidenceFromFuture,
    /// The issuer may not state this attribute.
    AttributeNotAllowed,
    /// More attributes than a claim may carry.
    TooManyAttributes,
    /// Not a decentralised identifier: `did:` method `:` identifier.
    InvalidDid,
    /// A `did:key` identifier that does not belong to the key.
    DidKeyMismatch,
    /// An issuer without a key, or with too many.
    InvalidKeys,
    /// More attribute keys than an issuer entry may hold.
    TooManyAttributeKeys,
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::IssuerNotActive => "issuer not active",
            Self::WrongIssuer => "claim names another issuer",
            Self::BadSignature => "signature does not match",
            Self::NotYetValid => "claim dated in the future",
            Self::Expired => "claim expired",
            Self::ExpiryBeforeIssue => "claim ends before it begins",
            Self::ValidityTooLong => "claim holds longer than the issuer may state",
            Self::LevelNotAllowed => "verification level not allowed for the issuer",
            Self::ResidenceNotAllowed => "issuer may not state a residence",
            Self::ResidenceFromFuture => "residence proven after the claim was issued",
            Self::AttributeNotAllowed => "issuer may not state this attribute",
            Self::TooManyAttributes => "too many attributes",
            Self::InvalidDid => "not a decentralised identifier",
            Self::DidKeyMismatch => "did:key does not belong to the key",
            Self::InvalidKeys => "issuer needs between one and eight keys",
            Self::TooManyAttributeKeys => "too many attribute keys",
        })
    }
}

impl core::error::Error for CredentialError {}
