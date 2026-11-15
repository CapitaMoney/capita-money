// SPDX-License-Identifier: AGPL-3.0-only
//! The phone check of the identity service (ADR 0024): the shipped roots
//! load, the revocation list reads, the request digest is the one the app
//! computes (`app/tests/verify.test.mjs` checks the same vector), and proofs
//! that do not fit are refused. The attestation logic itself is tested in
//! `capita-attest`.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

use std::path::PathBuf;

use capita_attest::AttestError;
use capita_issuer::phone::{
    request_digest, revoked_serials, DigestParts, PhoneCheck, PhoneConfig, PhoneProof,
};

fn trust(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("trust/attestation")
        .join(path)
}

fn config() -> PhoneConfig {
    PhoneConfig {
        android_roots: trust("google"),
        android_revoked: None,
        packages: vec!["com.capitamoney.app".into()],
        signers: vec!["AB:CD:".repeat(16).trim_end_matches(':').into()],
        boot_keys: vec![],
        strongbox_only: false,
        apple_root: Some(trust("apple-app-attestation-root-ca.pem")),
        apple_app_ids: vec![],
        apple_development: false,
    }
}

#[test]
fn the_shipped_roots_load() {
    let check = PhoneCheck::load(&config()).unwrap();
    // no iPhone app id yet: iPhones cannot verify until the Apple account exists
    assert_eq!(check.kinds(), vec!["android"]);
    let mut with_apple = config();
    with_apple.apple_app_ids = vec!["ABCDE12345.com.capitamoney.app".into()];
    assert_eq!(
        PhoneCheck::load(&with_apple).unwrap().kinds(),
        vec!["android", "apple"]
    );
}

#[test]
fn a_phone_check_needs_the_app_named() {
    let mut c = config();
    c.signers.clear();
    assert!(PhoneCheck::load(&c).is_err());
    let mut c = config();
    c.signers = vec!["not hex".into()];
    assert!(PhoneCheck::load(&c).is_err());
}

#[test]
fn googles_status_list_reads() {
    let list = r#"{"entries": {"2c8cdddfd5e03bfc": {"status": "REVOKED", "reason": "KEY_COMPROMISE"},
                   "00C35848C9A9E28C": {"status": "SUSPENDED"}}}"#;
    let serials = revoked_serials(list).unwrap();
    assert!(serials.contains("2c8cdddfd5e03bfc"));
    assert!(serials.contains("c35848c9a9e28c"));
    assert!(revoked_serials("[]").is_err());
}

#[test]
fn the_request_digest_is_the_apps() {
    let nonce: [u8; 32] = core::array::from_fn(|i| u8::try_from(i + 1).unwrap());
    let frames = vec![b"f1".to_vec(), b"f2".to_vec()];
    let digest = request_digest(
        &nonce,
        "0123456789abcdef0123456789abcdef",
        &DigestParts {
            sod: b"sod",
            dg1: b"dg1",
            dg2: b"dg2",
            dg14: None,
            dg15: Some(b"dg15"),
            active: Some(b"aa"),
            chip_authenticated: true,
            frames: &frames,
        },
    );
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex,
        "750324a761902b2b4af429a920946d9a85cd61d5dda2584cbd98968e30f825b0"
    );
}

#[test]
fn proofs_that_do_not_fit_are_refused() {
    let check = PhoneCheck::load(&config()).unwrap();
    let nonce = [7u8; 32];
    let digest = [9u8; 32];
    let garbage = PhoneProof::Android {
        chain: vec![vec![0x30, 0x00], vec![0x30, 0x00]],
        signature: vec![],
    };
    assert!(matches!(
        check.check(&garbage, &nonce, &digest, 1_794_700_000),
        Err(AttestError::Malformed(_))
    ));
    // an iPhone proof while no iPhone app id is listed
    let iphone = PhoneProof::Apple {
        key_id: vec![0; 32],
        attestation: vec![0xa0],
        assertion: vec![0xa0],
    };
    let mut no_apple = config();
    no_apple.apple_root = None;
    assert_eq!(
        PhoneCheck::load(&no_apple)
            .unwrap()
            .check(&iphone, &nonce, &digest, 1_794_700_000),
        Err(AttestError::Untrusted)
    );
}
