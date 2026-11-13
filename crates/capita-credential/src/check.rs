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

//! The check: does a claim count, and what does it say to the rules?

use capita_core::attributes::{Attestation, VerificationLevel};
use capita_core::types::{PersonId, Timestamp};

use crate::did::check_did;
use crate::encoding::{claim_bytes, person_bytes, revocation_bytes};
use crate::errors::CredentialError;
use crate::types::{
    Audience, Claim, Issuer, IssuerId, IssuerStatus, PublicKey, Realm, Revocation, Signature,
    SubjectRef, MAX_ATTRIBUTES, MAX_ATTRIBUTE_KEYS, MAX_KEYS,
};

/// What the host supplies: signature schemes and a hash function. A chain
/// runtime passes them on to its node; a service uses a library.
pub trait Crypto {
    /// `true` if `signature` was made over `message` with the secret key
    /// belonging to `key`. A signature of another scheme than the key never
    /// matches.
    fn verify(key: &PublicKey, message: &[u8], signature: &Signature) -> bool;
    /// A cryptographic hash of 32 bytes. Every host of one currency must
    /// use the same function (BLAKE2b-256), or one person gets two
    /// identifiers.
    fn hash(data: &[u8]) -> [u8; 32];
}

impl Issuer {
    /// Checks an entry of the issuer list before it is stored: a well-formed
    /// DID that fits the keys, between one and [`MAX_KEYS`] keys, a bounded
    /// list of attributes.
    pub fn validate(&self) -> Result<(), CredentialError> {
        if self.keys.is_empty() || self.keys.len() > MAX_KEYS {
            return Err(CredentialError::InvalidKeys);
        }
        if self.attributes.len() > MAX_ATTRIBUTE_KEYS {
            return Err(CredentialError::TooManyAttributeKeys);
        }
        check_did(&self.did, &self.keys)
    }

    /// `true` if any key of the issuer made `signature` over `message`.
    fn signed<C: Crypto>(&self, message: &[u8], signature: &Signature) -> bool {
        self.keys
            .iter()
            .any(|key| C::verify(key, message, signature))
    }
}

/// The identifier of a person inside the rules: the hash of realm and
/// pseudonym. Two issuers of one realm that know the same person under the
/// same pseudonym name the same person.
#[must_use]
pub fn person_id<C: Crypto>(realm: Realm, subject_ref: &SubjectRef) -> PersonId {
    PersonId(C::hash(&person_bytes(realm, subject_ref)))
}

/// Checks a claim presented at `now` on the host `audience` and returns
/// what it says to the rules.
///
/// `id` and `issuer` are the entry of the issuer list the claim names. The
/// claim counts if
/// - the issuer is active and signed exactly these bytes for this host;
/// - `now` lies between issue and expiry;
/// - the validity is within what the issuer may state;
/// - level, residence and attributes are within what the issuer may state.
///
/// Not checked here, because it needs the state of the host: that the claim
/// is presented by its subject, that it was not revoked or presented before,
/// and that the person holds no other account.
pub fn check_claim<C: Crypto>(
    claim: &Claim,
    signature: &Signature,
    id: IssuerId,
    issuer: &Issuer,
    audience: &Audience,
    now: Timestamp,
) -> Result<Attestation, CredentialError> {
    if claim.issuer != id {
        return Err(CredentialError::WrongIssuer);
    }
    if issuer.status != IssuerStatus::Active {
        return Err(CredentialError::IssuerNotActive);
    }
    if claim.attributes.len() > MAX_ATTRIBUTES {
        return Err(CredentialError::TooManyAttributes);
    }
    if !issuer.signed::<C>(&claim_bytes(audience, claim), signature) {
        return Err(CredentialError::BadSignature);
    }
    if claim.issued_at > now {
        return Err(CredentialError::NotYetValid);
    }
    match claim.expires_at {
        Some(end) if end <= claim.issued_at => return Err(CredentialError::ExpiryBeforeIssue),
        Some(end) if end <= now => return Err(CredentialError::Expired),
        _ => {}
    }
    if let Some(longest) = issuer.max_validity {
        // An end later than issue + longest, or none at all, is too long.
        // If issue + longest leaves the range of time, any end is within it.
        let limit = claim.issued_at.checked_add(longest).ok();
        let within = match (claim.expires_at, limit) {
            (Some(end), Some(limit)) => end <= limit,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if !within {
            return Err(CredentialError::ValidityTooLong);
        }
    }
    if claim.level == VerificationLevel(0) || claim.level > issuer.max_level {
        return Err(CredentialError::LevelNotAllowed);
    }
    if let Some(residence) = &claim.residence {
        if !issuer.residence {
            return Err(CredentialError::ResidenceNotAllowed);
        }
        if residence.proven_at > claim.issued_at {
            return Err(CredentialError::ResidenceFromFuture);
        }
    }
    if claim
        .attributes
        .iter()
        .any(|attribute| !issuer.attributes.contains(&attribute.key))
    {
        return Err(CredentialError::AttributeNotAllowed);
    }
    Ok(attestation::<C>(claim, issuer.realm))
}

/// What a claim says to the rules, without any check: for a claim the host
/// accepted earlier and stored.
#[must_use]
pub fn attestation<C: Crypto>(claim: &Claim, realm: Realm) -> Attestation {
    Attestation {
        person: person_id::<C>(realm, &claim.subject_ref),
        level: claim.level,
        residence: claim.residence,
        attributes: claim.attributes.clone(),
        issued_at: claim.issued_at,
        expires_at: claim.expires_at,
    }
}

/// `true` while a claim accepted earlier keeps counting at `now`: not
/// expired, and its issuer not revoked. A suspended issuer issues nothing
/// new, but what it stated stands.
#[must_use]
pub fn still_counts(claim: &Claim, issuer: &Issuer, now: Timestamp) -> bool {
    issuer.status != IssuerStatus::Revoked && claim.expires_at.is_none_or(|end| end > now)
}

/// Checks that the issuer signed the revocation for this host. Its status
/// does not matter: taking a claim back is always allowed.
pub fn check_revocation<C: Crypto>(
    revocation: &Revocation,
    signature: &Signature,
    id: IssuerId,
    issuer: &Issuer,
    audience: &Audience,
) -> Result<(), CredentialError> {
    if revocation.issuer != id {
        return Err(CredentialError::WrongIssuer);
    }
    if !issuer.signed::<C>(&revocation_bytes(audience, revocation), signature) {
        return Err(CredentialError::BadSignature);
    }
    Ok(())
}
