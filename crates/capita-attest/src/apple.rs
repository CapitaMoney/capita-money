// Capita Money – capita-attest
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Apple App Attest, as the identity service checks it (ADR 0024).
//!
//! For each verification the iPhone app makes a fresh App Attest key and
//! asks Apple to certify it with the service's nonce (the *attestation
//! object*); then the key signs the request (an *assertion*). The service
//! keeps neither: key and counter live for one request. Steps as Apple
//! describes them ("Validating apps that connect to your server").

use std::cmp::Ordering;

use ciborium::Value;
use openssl::asn1::Asn1Time;
use openssl::pkey::{PKey, Public};
use openssl::x509::{X509Ref, X509};

use crate::der::{self, tag, Class};
use crate::{linked, sha256, signed, AttestError};

/// 1.2.840.113635.100.8.2, the nonce Apple certified.
pub const NONCE_EXTENSION: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x63, 0x64, 0x08, 0x02];
/// AAGUID of keys of the production environment.
pub const PRODUCTION: &[u8; 16] = b"appattest\0\0\0\0\0\0\0";
/// AAGUID of keys of the development environment.
pub const DEVELOPMENT: &[u8; 16] = b"appattestdevelop";

/// What the identity service accepts.
#[derive(Clone)]
pub struct Policy {
    /// Apple App Attestation Root CA.
    pub root: X509,
    /// App ids: team id + "." + bundle id.
    pub app_ids: Vec<String>,
    /// Whether keys of the development environment count (test builds).
    pub development: bool,
}

/// A certified App Attest key.
#[derive(Clone, Debug)]
pub struct AppKey {
    /// The key.
    pub key: PKey<Public>,
    /// The app it belongs to.
    pub app_id: String,
    /// Whether it is a development key.
    pub development: bool,
}

fn get<'a>(map: &'a [(Value, Value)], name: &str) -> Option<&'a Value> {
    map.iter()
        .find(|(k, _)| k.as_text() == Some(name))
        .map(|(_, v)| v)
}

fn cbor_map(bytes: &[u8], what: &'static str) -> Result<Vec<(Value, Value)>, AttestError> {
    let value: Value = ciborium::from_reader(bytes).map_err(|_| AttestError::Malformed(what))?;
    value.into_map().map_err(|_| AttestError::Malformed(what))
}

fn bytes_of<'a>(
    map: &'a [(Value, Value)],
    name: &str,
    what: &'static str,
) -> Result<&'a [u8], AttestError> {
    get(map, name)
        .and_then(Value::as_bytes)
        .map(Vec::as_slice)
        .ok_or(AttestError::Malformed(what))
}

fn within(certificate: &X509Ref, now: &Asn1Time) -> Result<bool, AttestError> {
    Ok(certificate.not_before().compare(now)? != Ordering::Greater
        && certificate.not_after().compare(now)? != Ordering::Less)
}

fn app_of(rp_hash: &[u8], app_ids: &[String]) -> Result<String, AttestError> {
    for id in app_ids {
        if sha256(id.as_bytes())?.as_slice() == rp_hash {
            return Ok(id.clone());
        }
    }
    Err(AttestError::WrongApp)
}

fn counter(data: &[u8]) -> Result<u32, AttestError> {
    let bytes: [u8; 4] = data
        .get(33..37)
        .and_then(|b| b.try_into().ok())
        .ok_or(AttestError::Malformed("authenticator data"))?;
    Ok(u32::from_be_bytes(bytes))
}

