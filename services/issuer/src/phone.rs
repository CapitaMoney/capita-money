// Capita Money – capita-issuer
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The phone check of a document check (`docs/adr/0024-hardware-keys-checked-phones.md`):
//! a verification counts only from a real phone with an unchanged system,
//! running the Capita app. The phone proves it with a fresh key – Android
//! key attestation or Apple App Attest, both made for the session's nonce –
//! and that key signs the [`request_digest`] of everything it sends. No
//! Google service is asked; Google's roots and revocation list are files.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use capita_attest::android::{self, SecurityLevel};
use capita_attest::{apple, signed, AttestError};
use openssl::pkey::{PKey, Public};
use openssl::x509::X509;
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Tag of the request digest.
pub const DIGEST_TAG: &[u8] = b"capita/document/v1";

/// Settings of the phone check (`[checks.document.phone]`).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhoneConfig {
    /// Google's hardware attestation roots: a PEM file or a folder of them
    /// (`trust/attestation/google/`).
    pub android_roots: PathBuf,
    /// Google's revocation list as published at
    /// `https://android.googleapis.com/attestation/status` (JSON), fetched
    /// by the operator. Without it nothing counts as revoked.
    #[serde(default)]
    pub android_revoked: Option<PathBuf>,
    /// Package names of the app.
    pub packages: Vec<String>,
    /// SHA-256 of the certificates the Android app may be signed with, in
    /// hexadecimal (colons allowed, as `apksigner` and `keytool` print them).
    pub signers: Vec<String>,
    /// Verified-boot keys of systems accepted as *self-signed* (for example
    /// GrapheneOS), hexadecimal.
    #[serde(default)]
    pub boot_keys: Vec<String>,
    /// Accept keys of the StrongBox chip only, not of the trusted environment.
    #[serde(default)]
    pub strongbox_only: bool,
    /// Apple App Attestation Root CA (PEM). Without it iPhones cannot verify.
    #[serde(default)]
    pub apple_root: Option<PathBuf>,
    /// App ids of the iPhone app: team id + "." + bundle id.
    #[serde(default)]
    pub apple_app_ids: Vec<String>,
    /// Accept App Attest keys of the development environment (test builds).
    #[serde(default)]
    pub apple_development: bool,
}

/// What the phone sends as its proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhoneProof {
    /// Android: the certificate chain of the session key (DER, key first)
    /// and its signature (DER) over the request digest.
    Android {
        /// The chain.
        chain: Vec<Vec<u8>>,
        /// The signature.
        signature: Vec<u8>,
    },
    /// iPhone: a fresh App Attest key certified for the nonce, and its
    /// assertion over the request digest.
    Apple {
        /// SHA-256 of the key's public point.
        key_id: Vec<u8>,
        /// The attestation object (CBOR).
        attestation: Vec<u8>,
        /// The assertion (CBOR).
        assertion: Vec<u8>,
    },
}

/// The loaded phone check.
pub struct PhoneCheck {
    android: android::Policy,
    apple: Option<apple::Policy>,
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    let clean: Vec<u8> = text
        .bytes()
        .filter(|b| !matches!(b, b':' | b' ' | b'\t'))
        .collect();
    if !clean.len().is_multiple_of(2) {
        return Err(format!("{text}: not hexadecimal"));
    }
    clean
        .chunks(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|p| u8::from_str_radix(p, 16).ok())
                .ok_or_else(|| format!("{text}: not hexadecimal"))
        })
        .collect()
}

fn certificates(path: &Path) -> Result<Vec<X509>, String> {
    let files = if path.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "pem"))
            .collect();
        files.sort();
        files
    } else {
        vec![path.to_path_buf()]
    };
    let mut out = Vec::new();
    for file in files {
        let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        out.extend(X509::stack_from_pem(&bytes).map_err(|e| format!("{}: {e}", file.display()))?);
    }
    if out.is_empty() {
        return Err(format!("{}: no certificate", path.display()));
    }
    Ok(out)
}

/// The serial numbers of Google's status list (`{"entries": {serial: …}}`).
/// Every listed certificate is refused, revoked or suspended.
///
/// # Errors
/// If the text is not the list.
pub fn revoked_serials(json: &str) -> Result<BTreeSet<String>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("revocation list: {e}"))?;
    let entries = value
        .get("entries")
        .and_then(serde_json::Value::as_object)
        .ok_or("revocation list: no entries")?;
    Ok(entries.keys().map(|k| android::revocation_key(k)).collect())
}

