// Capita Money – capita-mrtd
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Passive authentication (ICAO 9303 part 11, 5.1): are the data groups the
//! ones the issuing state signed?
//!
//! 1. EF.SOD (tag `77`) holds a CMS signed-data object; its content is the
//!    LDS security object: a hash algorithm and the hash of every data
//!    group.
//! 2. The signature over it is checked with the document signer
//!    certificate the object carries (OpenSSL CMS).
//! 3. That certificate must be signed by a country signing certificate of
//!    the [`TrustList`]. Validity dates are not checked: a document signer
//!    certificate may expire before the documents it signed; the document's
//!    own expiry is read from DG1.
//! 4. The hash of each data group given must be the one listed.

use std::path::Path;

use openssl::cms::{CMSOptions, CmsContentInfo};
use openssl::hash::{hash, MessageDigest};
use openssl::stack::Stack;
use openssl::x509::store::{X509Store, X509StoreBuilder};
use openssl::x509::verify::X509VerifyFlags;
use openssl::x509::{X509Ref, X509StoreContext, X509};

use crate::tlv;
use crate::MrtdError;

/// The country signing certificates (CSCA) an identity service trusts.
pub struct TrustList {
    store: X509Store,
    count: usize,
}

impl TrustList {
    /// A trust list of these certificates.
    ///
    /// # Errors
    /// [`MrtdError::Crypto`] if OpenSSL refuses a certificate.
    pub fn new(certificates: Vec<X509>) -> Result<Self, MrtdError> {
        let mut builder = X509StoreBuilder::new()?;
        builder.set_flags(X509VerifyFlags::NO_CHECK_TIME)?;
        let count = certificates.len();
        for certificate in certificates {
            builder.add_cert(certificate)?;
        }
        Ok(Self {
            store: builder.build(),
            count,
        })
    }

    /// Reads a trust list from a file or from every file of a folder:
    /// `.pem`, `.crt` (PEM, several certificates allowed), `.der`, `.cer`
    /// (one DER certificate), `.ml` (an ICAO master list, as the German BSI
    /// publishes it).
    ///
    /// # Errors
    /// [`MrtdError::Io`] or [`MrtdError::Crypto`] if a file is not readable.
    pub fn load(path: &Path) -> Result<Self, MrtdError> {
        let mut certificates = Vec::new();
        if path.is_dir() {
            let entries = std::fs::read_dir(path).map_err(|e| MrtdError::Io(e.to_string()))?;
            let mut files: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
            files.sort();
            for file in files {
                certificates.extend(read_file(&file)?);
            }
        } else {
            certificates.extend(read_file(path)?);
        }
        Self::new(certificates)
    }

    /// How many certificates the list holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether the list is empty (then no document passes).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Whether `certificate` is signed by a certificate of the list.
    ///
    /// # Errors
    /// [`MrtdError::Crypto`] if OpenSSL fails.
    pub fn vouches_for(&self, certificate: &X509Ref) -> Result<bool, MrtdError> {
        let chain: Stack<X509> = Stack::new()?;
        let mut context = X509StoreContext::new()?;
        Ok(context.init(&self.store, certificate, &chain, |c| c.verify_cert())?)
    }
}

fn read_file(path: &Path) -> Result<Vec<X509>, MrtdError> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let read =
        || std::fs::read(path).map_err(|e| MrtdError::Io(format!("{}: {e}", path.display())));
    Ok(match extension.as_str() {
        "pem" | "crt" => X509::stack_from_pem(&read()?)?,
        "der" | "cer" => vec![X509::from_der(&read()?)?],
        "ml" => master_list(&read()?)?,
        _ => Vec::new(),
    })
}

/// The certificates of an ICAO master list (CMS signed data whose content is
/// `CscaMasterList ::= SEQUENCE { version, SET OF Certificate }`).
///
/// Offene Frage: the signature of the master list itself is not checked
/// here; the operator fetches it from the BSI over TLS and may check it with
/// `openssl cms -verify`.
///
/// # Errors
/// [`MrtdError::Malformed`] or [`MrtdError::Crypto`] if it is no master list.
pub fn master_list(bytes: &[u8]) -> Result<Vec<X509>, MrtdError> {
    let content = encapsulated_content(bytes)?;
    let list = tlv::one(content)?;
    let parts = tlv::children(list.value)?;
    let set = parts.get(1).ok_or(MrtdError::Malformed("master list"))?;
    tlv::children(set.value)?
        .iter()
        .map(|c| X509::from_der(c.raw).map_err(MrtdError::from))
        .collect()
}

/// `ContentInfo { OID, [0] SignedData }` → the parts of `SignedData`.
fn signed_data(cms: &[u8]) -> Result<Vec<tlv::Tlv<'_>>, MrtdError> {
    let info = tlv::one(cms)?;
    let explicit =
        tlv::child(info.value, 0xa0)?.ok_or(MrtdError::Malformed("CMS without content"))?;
    let signed = tlv::one(explicit.value)?;
    tlv::children(signed.value)
}

