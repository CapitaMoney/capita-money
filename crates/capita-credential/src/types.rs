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

//! The data model: issuers, claims, revocations.

use alloc::vec::Vec;

use capita_core::attributes::{AttrKey, Attribute, Residence, VerificationLevel};
use capita_core::types::{AccountId, Duration, Timestamp};

/// Most attributes one claim may carry.
pub const MAX_ATTRIBUTES: usize = 32;
/// Most keys one issuer may sign with at a time (room for rotation).
pub const MAX_KEYS: usize = 8;
/// Most attribute keys one issuer entry may list.
pub const MAX_ATTRIBUTE_KEYS: usize = 64;
/// Longest decentralised identifier of an issuer, in bytes.
pub const MAX_DID_LEN: usize = 256;

macro_rules! data {
    ($(#[$meta:meta])* $vis:vis struct $name:ident $body:tt $(;)?) => {
        $(#[$meta])*
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[cfg_attr(
            feature = "scale",
            derive(
                codec::Encode,
                codec::Decode,
                codec::DecodeWithMemTracking,
                scale_info::TypeInfo
            )
        )]
        $vis struct $name $body
    };
}

macro_rules! small {
    ($(#[$meta:meta])* $vis:vis struct $name:ident ($inner:ty);) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[cfg_attr(
            feature = "scale",
            derive(
                codec::Encode,
                codec::Decode,
                codec::DecodeWithMemTracking,
                codec::MaxEncodedLen,
                scale_info::TypeInfo
            )
        )]
        $vis struct $name(pub $inner);
    };
}

small! {
    /// Number of an issuer on the list of the host.
    pub struct IssuerId(u32);
}

small! {
    /// Number of a claim, unique per issuer and chosen by it (for example a
    /// random UUID). Revocation refers to it.
    pub struct Serial([u8; 16]);
}

small! {
    /// The host a claim is made out to: the genesis hash of a chain, or the
    /// identifier of a service. A claim for one host counts on no other.
    pub struct Audience([u8; 32]);
}

small! {
    /// A group of issuers that give the same person the same person key:
    /// they share its scheme and its secret (`docs/adr/0013-person-key.md`).
    /// Within a realm one person is one person; across realms nothing links
    /// two person keys.
    ///
    /// Offene Frage: F3 – how realms are kept from overlapping.
    pub struct Realm(u32);
}

small! {
    /// The person key an issuer gives a person: the same for every claim
    /// about that person, different for any other. Never a name or a
    /// document number, but a keyed hash of such data (`capita-person-key`,
    /// `docs/adr/0013-person-key.md`): a pseudonym.
    pub struct SubjectRef([u8; 32]);
}

/// A public key an issuer signs with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub enum PublicKey {
    /// Ed25519, the scheme most credential software signs with.
    Ed25519([u8; 32]),
    /// Sr25519 (Schnorr over Ristretto), the scheme of Substrate accounts.
    Sr25519([u8; 32]),
}

/// A signature of an issuer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub enum Signature {
    /// Made with an [`PublicKey::Ed25519`] key.
    Ed25519([u8; 64]),
    /// Made with an [`PublicKey::Sr25519`] key.
    Sr25519([u8; 64]),
}

/// Whether an issuer is trusted at the moment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub enum IssuerStatus {
    /// New claims count.
    #[default]
    Active,
    /// No new claims; those already accepted keep counting.
    Suspended,
    /// No new claims, and those already accepted count no longer.
    Revoked,
}

data! {
    /// One trusted identity service, as the public list of a host holds it.
    ///
    /// The limits say what the service is trusted for: a service that
    /// checks e-mail addresses may state a low level only, one that reads
    /// identity documents a higher one, one that sends letters a residence.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Issuer {
        /// Decentralised identifier of the service, for example
        /// `did:web:id.example.org`. Tells people and software where to
        /// find the service; the host trusts the keys below.
        pub did: Vec<u8>,
        /// The keys the service signs with. More than one while a key is
        /// being replaced.
        pub keys: Vec<PublicKey>,
        /// The realm of its pseudonyms.
        pub realm: Realm,
        /// Highest verification level it may state.
        pub max_level: VerificationLevel,
        /// Whether it may state a residence.
        pub residence: bool,
        /// The attributes it may state.
        pub attributes: Vec<AttrKey>,
        /// Longest time a claim of it may hold. `None`: no limit, and
        /// claims without an end are allowed.
        pub max_validity: Option<Duration>,
        /// Whether it is trusted at the moment.
        pub status: IssuerStatus,
    }
}

data! {
    /// What an issuer states about the holder of an account. Derived facts
    /// only: no name, no document, no date of birth.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Claim {
        /// Who states it.
        pub issuer: IssuerId,
        /// Number of the claim at the issuer.
        pub serial: Serial,
        /// The account the claim is made out to. Nobody else can use it.
        pub subject: AccountId,
        /// The person behind the account, as a person key.
        pub subject_ref: SubjectRef,
        /// The person key the same person had before, if it changed (a new
        /// name, a new secret of the identity services). An account bound
        /// to that key is bound to `subject_ref` from now on.
        pub replaces: Option<SubjectRef>,
        /// Verification level reached.
        pub level: VerificationLevel,
        /// Proven residence, if any.
        pub residence: Option<Residence>,
        /// Further proven attributes.
        pub attributes: Vec<Attribute>,
        /// When the issuer made the statement.
        pub issued_at: Timestamp,
        /// End of validity; `None` = until revoked.
        pub expires_at: Option<Timestamp>,
    }
}

data! {
    /// An issuer takes a claim back, for example after an error or a fraud
    /// came to light. Valid with the signature of the issuer.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Revocation {
        /// Who takes back.
        pub issuer: IssuerId,
        /// Which claim.
        pub serial: Serial,
    }
}
