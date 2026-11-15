// Capita Money – capita-attest
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Android key attestation, as the identity service checks it (ADR 0024).
//!
//! The app makes a P-256 key in the Keystore with the service's nonce as
//! attestation challenge and sends the certificate chain. The first
//! certificate carries the *key description* (extension
//! 1.3.6.1.4.1.11129.2.1.17): where the key lives, how it was made, the
//! phone's root of trust and the app that asked. The chain ends at one of
//! Google's hardware attestation roots – Google publishes them; nothing is
//! asked of Google at check time. Revoked certificates come from Google's
//! status list, which the operator fetches (not the phone).

use std::collections::BTreeSet;

use openssl::pkey::{PKey, Public};
use openssl::x509::{X509Ref, X509};

use crate::der::{self, tag, Class, Der};
use crate::{linked, AttestError};

/// 1.3.6.1.4.1.11129.2.1.17, the key description.
pub const KEY_DESCRIPTION: &[u8] = &[0x2b, 0x06, 0x01, 0x04, 0x01, 0xd6, 0x79, 0x02, 0x01, 0x11];

/// Where a key lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecurityLevel {
    /// In the operating system: no protection beyond it (and emulators).
    Software,
    /// In the trusted environment of the phone's processor.
    TrustedEnvironment,
    /// In a separate security chip.
    StrongBox,
}

/// The state of the phone's verified boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootState {
    /// The system is the phone maker's, checked at boot.
    Verified,
    /// A system signed by its own key (for example GrapheneOS), checked at boot.
    SelfSigned,
    /// Not checked: unlocked bootloader.
    Unverified,
    /// Checking failed.
    Failed,
}

/// What the identity service accepts.
#[derive(Clone, Debug)]
pub struct Policy {
    /// The public keys of the roots (Google's hardware attestation roots).
    pub roots: Vec<PKey<Public>>,
    /// Serial numbers of revoked certificates, lower-case hexadecimal
    /// without leading zeros (Google's status list).
    pub revoked: BTreeSet<String>,
    /// Package names of the app (`com.capitamoney.app`).
    pub packages: Vec<String>,
    /// SHA-256 of the certificates the app may be signed with.
    pub signers: Vec<[u8; 32]>,
    /// Verified-boot keys of systems accepted in the state *self-signed*
    /// (SHA-256 as the key description states them).
    pub boot_keys: Vec<Vec<u8>>,
    /// The lowest place a key may live (default: trusted environment).
    pub lowest: SecurityLevel,
}

/// What a passed check established.
#[derive(Clone, Debug)]
pub struct Phone {
    /// Where the key lives.
    pub level: SecurityLevel,
    /// The boot state.
    pub boot: BootState,
    /// The checked key: it signs the request.
    pub key: PKey<Public>,
    /// The system's security patch level (`YYYYMM`), if stated.
    pub patch_level: Option<u64>,
}

fn level(element: &Der<'_>) -> Result<SecurityLevel, AttestError> {
    match element.unsigned("security level")? {
        0 => Ok(SecurityLevel::Software),
        1 => Ok(SecurityLevel::TrustedEnvironment),
        2 => Ok(SecurityLevel::StrongBox),
        _ => Err(AttestError::Malformed("security level")),
    }
}

/// The element inside the authorization list entry `[number]`.
fn entry<'a>(list: &[Der<'a>], number: u32) -> Result<Option<Der<'a>>, AttestError> {
    match list
        .iter()
        .find(|e| e.class == Class::Context && e.tag == number)
    {
        Some(e) => Ok(Some(der::one(e.value)?)),
        None => Ok(None),
    }
}