/// Checks an attestation object: Apple certified the key `key_id` of an
/// allowed app for `challenge`, at Unix time `now`.
///
/// # Errors
/// Why the key does not pass ([`AttestError`]).
pub fn attestation(
    object: &[u8],
    key_id: &[u8],
    challenge: &[u8],
    now: i64,
    policy: &Policy,
) -> Result<AppKey, AttestError> {
    let map = cbor_map(object, "attestation object")?;
    if get(&map, "fmt").and_then(Value::as_text) != Some("apple-appattest") {
        return Err(AttestError::Malformed("attestation format"));
    }
    let statement = get(&map, "attStmt")
        .and_then(Value::as_map)
        .ok_or(AttestError::Malformed("attestation statement"))?;
    let chain = get(statement, "x5c")
        .and_then(Value::as_array)
        .ok_or(AttestError::Malformed("certificates"))?;
    let chain = chain
        .iter()
        .map(|c| c.as_bytes().ok_or(AttestError::Malformed("certificate")))
        .collect::<Result<Vec<_>, _>>()?;
    let [credential, intermediate] = chain.as_slice() else {
        return Err(AttestError::Malformed("two certificates"));
    };
    let data = bytes_of(&map, "authData", "authenticator data")?;

    let credential_cert =
        X509::from_der(credential).map_err(|_| AttestError::Malformed("certificate"))?;
    let intermediate_cert =
        X509::from_der(intermediate).map_err(|_| AttestError::Malformed("certificate"))?;
    linked(&[&credential_cert, &intermediate_cert, &policy.root])?;
    let now = Asn1Time::from_unix(now)?;
    for certificate in [
        credential_cert.as_ref(),
        intermediate_cert.as_ref(),
        policy.root.as_ref(),
    ] {
        if !within(certificate, &now)? {
            return Err(AttestError::Expired);
        }
    }

    let mut signed_data = data.to_vec();
    signed_data.extend_from_slice(&sha256(challenge)?);
    let nonce = sha256(&signed_data)?;
    let extension =
        der::extension(credential, NONCE_EXTENSION)?.ok_or(AttestError::Malformed("no nonce"))?;
    let outer = der::one(extension)?.inside(tag::SEQUENCE, "nonce")?;
    let wrapped = outer
        .iter()
        .find(|e| e.class == Class::Context && e.tag == 1)
        .ok_or(AttestError::Malformed("nonce"))?;
    let certified = der::one(wrapped.value)?.bytes(tag::OCTET_STRING, "nonce")?;
    if certified != nonce {
        return Err(AttestError::Challenge);
    }

    let key = credential_cert.public_key()?;
    let ec = key.ec_key().map_err(|_| AttestError::WrongKey)?;
    let mut context = openssl::bn::BigNumContext::new()?;
    let point = ec.public_key().to_bytes(
        ec.group(),
        openssl::ec::PointConversionForm::UNCOMPRESSED,
        &mut context,
    )?;
    if sha256(&point)?.as_slice() != key_id {
        return Err(AttestError::WrongKey);
    }

    let rp = data
        .get(..32)
        .ok_or(AttestError::Malformed("authenticator data"))?;
    let app_id = app_of(rp, &policy.app_ids)?;
    if counter(data)? != 0 {
        return Err(AttestError::Counter);
    }
    let aaguid = data
        .get(37..53)
        .ok_or(AttestError::Malformed("authenticator data"))?;
    let development = if aaguid == PRODUCTION {
        false
    } else if aaguid == DEVELOPMENT && policy.development {
        true
    } else {
        return Err(AttestError::WrongApp);
    };
    let length = data
        .get(53..55)
        .and_then(|b| <[u8; 2]>::try_from(b).ok())
        .map(u16::from_be_bytes)
        .ok_or(AttestError::Malformed("authenticator data"))?;
    let end = 55usize
        .checked_add(usize::from(length))
        .ok_or(AttestError::Malformed("authenticator data"))?;
    let credential_id = data
        .get(55..end)
        .ok_or(AttestError::Malformed("authenticator data"))?;
    if credential_id != key_id {
        return Err(AttestError::WrongKey);
    }
    Ok(AppKey {
        key,
        app_id,
        development,
    })
}

/// Checks an assertion: `key` signed `client_data` with a counter above
/// `after`. Answers the counter.
///
/// # Errors
/// Why the assertion does not count ([`AttestError`]).
pub fn assertion(
    object: &[u8],
    client_data: &[u8],
    key: &AppKey,
    after: u32,
) -> Result<u32, AttestError> {
    let map = cbor_map(object, "assertion")?;
    let signature = bytes_of(&map, "signature", "assertion signature")?;
    let data = bytes_of(&map, "authenticatorData", "assertion data")?;
    let mut message = data.to_vec();
    message.extend_from_slice(&sha256(client_data)?);
    let nonce = sha256(&message)?;
    if !signed(&key.key, &nonce, signature) {
        return Err(AttestError::BadSignature);
    }
    let rp = data
        .get(..32)
        .ok_or(AttestError::Malformed("assertion data"))?;
    if sha256(key.app_id.as_bytes())?.as_slice() != rp {
        return Err(AttestError::WrongApp);
    }
    let n = counter(data)?;
    if n <= after {
        return Err(AttestError::Counter);
    }
    Ok(n)
}

/// Both at once, as one verification sends them: a fresh key certified for
/// `challenge`, then its first assertion over `client_data`.
///
/// # Errors
/// Why the phone does not pass ([`AttestError`]).
pub fn check(
    attestation_object: &[u8],
    key_id: &[u8],
    assertion_object: &[u8],
    challenge: &[u8],
    client_data: &[u8],
    now: i64,
    policy: &Policy,
) -> Result<AppKey, AttestError> {
    let key = attestation(attestation_object, key_id, challenge, now, policy)?;
    assertion(assertion_object, client_data, &key, 0)?;
    Ok(key)
}
