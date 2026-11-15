// Capita Money – capita-attest
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

//! # capita-attest
//!
//! The phone check of a verification (`docs/adr/0024-hardware-keys-checked-phones.md`):
//! a phone proves that it is a real phone with an unchanged system, running
//! the Capita app. The identity service checks the proof here, offline –
//! the phone asks no Google service.
//!
//! - [`android`]: **key attestation**. The app makes a fresh key in the
//!   phone's security chip with the service's nonce as challenge; Android
//!   answers a certificate chain up to Google's hardware attestation root
//!   whose first certificate describes key, phone and app.
//! - [`apple`]: **App Attest**. The app makes a fresh App Attest key,
//!   Apple certifies it (attestation object, root: Apple App Attestation
//!   Root CA), and the key signs the request (assertion).
//!
//! In both cases the checked key then signs the request that carries chip
//! data and film, so neither can be changed or sent by someone else.

#![forbid(unsafe_code)]

use core::fmt;

use openssl::hash::MessageDigest;
use openssl::pkey::{PKeyRef, Public};
use openssl::sign::Verifier;
use openssl::x509::X509Ref;

pub mod android;
pub mod apple;
pub mod der;

/// Why a phone check fails.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttestError {
    /// The bytes are not what the format describes.
    Malformed(&'static str),
    /// A certificate of the chain is not signed by the next one.
    Chain,
    /// The chain does not end at a root on the list.
    Untrusted,
    /// A certificate of the chain is on the revocation list (its serial).
    Revoked(String),
    /// A certificate is outside its validity.
    Expired,
    /// The proof was made for another challenge.
    Challenge,
    /// The key is not in hardware (software keystore or emulator).
    NotHardware,
    /// The key was imported, or is not a P-256 signing key.
    WrongKey,
    /// Another app, or an app signed by someone else.
    WrongApp,
    /// The phone's system is not the one its maker signed, or unlocked.
    Unlocked,
    /// A signature does not fit.
    BadSignature,
    /// An assertion's counter did not rise.
    Counter,
    /// OpenSSL failed.
    Crypto(String),
}

impl fmt::Display for AttestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "phone check not readable: {what}"),
            Self::Chain => f.write_str("phone check: broken certificate chain"),
            Self::Untrusted => f.write_str("phone check: not from a phone maker on the list"),
            Self::Revoked(serial) => write!(f, "phone check: certificate {serial} revoked"),
            Self::Expired => f.write_str("phone check: certificate outside its validity"),
            Self::Challenge => f.write_str("phone check: made for another request"),
            Self::NotHardware => f.write_str("phone check: key not in the phone's security chip"),
            Self::WrongKey => f.write_str("phone check: key imported or of another kind"),
            Self::WrongApp => f.write_str("phone check: not the Capita app"),
            Self::Unlocked => f.write_str("phone check: phone system changed or unlocked"),
            Self::BadSignature => f.write_str("phone check: signature does not fit"),
            Self::Counter => f.write_str("phone check: used before"),
            Self::Crypto(e) => write!(f, "OpenSSL: {e}"),
        }
    }
}

impl std::error::Error for AttestError {}

impl From<openssl::error::ErrorStack> for AttestError {
    fn from(e: openssl::error::ErrorStack) -> Self {
        Self::Crypto(e.to_string())
    }
}

/// Whether `signature` (ASN.1 DER, as Android and Apple produce it) is
/// `key`'s ECDSA signature with SHA-256 over `message`.
#[must_use]
pub fn signed(key: &PKeyRef<Public>, message: &[u8], signature: &[u8]) -> bool {
    Verifier::new(MessageDigest::sha256(), key)
        .and_then(|mut v| v.verify_oneshot(signature, message))
        .unwrap_or(false)
}

/// Checks that each certificate is signed by the next one.
pub(crate) fn linked(chain: &[&X509Ref]) -> Result<(), AttestError> {
    for pair in chain.windows(2) {
        let (Some(child), Some(parent)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        let key = parent.public_key()?;
        if !child.verify(&key).unwrap_or(false) {
            return Err(AttestError::Chain);
        }
    }
    Ok(())
}

/// SHA-256 of `data`.
pub(crate) fn sha256(data: &[u8]) -> Result<[u8; 32], AttestError> {
    let digest = openssl::hash::hash(MessageDigest::sha256(), data)?;
    let mut out = [0u8; 32];
    if digest.len() != 32 {
        return Err(AttestError::Crypto("SHA-256".into()));
    }
    out.copy_from_slice(&digest);
    Ok(out)
}
