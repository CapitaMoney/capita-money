// Capita Money – capita-keys
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Whether a proof is a valid signature of a key over a challenge.
//!
//! For a passkey that means (WebAuthn level 2, "verifying an assertion",
//! as far as a chain can check it):
//! 1. the relying party hash in the authenticator data is one of the list;
//! 2. the flags say *user present* and *user verified*;
//! 3. the client data say `"type":"webauthn.get"`, carry the challenge as
//!    base64url and an origin of the list, and are not `crossOrigin`;
//! 4. the ECDSA signature over `authenticatorData ‖ SHA-256(clientDataJSON)`
//!    fits the key (either form of `s` is accepted).
//!
//! The signature counter is not checked: synced passkeys always report 0.

use alloc::vec::Vec;

use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};

use crate::encoding::{base64url, sha256};
use crate::types::{DeviceKey, KeyError, PasskeyProof, Proof};

/// Longest authenticator data a proof may carry.
pub const MAX_AUTHENTICATOR_DATA: usize = 1024;
/// Longest client data a proof may carry.
pub const MAX_CLIENT_DATA: usize = 2048;

/// Flag *user present* of the authenticator data.
pub const FLAG_USER_PRESENT: u8 = 0x01;
/// Flag *user verified* (fingerprint, face, screen lock).
pub const FLAG_USER_VERIFIED: u8 = 0x04;

/// The signature check of Ed25519, supplied by the host.
pub trait Verify {
    /// Whether `signature` is a valid Ed25519 signature of `key` over
    /// `message`.
    fn ed25519(key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool;
}

/// Checks that `proof` is a signature of `key` over `challenge`.
/// `relying_parties` are the SHA-256 hashes of the allowed relying party
/// ids; `origin_allowed` says whether an origin is on the list.
///
/// # Errors
/// Why the proof does not count ([`KeyError`]).
pub fn verify_proof<V: Verify>(
    key: &DeviceKey,
    challenge: &[u8; 32],
    proof: &Proof,
    relying_parties: &[[u8; 32]],
    origin_allowed: impl Fn(&[u8]) -> bool,
) -> Result<(), KeyError> {
    match (key, proof) {
        (DeviceKey::Ed25519(public), Proof::Ed25519(signature)) => {
            if V::ed25519(public, challenge, signature) {
                Ok(())
            } else {
                Err(KeyError::BadSignature)
            }
        }
        (DeviceKey::Passkey(public), Proof::Passkey(assertion)) => verify_passkey(
            public,
            challenge,
            assertion,
            relying_parties,
            origin_allowed,
        ),
        (DeviceKey::Hardware(public), Proof::Hardware(signature)) => {
            verify_hardware(public, challenge, signature)
        }
        _ => Err(KeyError::KindMismatch),
    }
}

/// Checks a passkey assertion (see the module).
///
/// # Errors
/// Why the assertion does not count ([`KeyError`]).
pub fn verify_passkey(
    public: &[u8; 33],
    challenge: &[u8; 32],
    proof: &PasskeyProof,
    relying_parties: &[[u8; 32]],
    origin_allowed: impl Fn(&[u8]) -> bool,
) -> Result<(), KeyError> {
    let data = proof.authenticator_data.as_slice();
    let client = proof.client_data_json.as_slice();
    if data.len() < 37 || data.len() > MAX_AUTHENTICATOR_DATA || client.len() > MAX_CLIENT_DATA {
        return Err(KeyError::Malformed);
    }
    let rp = data.get(..32).ok_or(KeyError::Malformed)?;
    if !relying_parties.iter().any(|h| h.as_slice() == rp) {
        return Err(KeyError::WrongRelyingParty);
    }
    let flags = *data.get(32).ok_or(KeyError::Malformed)?;
    if flags & FLAG_USER_PRESENT == 0 || flags & FLAG_USER_VERIFIED == 0 {
        return Err(KeyError::NotVerified);
    }
    // Escapes are never needed in the fields read here; refusing them keeps
    // the reading simple and unambiguous.
    if client.contains(&b'\\') {
        return Err(KeyError::Malformed);
    }
    if json_string(client, b"type") != Some(b"webauthn.get".as_slice()) {
        return Err(KeyError::WrongType);
    }
    if json_string(client, b"challenge") != Some(base64url(challenge).as_slice()) {
        return Err(KeyError::WrongChallenge);
    }
    let origin = json_string(client, b"origin").ok_or(KeyError::WrongOrigin)?;
    if !origin_allowed(origin) {
        return Err(KeyError::WrongOrigin);
    }
    if json_value(client, b"crossOrigin").is_some_and(|v| v.starts_with(b"true")) {
        return Err(KeyError::CrossOrigin);
    }
    let key = p256_key(public)?;
    let signature = Signature::from_slice(&proof.signature).map_err(|_| KeyError::BadSignature)?;
    let signature = signature.normalize_s().unwrap_or(signature);
    let mut message = Vec::with_capacity(data.len().saturating_add(32));
    message.extend_from_slice(data);
    message.extend_from_slice(&sha256(client));
    key.verify(&message, &signature)
        .map_err(|_| KeyError::BadSignature)
}

/// A P-256 public key in the one form the chain lists: compressed SEC1,
/// first byte 2 or 3. Other forms of the same point (SEC1 "compact", tag 5)
/// are refused, so one key never stands under two names in the key lists.
fn p256_key(public: &[u8; 33]) -> Result<VerifyingKey, KeyError> {
    if !matches!(public.first(), Some(2 | 3)) {
        return Err(KeyError::BadKey);
    }
    VerifyingKey::from_sec1_bytes(public).map_err(|_| KeyError::BadKey)
}

/// Checks a hardware key's signature: ECDSA on P-256 with SHA-256 over the
/// 32 bytes of the challenge – what Android's `SHA256withECDSA` and Apple's
/// `ecdsaSignatureMessageX962SHA256` produce, turned into `r ‖ s`. High `s`
/// is accepted and normalised, as for passkeys.
///
/// # Errors
/// [`KeyError::BadKey`] for a point off the curve, else
/// [`KeyError::BadSignature`].
pub fn verify_hardware(
    public: &[u8; 33],
    challenge: &[u8; 32],
    signature: &[u8; 64],
) -> Result<(), KeyError> {
    let key = p256_key(public)?;
    let signature = Signature::from_slice(signature).map_err(|_| KeyError::BadSignature)?;
    let signature = signature.normalize_s().unwrap_or(signature);
    key.verify(challenge, &signature)
        .map_err(|_| KeyError::BadSignature)
}

/// The text after `"name"`, optional spaces, `:` and optional spaces – of
/// the first occurrence of `"name"` followed by a colon.
fn json_value<'a>(json: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let mut needle = Vec::with_capacity(name.len().saturating_add(2));
    needle.push(b'"');
    needle.extend_from_slice(name);
    needle.push(b'"');
    let mut rest = json;
    loop {
        let at = rest
            .windows(needle.len())
            .position(|w| w == needle.as_slice())?;
        let after = rest.get(at.saturating_add(needle.len())..)?;
        let after = skip_spaces(after);
        if let Some(value) = after.strip_prefix(b":") {
            return Some(skip_spaces(value));
        }
        rest = after;
    }
}

