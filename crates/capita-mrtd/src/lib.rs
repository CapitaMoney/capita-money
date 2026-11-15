// Capita Money – capita-mrtd
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

//! # capita-mrtd
//!
//! The chip of a passport or identity card (ICAO Doc 9303, "machine readable
//! travel document") as an identity service checks it
//! (`docs/adr/0021-verification-ways.md`). The phone reads the files and
//! sends them; this crate decides whether they are genuine and what they
//! say:
//!
//! - [`pa::passive_authentication`]: the hashes of the data groups in the
//!   security object (EF.SOD) match; the security object is signed by the
//!   document signer certificate it carries; that certificate is signed by
//!   a country signing certificate (CSCA) of the [`pa::TrustList`].
//! - [`mrz`]: the machine-readable zone of DG1 – name, date of birth,
//!   document number, nationality, expiry – with its check digits.
//! - [`groups::face_image`]: the photo of DG2, JPEG or JPEG 2000.
//!
//! [`read_chip`] does all three. Nothing is kept: the caller holds the bytes
//! for the length of one check.
//!
//! [`check_chip`] adds the proof that the chip itself was there
//! ([`aa`]: active authentication checked here, chip authentication as the
//! checked phone reports it; ADR 0024).
//!
//! Not here (yet): revocation lists of document signers.

#![forbid(unsafe_code)]

pub mod aa;
pub mod groups;
pub mod mrz;
pub mod pa;
pub mod tlv;

use core::fmt;

/// Why chip data are refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MrtdError {
    /// The bytes are not what ICAO 9303 describes here.
    Malformed(&'static str),
    /// A check digit of the machine-readable zone is wrong.
    CheckDigit(&'static str),
    /// The security object is not signed by its document signer.
    BadSignature,
    /// The document signer is not signed by a country on the trust list.
    Untrusted,
    /// A data group is not listed in the security object.
    MissingGroup(u8),
    /// A data group does not have the hash the security object lists.
    HashMismatch(u8),
    /// The security object lists a data group that was not sent.
    MissingFile(u8),
    /// The chip did not prove itself (active or chip authentication).
    ChipNotProven,
    /// The hash algorithm of the security object is not supported.
    UnknownHash,
    /// OpenSSL failed.
    Crypto(String),
    /// A file of the trust list could not be read.
    Io(String),
}

impl fmt::Display for MrtdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "chip data not readable: {what}"),
            Self::CheckDigit(field) => write!(f, "check digit wrong: {field}"),
            Self::BadSignature => f.write_str("security object not signed by its document signer"),
            Self::Untrusted => f.write_str("document signer not signed by a trusted country"),
            Self::MissingGroup(n) => write!(f, "DG{n} not listed in the security object"),
            Self::HashMismatch(n) => write!(f, "DG{n} does not match its hash: changed data"),
            Self::MissingFile(n) => write!(f, "DG{n} is listed by the security object but missing"),
            Self::ChipNotProven => {
                f.write_str("the chip did not prove itself: a copy of its data?")
            }
            Self::UnknownHash => f.write_str("hash algorithm not supported"),
            Self::Crypto(e) => write!(f, "OpenSSL: {e}"),
            Self::Io(e) => write!(f, "trust list: {e}"),
        }
    }
}

impl std::error::Error for MrtdError {}

impl From<openssl::error::ErrorStack> for MrtdError {
    fn from(e: openssl::error::ErrorStack) -> Self {
        Self::Crypto(e.to_string())
    }
}

/// What a checked chip says.
#[derive(Clone, Debug)]
pub struct Chip {
    /// The machine-readable zone.
    pub mrz: mrz::Mrz,
    /// Format of the photo.
    pub photo_format: groups::ImageFormat,
    /// The photo as stored on the chip.
    pub photo: Vec<u8>,
    /// What passive authentication established.
    pub authenticated: pa::Authenticated,
}

/// Checks EF.SOD, DG1 and DG2 against the trust list and reads them.
///
/// # Errors
/// Why the data are refused ([`MrtdError`]).
pub fn read_chip(
    sod: &[u8],
    dg1: &[u8],
    dg2: &[u8],
    trust: &pa::TrustList,
) -> Result<Chip, MrtdError> {
    let authenticated = pa::passive_authentication(sod, &[(1, dg1), (2, dg2)], trust)?;
    let mrz = mrz::parse(&groups::mrz_text(dg1)?)?;
    let (photo_format, photo) = groups::face_image(dg2)?;
    Ok(Chip {
        mrz,
        photo_format,
        photo: photo.to_vec(),
        authenticated,
    })
}

/// The files a phone read from a chip.
#[derive(Clone, Copy, Debug)]
pub struct ChipFiles<'a> {
    /// EF.SOD.
    pub sod: &'a [u8],
    /// DG1, the machine-readable zone.
    pub dg1: &'a [u8],
    /// DG2, the photo.
    pub dg2: &'a [u8],
    /// DG14, the security infos, if the chip has it.
    pub dg14: Option<&'a [u8]>,
    /// DG15, the active authentication key, if the chip has it.
    pub dg15: Option<&'a [u8]>,
}

/// What the chip answered to prove itself.
#[derive(Clone, Copy, Debug)]
pub struct ChipProof<'a> {
    /// The service's nonce for active authentication (8 bytes).
    pub nonce: &'a [u8],
    /// The chip's signature of the nonce, if it has DG15.
    pub active: Option<&'a [u8]>,
    /// Whether the phone ran chip authentication successfully (reported
    /// inside the request the checked phone signed).
    pub chip_authenticated: bool,
}

/// Checks the files against the trust list, and the chip's proof where its
/// security object lists DG14 or DG15: a file the security object lists
/// must be sent (else [`MrtdError::MissingFile`]), DG15 demands a valid
/// active authentication, a DG14 that offers chip authentication demands
/// the phone's report of it (else [`MrtdError::ChipNotProven`]).
///
/// # Errors
/// Why the chip is refused ([`MrtdError`]).
pub fn check_chip(
    files: &ChipFiles<'_>,
    proof: &ChipProof<'_>,
    trust: &pa::TrustList,
) -> Result<Chip, MrtdError> {
    let mut groups: Vec<(u8, &[u8])> = vec![(1, files.dg1), (2, files.dg2)];
    if let Some(dg14) = files.dg14 {
        groups.push((14, dg14));
    }
    if let Some(dg15) = files.dg15 {
        groups.push((15, dg15));
    }
    let authenticated = pa::passive_authentication(files.sod, &groups, trust)?;
    for n in [14u8, 15] {
        if authenticated.listed.contains(&n) && !authenticated.groups.contains(&n) {
            return Err(MrtdError::MissingFile(n));
        }
    }
    if let Some(dg15) = files.dg15 {
        let signature = proof.active.ok_or(MrtdError::ChipNotProven)?;
        aa::active_authentication(dg15, files.dg14, proof.nonce, signature)?;
    }
    if let Some(dg14) = files.dg14 {
        if aa::needs_chip_authentication(dg14)? && !proof.chip_authenticated {
            return Err(MrtdError::ChipNotProven);
        }
    }
    let mrz = mrz::parse(&groups::mrz_text(files.dg1)?)?;
    let (photo_format, photo) = groups::face_image(files.dg2)?;
    Ok(Chip {
        mrz,
        photo_format,
        photo: photo.to_vec(),
        authenticated,
    })
}