fn number_in(list: &[Der<'_>], number: u32) -> Result<Option<u64>, AttestError> {
    entry(list, number)?
        .map(|e| e.unsigned("authorization"))
        .transpose()
}

fn serial(certificate: &X509Ref) -> Result<String, AttestError> {
    let hex = certificate.serial_number().to_bn()?.to_hex_str()?;
    let hex = hex.trim_start_matches('0').to_ascii_lowercase();
    Ok(if hex.is_empty() { "0".into() } else { hex })
}

/// Normalises a serial number of Google's status list.
#[must_use]
pub fn revocation_key(serial: &str) -> String {
    let hex = serial.trim().trim_start_matches('0').to_ascii_lowercase();
    if hex.is_empty() {
        "0".into()
    } else {
        hex
    }
}

/// Checks the certificate chain `chain` (DER, the key's certificate first,
/// the root last) against `challenge` and `policy`.
///
/// # Errors
/// Why the phone does not pass ([`AttestError`]).
pub fn check(chain: &[Vec<u8>], challenge: &[u8], policy: &Policy) -> Result<Phone, AttestError> {
    if chain.len() < 2 {
        return Err(AttestError::Malformed("certificate chain"));
    }
    let certificates = chain
        .iter()
        .map(|c| X509::from_der(c).map_err(|_| AttestError::Malformed("certificate")))
        .collect::<Result<Vec<_>, _>>()?;
    let refs: Vec<&X509Ref> = certificates.iter().map(|c| c.as_ref()).collect();
    linked(&refs)?;
    let root = refs.last().ok_or(AttestError::Chain)?;
    let root_key = root.public_key()?;
    if !root.verify(&root_key).unwrap_or(false) {
        return Err(AttestError::Chain);
    }
    if !policy.roots.iter().any(|r| r.public_eq(&root_key)) {
        return Err(AttestError::Untrusted);
    }
    for certificate in &refs {
        let s = serial(certificate)?;
        if policy.revoked.contains(&s) {
            return Err(AttestError::Revoked(s));
        }
    }
    let leaf = chain.first().ok_or(AttestError::Chain)?;
    let description = der::extension(leaf, KEY_DESCRIPTION)?
        .ok_or(AttestError::Malformed("no key description"))?;
    let fields = der::one(description)?.inside(tag::SEQUENCE, "key description")?;
    let field = |i: usize| {
        fields
            .get(i)
            .ok_or(AttestError::Malformed("key description"))
    };
    let attestation_level = level(field(1)?)?;
    let keymint_level = level(field(3)?)?;
    let found = field(4)?.bytes(tag::OCTET_STRING, "challenge")?;
    let software = field(6)?.inside(tag::SEQUENCE, "software list")?;
    let hardware = field(7)?.inside(tag::SEQUENCE, "hardware list")?;

    if found != challenge {
        return Err(AttestError::Challenge);
    }
    let level = attestation_level.min(keymint_level);
    if level < policy.lowest || level == SecurityLevel::Software {
        return Err(AttestError::NotHardware);
    }
    // origin [702] = generated inside; algorithm [2] = EC; curve [10] = P-256;
    // purpose [1] includes sign (2).
    if number_in(&hardware, 702)? != Some(0)
        || number_in(&hardware, 2)? != Some(3)
        || number_in(&hardware, 10)? != Some(1)
    {
        return Err(AttestError::WrongKey);
    }
    let purposes = entry(&hardware, 1)?.ok_or(AttestError::WrongKey)?;
    let signs = purposes
        .inside(tag::SET, "purpose")?
        .iter()
        .any(|p| p.unsigned("purpose") == Ok(2));
    if !signs {
        return Err(AttestError::WrongKey);
    }

    let trust = entry(&hardware, 704)?
        .ok_or(AttestError::Unlocked)?
        .inside(tag::SEQUENCE, "root of trust")?;
    let boot_key = trust
        .first()
        .ok_or(AttestError::Malformed("root of trust"))?
        .bytes(tag::OCTET_STRING, "boot key")?;
    let locked = trust
        .get(1)
        .ok_or(AttestError::Malformed("root of trust"))?
        .bytes(tag::BOOLEAN, "device locked")?;
    let boot = match trust
        .get(2)
        .ok_or(AttestError::Malformed("root of trust"))?
        .unsigned("boot state")?
    {
        0 => BootState::Verified,
        1 => BootState::SelfSigned,
        2 => BootState::Unverified,
        _ => BootState::Failed,
    };
    let locked = matches!(locked, [b] if *b != 0);
    let boot_ok = match boot {
        BootState::Verified => true,
        BootState::SelfSigned => policy.boot_keys.iter().any(|k| k.as_slice() == boot_key),
        BootState::Unverified | BootState::Failed => false,
    };
    if !locked || !boot_ok {
        return Err(AttestError::Unlocked);
    }

    let app = match entry(&software, 709)? {
        Some(app) => app,
        None => entry(&hardware, 709)?.ok_or(AttestError::WrongApp)?,
    };
    let app = der::one(app.bytes(tag::OCTET_STRING, "application id")?)?
        .inside(tag::SEQUENCE, "application id")?;
    let packages = app
        .first()
        .ok_or(AttestError::WrongApp)?
        .inside(tag::SET, "packages")?;
    let mut names = Vec::new();
    for package in packages {
        let info = package.inside(tag::SEQUENCE, "package")?;
        let name = info
            .first()
            .ok_or(AttestError::WrongApp)?
            .bytes(tag::OCTET_STRING, "package name")?;
        names.push(name);
    }
    if names.is_empty()
        || !names
            .iter()
            .all(|n| policy.packages.iter().any(|p| p.as_bytes() == *n))
    {
        return Err(AttestError::WrongApp);
    }
    let digests = app
        .get(1)
        .ok_or(AttestError::WrongApp)?
        .inside(tag::SET, "signers")?;
    let mut any = false;
    for digest in digests {
        let d = digest.bytes(tag::OCTET_STRING, "signer")?;
        if !policy.signers.iter().any(|s| s.as_slice() == d) {
            return Err(AttestError::WrongApp);
        }
        any = true;
    }
    if !any {
        return Err(AttestError::WrongApp);
    }

    let key = refs.first().ok_or(AttestError::Chain)?.public_key()?;
    let is_p256 = key.ec_key().ok().and_then(|k| k.group().curve_name())
        == Some(openssl::nid::Nid::X9_62_PRIME256V1);
    if !is_p256 {
        return Err(AttestError::WrongKey);
    }
    let patch_level = match number_in(&hardware, 706)? {
        Some(n) => Some(n),
        None => number_in(&software, 706)?,
    };
    Ok(Phone {
        level,
        boot,
        key,
        patch_level,
    })
}