/// The string value of the field `name`, without its quotes.
fn json_string<'a>(json: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let value = json_value(json, name)?.strip_prefix(b"\"")?;
    let end = value.iter().position(|&c| c == b'"')?;
    value.get(..end)
}

fn skip_spaces(text: &[u8]) -> &[u8] {
    let start = text
        .iter()
        .position(|c| !matches!(c, b' ' | b'\t' | b'\n' | b'\r'))
        .unwrap_or(text.len());
    text.get(start..).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{json_string, json_value};

    #[test]
    fn reads_fields_of_client_data() {
        let json = br#"{"type":"webauthn.get", "challenge" : "abc","origin":"https://x","crossOrigin":false}"#;
        assert_eq!(json_string(json, b"type"), Some(b"webauthn.get".as_slice()));
        assert_eq!(json_string(json, b"challenge"), Some(b"abc".as_slice()));
        assert_eq!(json_string(json, b"origin"), Some(b"https://x".as_slice()));
        assert_eq!(
            json_value(json, b"crossOrigin").map(|v| v.starts_with(b"false")),
            Some(true)
        );
        assert_eq!(json_string(json, b"topOrigin"), None);
    }

    #[test]
    fn a_name_used_as_value_is_not_a_field() {
        let json = br#"{"x":"type","type":"webauthn.get"}"#;
        assert_eq!(json_string(json, b"type"), Some(b"webauthn.get".as_slice()));
    }
}