/// The content (`eContent`) of a CMS signed-data object.
fn encapsulated_content(cms: &[u8]) -> Result<&[u8], MrtdError> {
    let parts = signed_data(cms)?;
    let encap = parts.get(2).ok_or(MrtdError::Malformed("CMS content"))?;
    let explicit = tlv::child(encap.value, 0xa0)?.ok_or(MrtdError::Malformed("CMS content"))?;
    let octets = tlv::one(explicit.value)?;
    if octets.tag != 0x04 {
        return Err(MrtdError::Malformed("CMS content"));
    }
    Ok(octets.value)
}

/// The certificates a CMS signed-data object carries (`[0] IMPLICIT`).
fn embedded_certificates(cms: &[u8]) -> Result<Vec<&[u8]>, MrtdError> {
    let parts = signed_data(cms)?;
    match parts.iter().find(|p| p.tag == 0xa0) {
        Some(set) => Ok(tlv::children(set.value)?
            .into_iter()
            .map(|c| c.raw)
            .collect()),
        None => Ok(Vec::new()),
    }
}

/// What passive authentication established.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Authenticated {
    /// The hash algorithm of the security object.
    pub algorithm: &'static str,
    /// The document signer, as OpenSSL prints its name.
    pub signer: String,
    /// The data groups whose hashes were checked.
    pub groups: Vec<u8>,
    /// Every data group the security object lists.
    pub listed: Vec<u8>,
}

fn digest(oid: &[u8]) -> Result<(&'static str, MessageDigest), MrtdError> {
    const SHA2: [u8; 8] = [0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02];
    if oid == [0x2b, 0x0e, 0x03, 0x02, 0x1a] {
        return Ok(("SHA-1", MessageDigest::sha1()));
    }
    let (prefix, last) = oid.split_at_checked(8).ok_or(MrtdError::UnknownHash)?;
    if prefix != SHA2 {
        return Err(MrtdError::UnknownHash);
    }
    match last {
        [1] => Ok(("SHA-256", MessageDigest::sha256())),
        [2] => Ok(("SHA-384", MessageDigest::sha384())),
        [3] => Ok(("SHA-512", MessageDigest::sha512())),
        [4] => Ok(("SHA-224", MessageDigest::sha224())),
        _ => Err(MrtdError::UnknownHash),
    }
}

type Hashes = Vec<(u8, Vec<u8>)>;

/// `LDSSecurityObject ::= SEQUENCE { version, hashAlgorithm, dataGroupHashValues, … }`.
fn security_object(content: &[u8]) -> Result<(&'static str, MessageDigest, Hashes), MrtdError> {
    let object = tlv::read(content)?.0;
    let parts = tlv::children(object.value)?;
    let algorithm = parts
        .get(1)
        .ok_or(MrtdError::Malformed("security object"))?;
    let oid = tlv::children(algorithm.value)?
        .into_iter()
        .next()
        .ok_or(MrtdError::Malformed("hash algorithm"))?;
    let (name, md) = digest(oid.value)?;
    let values = parts
        .get(2)
        .ok_or(MrtdError::Malformed("security object"))?;
    let mut hashes = Vec::new();
    for entry in tlv::children(values.value)? {
        let fields = tlv::children(entry.value)?;
        let (Some(number), Some(value)) = (fields.first(), fields.get(1)) else {
            return Err(MrtdError::Malformed("data group hash"));
        };
        let [n] = number.value else {
            return Err(MrtdError::Malformed("data group number"));
        };
        hashes.push((*n, value.value.to_vec()));
    }
    Ok((name, md, hashes))
}

/// Checks the security object `sod` (EF.SOD, as read from the chip) and the
/// data groups `groups` (number, bytes of the file) against `trust`.
///
/// # Errors
/// Why the data are not the ones the state signed ([`MrtdError`]).
pub fn passive_authentication(
    sod: &[u8],
    groups: &[(u8, &[u8])],
    trust: &TrustList,
) -> Result<Authenticated, MrtdError> {
    let outer = tlv::one(sod)?;
    if outer.tag != 0x77 {
        return Err(MrtdError::Malformed("EF.SOD tag"));
    }
    let cms_der = outer.value;
    let certificates = embedded_certificates(cms_der)?;
    let [signer] = certificates.as_slice() else {
        return Err(MrtdError::Malformed("one document signer certificate"));
    };
    let signer = X509::from_der(signer)?;

    let mut cms = CmsContentInfo::from_der(cms_der)?;
    let mut content = Vec::new();
    cms.verify(
        None,
        None,
        None,
        Some(&mut content),
        CMSOptions::BINARY | CMSOptions::NO_SIGNER_CERT_VERIFY,
    )
    .map_err(|_| MrtdError::BadSignature)?;

    if !trust.vouches_for(&signer)? {
        return Err(MrtdError::Untrusted);
    }

    let (algorithm, md, listed) = security_object(&content)?;
    let mut checked = Vec::new();
    for (number, data) in groups {
        let (_, want) = listed
            .iter()
            .find(|(n, _)| n == number)
            .ok_or(MrtdError::MissingGroup(*number))?;
        let got = hash(md, data)?;
        if &*got != want.as_slice() {
            return Err(MrtdError::HashMismatch(*number));
        }
        checked.push(*number);
    }
    Ok(Authenticated {
        algorithm,
        signer: format!("{:?}", signer.subject_name()),
        groups: checked,
        listed: listed.iter().map(|(n, _)| *n).collect(),
    })
}