impl PhoneCheck {
    /// Loads roots, lists and app identities.
    ///
    /// # Errors
    /// What could not be loaded.
    pub fn load(config: &PhoneConfig) -> Result<Self, String> {
        let mut roots: Vec<PKey<Public>> = Vec::new();
        for c in certificates(&config.android_roots)? {
            let key = c.public_key().map_err(|e| e.to_string())?;
            if !roots.iter().any(|r| r.public_eq(&key)) {
                roots.push(key);
            }
        }
        let revoked = match &config.android_revoked {
            Some(path) => revoked_serials(
                &std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
            )?,
            None => BTreeSet::new(),
        };
        let mut signers = Vec::new();
        for s in &config.signers {
            let bytes: [u8; 32] = unhex(s)?
                .try_into()
                .map_err(|_| format!("{s}: not 32 bytes"))?;
            signers.push(bytes);
        }
        if config.packages.is_empty() || signers.is_empty() {
            return Err("phone check: name the app's packages and signers".into());
        }
        let boot_keys = config
            .boot_keys
            .iter()
            .map(|k| unhex(k))
            .collect::<Result<Vec<_>, _>>()?;
        let apple = match &config.apple_root {
            Some(path) => {
                let root = certificates(path)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| format!("{}: no certificate", path.display()))?;
                Some(apple::Policy {
                    root,
                    app_ids: config.apple_app_ids.clone(),
                    development: config.apple_development,
                })
            }
            None => None,
        };
        Ok(Self {
            android: android::Policy {
                roots,
                revoked,
                packages: config.packages.clone(),
                signers,
                boot_keys,
                lowest: if config.strongbox_only {
                    SecurityLevel::StrongBox
                } else {
                    SecurityLevel::TrustedEnvironment
                },
            },
            apple,
        })
    }

    /// The kinds of phones this service can check.
    #[must_use]
    pub fn kinds(&self) -> Vec<&'static str> {
        let mut kinds = vec!["android"];
        if self.apple.as_ref().is_some_and(|a| !a.app_ids.is_empty()) {
            kinds.push("apple");
        }
        kinds
    }

    /// Checks `proof` for the session's `nonce` and the `digest` of the
    /// request. `now` in seconds since 1970.
    ///
    /// # Errors
    /// Why the phone does not count ([`AttestError`]).
    pub fn check(
        &self,
        proof: &PhoneProof,
        nonce: &[u8; 32],
        digest: &[u8; 32],
        now: u64,
    ) -> Result<(), AttestError> {
        match proof {
            PhoneProof::Android { chain, signature } => {
                let phone = android::check(chain, nonce, &self.android)?;
                if signed(&phone.key, digest, signature) {
                    Ok(())
                } else {
                    Err(AttestError::BadSignature)
                }
            }
            PhoneProof::Apple {
                key_id,
                attestation,
                assertion,
            } => {
                let policy = self.apple.as_ref().ok_or(AttestError::Untrusted)?;
                let now = i64::try_from(now).map_err(|_| AttestError::Expired)?;
                apple::check(attestation, key_id, assertion, nonce, digest, now, policy).map(|_| ())
            }
        }
    }
}

/// What the phone's key signs: everything the request carries, bound to
/// the session and its nonce.
///
/// `SHA-256("capita/document/v1" ‖ nonce ‖ SHA-256(session) ‖ SHA-256(sod)
/// ‖ SHA-256(dg1) ‖ SHA-256(dg2) ‖ SHA-256(dg14) ‖ SHA-256(dg15) ‖
/// SHA-256(active) ‖ chip authenticated (1 byte) ‖ frame count (4 bytes,
/// big-endian) ‖ SHA-256(frame) …)`; an absent file hashes as empty.
#[must_use]
pub fn request_digest(nonce: &[u8; 32], session: &str, parts: &DigestParts<'_>) -> [u8; 32] {
    let h = |b: &[u8]| Sha256::digest(b);
    let mut d = Sha256::new();
    d.update(DIGEST_TAG);
    d.update(nonce);
    d.update(h(session.as_bytes()));
    for file in [
        parts.sod,
        parts.dg1,
        parts.dg2,
        parts.dg14.unwrap_or(&[]),
        parts.dg15.unwrap_or(&[]),
        parts.active.unwrap_or(&[]),
    ] {
        d.update(h(file));
    }
    d.update([u8::from(parts.chip_authenticated)]);
    d.update(
        u32::try_from(parts.frames.len())
            .unwrap_or(u32::MAX)
            .to_be_bytes(),
    );
    for frame in parts.frames {
        d.update(h(frame));
    }
    d.finalize().into()
}

/// The parts of a request that the digest covers.
#[derive(Clone, Copy, Debug)]
pub struct DigestParts<'a> {
    /// EF.SOD.
    pub sod: &'a [u8],
    /// DG1.
    pub dg1: &'a [u8],
    /// DG2.
    pub dg2: &'a [u8],
    /// DG14, if sent.
    pub dg14: Option<&'a [u8]>,
    /// DG15, if sent.
    pub dg15: Option<&'a [u8]>,
    /// The chip's answer to the chip nonce, if sent.
    pub active: Option<&'a [u8]>,
    /// Whether the phone reports chip authentication.
    pub chip_authenticated: bool,
    /// The pictures.
    pub frames: &'a [Vec<u8>],
}
