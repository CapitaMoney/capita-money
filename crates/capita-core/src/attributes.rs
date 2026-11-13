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

//! Attributes of a person as the core sees them, and filters over them.
//!
//! The core knows no group by name. An attribute is a numbered key with a
//! numeric value; which keys exist and what they mean is published in the
//! parameters and supplied by the identity adapter. Sponsors may target only
//! keys on that public list – no hidden criteria.

use alloc::vec::Vec;

use crate::errors::CoreError;
use crate::params::Params;
use crate::types::{CountryCode, PersonId, Timestamp};

/// Numbered attribute key. Meaning assigned by the identity adapter and
/// published in `Params::sponsor_keys`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AttrKey(pub u16);

/// Numeric attribute value (a code, a number of years, a community id …).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AttrValue(pub u64);

/// One proven attribute of a person.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Attribute {
    /// Which attribute.
    pub key: AttrKey,
    /// Its value.
    pub value: AttrValue,
}

/// How thoroughly a person has been verified. 0 = not at all; higher = more.
/// The parameters decide which level each ladder step requires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VerificationLevel(pub u8);

/// Proven place of residence. The only source of the country cap; a passport
/// proves identity, never residence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Residence {
    /// Country of residence.
    pub country: CountryCode,
    /// When the proof (for example a coded postal letter) succeeded.
    pub proven_at: Timestamp,
}

/// What an identity provider states about the holder of an account.
/// Contains derived facts only – no name, document or birth date.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Attestation {
    /// The unique person behind the account.
    pub person: PersonId,
    /// Verification level reached.
    pub level: VerificationLevel,
    /// Proven residence, if any.
    pub residence: Option<Residence>,
    /// Further proven attributes (for sponsor targeting).
    pub attributes: Vec<Attribute>,
    /// When the provider issued the statement.
    pub issued_at: Timestamp,
    /// End of validity; `None` = until revoked.
    pub expires_at: Option<Timestamp>,
}

/// One condition on an attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Criterion {
    /// Attribute equals the value.
    Equals(AttrKey, AttrValue),
    /// Attribute equals one of the values.
    OneOf(AttrKey, Vec<AttrValue>),
    /// Attribute lies in the inclusive range.
    Between(AttrKey, AttrValue, AttrValue),
}

/// A target group: every criterion must hold. Empty = everybody.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AttributeFilter {
    /// Conditions, all of which must hold.
    pub all_of: Vec<Criterion>,
}

impl Criterion {
    /// The attribute the condition is about.
    #[must_use]
    pub const fn key(&self) -> AttrKey {
        match self {
            Self::Equals(key, _) | Self::OneOf(key, _) | Self::Between(key, _, _) => *key,
        }
    }

    /// `true` if `attributes` contain a value for the key that meets the
    /// condition. A person without the attribute never matches.
    #[must_use]
    pub fn matches(&self, attributes: &[Attribute]) -> bool {
        attributes
            .iter()
            .filter(|a| a.key == self.key())
            .any(|a| match self {
                Self::Equals(_, value) => a.value == *value,
                Self::OneOf(_, values) => values.contains(&a.value),
                Self::Between(_, low, high) => *low <= a.value && a.value <= *high,
            })
    }
}

impl AttributeFilter {
    /// Checks that the filter uses only attribute keys published in
    /// `params.sponsor_keys` and that every condition can be met at all.
    /// No hidden criteria: a filter over an unpublished key is refused.
    pub fn validate(&self, params: &Params) -> Result<(), CoreError> {
        for criterion in &self.all_of {
            if !params.sponsor_keys.contains(&criterion.key()) {
                return Err(CoreError::UnknownSponsorKey);
            }
            match criterion {
                Criterion::OneOf(_, values) if values.is_empty() => {
                    return Err(CoreError::OutOfRange("empty list of values"));
                }
                Criterion::Between(_, low, high) if low > high => {
                    return Err(CoreError::OutOfRange("range with low above high"));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// `true` if the attributes of a person meet every condition.
    #[must_use]
    pub fn matches(&self, attributes: &[Attribute]) -> bool {
        self.all_of.iter().all(|c| c.matches(attributes))
    }
}
