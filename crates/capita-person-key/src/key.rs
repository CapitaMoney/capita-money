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

//! The keyed function: from canonical data to a person key.

use alloc::vec::Vec;
use core::fmt;

/// Why no person key could be computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// The secret is unusable (empty, or of a wrong form).
    Secret,
    /// The data is unusable (empty or far too long).
    Input,
    /// The key service did not answer, or not correctly.
    Service,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Secret => "unusable secret",
            Self::Input => "unusable input",
            Self::Service => "key service failed",
        })
    }
}

impl core::error::Error for KeyError {}

/// Whoever can apply the keyed function. One interface for the three ways
/// of holding the secret (ADR 0013): in the identity service itself
/// ([`LocalKey`]), in a separate key service that is asked blindly, or
/// shared among several services. All three compute the same function.
pub trait KeyService {
    /// The function of RFC 9497 (OPRF, ristretto255-SHA512, mode 0) of
    /// `input` under the secret: 64 bytes.
    fn evaluate(&self, input: &[u8]) -> Result<[u8; 64], KeyError>;
}

/// The person key for canonical data (`crate::canonical`).
///
/// `counter` is 0 for everybody but namesakes: the second person with the
/// same canonical data gets 1, and so on (ADR 0013, "Namesakes").
pub fn person_key(
    service: &impl KeyService,
    canonical: &[u8],
    counter: u8,
) -> Result<[u8; 32], KeyError> {
    let mut input = Vec::with_capacity(canonical.len().saturating_add(2));
    input.extend_from_slice(canonical);
    input.extend_from_slice(&[0, counter]);
    first_half(&service.evaluate(&input)?)
}

/// What an identity service keeps to tell namesakes apart: a keyed hash of
/// the canonical data together with a further fact about the person (the
/// place of birth, `crate::canonical::discriminator`). Two requests with
/// the same canonical data are the same person if the tags are equal.
/// Never published.
pub fn namesake_tag(
    service: &impl KeyService,
    canonical: &[u8],
    discriminator: &str,
) -> Result<[u8; 32], KeyError> {
    let mut input = Vec::with_capacity(
        canonical
            .len()
            .saturating_add(discriminator.len())
            .saturating_add(2),
    );
    input.extend_from_slice(canonical);
    input.extend_from_slice(&[0xff, 0]);
    input.extend_from_slice(discriminator.as_bytes());
    first_half(&service.evaluate(&input)?)
}

fn first_half(output: &[u8; 64]) -> Result<[u8; 32], KeyError> {
    let mut key = [0u8; 32];
    for (to, from) in key.iter_mut().zip(output.iter()) {
        *to = *from;
    }
    Ok(key)
}

/// The keyed function, computed with a secret this program holds.
#[cfg(feature = "local")]
pub struct LocalKey(voprf::OprfServer<voprf::Ristretto255>);

#[cfg(feature = "local")]
impl LocalKey {
    /// Text that separates the key derived from a secret here from any
    /// other use of the same secret.
    const INFO: &'static [u8] = b"capita-person-key-v1";

    /// From a secret of at least 32 random bytes. The key of the function
    /// is derived from it as RFC 9497 says (`DeriveKeyPair`), so any
    /// implementation of the RFC computes the same person keys from the
    /// same secret.
    pub fn from_secret(secret: &[u8]) -> Result<Self, KeyError> {
        if secret.len() < 32 {
            return Err(KeyError::Secret);
        }
        voprf::OprfServer::new_from_seed(secret, Self::INFO)
            .map(Self)
            .map_err(|_| KeyError::Secret)
    }
}

#[cfg(feature = "local")]
impl KeyService for LocalKey {
    fn evaluate(&self, input: &[u8]) -> Result<[u8; 64], KeyError> {
        let output = self.0.evaluate(input).map_err(|_| KeyError::Input)?;
        let bytes: &[u8] = output.as_ref();
        <[u8; 64]>::try_from(bytes).map_err(|_| KeyError::Service)
    }
}

#[cfg(feature = "local")]
impl fmt::Debug for LocalKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LocalKey(secret)")
    }
}
