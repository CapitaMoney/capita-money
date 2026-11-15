// Capita Money – capita-mrtd
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Proof that the chip itself was there, not a copy of its files
//! (ADR 0024).
//!
//! - **Active authentication** (ICAO 9303 part 11, 6.1): the chip signs the
//!   service's nonce with the private key whose public half is DG15. RSA
//!   keys sign by ISO/IEC 9796-2 scheme 1 (partial message recovery), EC
//!   keys by ECDSA in plain form `r ‖ s` with the hash DG14 names.
//! - **Chip authentication** (BSI TR-03110): the phone runs it while
//!   reading; [`needs_chip_authentication`] says whether DG14 offers it, so
//!   the service can demand the phone's report of it.

use openssl::bn::{BigNum, BigNumContext};
use openssl::ecdsa::EcdsaSig;
use openssl::hash::{hash, MessageDigest};
use openssl::pkey::{Id, PKey, Public};
use openssl::sign::Verifier;

use crate::tlv;
use crate::MrtdError;

/// 2.23.136.1.1.5, `ActiveAuthenticationInfo` in DG14.
const AA_INFO: &[u8] = &[0x67, 0x81, 0x08, 0x01, 0x01, 0x05];
/// 0.4.0.127.0.7.2.2.1, prefix of the chip authentication public keys.
const ID_PK: &[u8] = &[0x04, 0x00, 0x7f, 0x00, 0x07, 0x02, 0x02, 0x01];
/// 1.2.840.10045.4, prefix of the ECDSA signature algorithms.
const ECDSA_WITH: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04];

/// The public key of DG15 (`6F` around a `SubjectPublicKeyInfo`).
///
/// # Errors
/// [`MrtdError::Malformed`] if DG15 cannot be read.
pub fn dg15_key(dg15: &[u8]) -> Result<PKey<Public>, MrtdError> {
    let outer = tlv::one(dg15)?;
    if outer.tag != 0x6f {
        return Err(MrtdError::Malformed("DG15 tag"));
    }
    PKey::public_key_from_der(outer.value).map_err(|_| MrtdError::Malformed("DG15 key"))
}

/// The security infos of DG14 (`6E` around a `SET OF SecurityInfo`), each
/// as its object identifier and the bytes of its fields.
type Infos = Vec<(Vec<u8>, Vec<u8>)>;

fn security_infos(dg14: &[u8]) -> Result<Infos, MrtdError> {
    let outer = tlv::one(dg14)?;
    if outer.tag != 0x6e {
        return Err(MrtdError::Malformed("DG14 tag"));
    }
    let set = tlv::one(outer.value)?;
    if set.tag != 0x31 {
        return Err(MrtdError::Malformed("DG14 set"));
    }
    let mut out = Vec::new();
    for info in tlv::children(set.value)? {
        let fields = tlv::children(info.value)?;
        let oid = fields.first().ok_or(MrtdError::Malformed("DG14 info"))?;
        out.push((oid.value.to_vec(), info.value.to_vec()));
    }
    Ok(out)
}

/// Whether DG14 offers chip authentication (a `ChipAuthenticationPublicKeyInfo`).
///
/// # Errors
/// [`MrtdError::Malformed`] if DG14 cannot be read.
pub fn needs_chip_authentication(dg14: &[u8]) -> Result<bool, MrtdError> {
    Ok(security_infos(dg14)?
        .iter()
        .any(|(oid, _)| oid.starts_with(ID_PK)))
}

/// The hash an EC chip signs with, from DG14's `ActiveAuthenticationInfo`.
fn ec_hash(dg14: Option<&[u8]>) -> Result<MessageDigest, MrtdError> {
    let dg14 = dg14.ok_or(MrtdError::Malformed(
        "EC active authentication without DG14",
    ))?;
    for (oid, fields) in security_infos(dg14)? {
        if oid != AA_INFO {
            continue;
        }
        let parts = tlv::children(&fields)?;
        let algorithm = parts
            .get(2)
            .ok_or(MrtdError::Malformed("active authentication info"))?;
        let rest = algorithm
            .value
            .strip_prefix(ECDSA_WITH)
            .ok_or(MrtdError::UnknownHash)?;
        return match rest {
            [0x01] => Ok(MessageDigest::sha1()),
            [0x03, 0x01] => Ok(MessageDigest::sha224()),
            [0x03, 0x02] => Ok(MessageDigest::sha256()),
            [0x03, 0x03] => Ok(MessageDigest::sha384()),
            [0x03, 0x04] => Ok(MessageDigest::sha512()),
            _ => Err(MrtdError::UnknownHash),
        };
    }
    Err(MrtdError::Malformed(
        "DG14 without active authentication info",
    ))
}

/// Checks the chip's answer `signature` to `nonce` with the key of DG15.
///
/// # Errors
/// [`MrtdError::ChipNotProven`] if the answer does not fit; others if the
/// files cannot be read.
pub fn active_authentication(
    dg15: &[u8],
    dg14: Option<&[u8]>,
    nonce: &[u8],
    signature: &[u8],
) -> Result<(), MrtdError> {
    let key = dg15_key(dg15)?;
    let ok = match key.id() {
        Id::RSA => rsa_9796_2(&key, nonce, signature)?,
        Id::EC => {
            let md = ec_hash(dg14)?;
            let (r, s) = signature
                .split_at_checked(signature.len() / 2)
                .ok_or(MrtdError::ChipNotProven)?;
            if r.is_empty() || r.len() != s.len() {
                return Err(MrtdError::ChipNotProven);
            }
            let der =
                EcdsaSig::from_private_components(BigNum::from_slice(r)?, BigNum::from_slice(s)?)?
                    .to_der()?;
            Verifier::new(md, &key)
                .and_then(|mut v| v.verify_oneshot(&der, nonce))
                .unwrap_or(false)
        }
        _ => return Err(MrtdError::Malformed("DG15 key type")),
    };
    if ok {
        Ok(())
    } else {
        Err(MrtdError::ChipNotProven)
    }
}

/// ISO/IEC 9796-2 scheme 1: `m = s^e mod n` (or `n − m`) must read
/// `6A ‖ M1 ‖ H(M1 ‖ nonce) ‖ trailer`; trailer `BC` = SHA-1, or
/// `hash id ‖ CC`.
fn rsa_9796_2(key: &PKey<Public>, nonce: &[u8], signature: &[u8]) -> Result<bool, MrtdError> {
    let rsa = key.rsa()?;
    let n = rsa.n();
    let k = usize::try_from(n.num_bytes()).map_err(|_| MrtdError::Malformed("RSA key"))?;
    let s = BigNum::from_slice(signature)?;
    if s >= *n {
        return Ok(false);
    }
    let mut ctx = BigNumContext::new()?;
    let mut m = BigNum::new()?;
    m.mod_exp(&s, rsa.e(), n, &mut ctx)?;
    let mut other = BigNum::new()?;
    other.checked_sub(n, &m)?;
    for candidate in [m, other] {
        let block = candidate
            .to_vec_padded(i32::try_from(k).map_err(|_| MrtdError::Malformed("RSA key"))?)?;
        if recovered_fits(&block, nonce)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn recovered_fits(block: &[u8], nonce: &[u8]) -> Result<bool, MrtdError> {
    if block.first() != Some(&0x6a) {
        return Ok(false);
    }
    let (md, trailer) = match block.last() {
        Some(0xbc) => (MessageDigest::sha1(), 1usize),
        Some(0xcc) => {
            let id = block
                .len()
                .checked_sub(2)
                .and_then(|i| block.get(i))
                .copied();
            let md = match id {
                Some(0x33) => MessageDigest::sha1(),
                Some(0x34) => MessageDigest::sha256(),
                Some(0x35) => MessageDigest::sha512(),
                Some(0x36) => MessageDigest::sha384(),
                Some(0x38) => MessageDigest::sha224(),
                _ => return Ok(false),
            };
            (md, 2)
        }
        _ => return Ok(false),
    };
    let h = md.size();
    let Some(digest_at) = block.len().checked_sub(h.saturating_add(trailer)) else {
        return Ok(false);
    };
    let (Some(m1), Some(digest)) = (
        block.get(1..digest_at),
        block.get(digest_at..digest_at.saturating_add(h)),
    ) else {
        return Ok(false);
    };
    let mut message = m1.to_vec();
    message.extend_from_slice(nonce);
    Ok(*hash(md, &message)? == *digest)
}
