// SPDX-License-Identifier: AGPL-3.0-only
//! The chip proves itself (ADR 0024): active authentication with DG15,
//! the report of chip authentication where DG14 offers it, files the
//! security object lists may not be left out. Test chip: fixtures/proof
//! (`fixtures/make-proof.sh`).

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use capita_mrtd::aa::{active_authentication, needs_chip_authentication};
use capita_mrtd::pa::TrustList;
use capita_mrtd::{check_chip, ChipFiles, ChipProof, MrtdError};
use openssl::bn::{BigNum, BigNumContext};
use openssl::ecdsa::EcdsaSig;
use openssl::hash::{hash, MessageDigest};
use openssl::pkey::PKey;
use openssl::rsa::{Padding, Rsa};
use openssl::x509::X509;

const SOD: &[u8] = include_bytes!("fixtures/proof/sod.bin");
const DG1: &[u8] = include_bytes!("fixtures/dg1.bin");
const DG2: &[u8] = include_bytes!("fixtures/dg2.bin");
const DG14: &[u8] = include_bytes!("fixtures/proof/dg14.bin");
const DG15: &[u8] = include_bytes!("fixtures/proof/dg15.bin");
const CSCA: &[u8] = include_bytes!("fixtures/proof/csca.pem");
const OTHER: &[u8] = include_bytes!("fixtures/csca.pem");
const AA_KEY: &[u8] = include_bytes!("fixtures/proof/aa.key");
const NONCE: &[u8] = &[1, 2, 3, 4, 5, 6, 7, 8];

fn trust(pem: &[u8]) -> TrustList {
    TrustList::new(X509::stack_from_pem(pem).unwrap()).unwrap()
}

/// What the test chip answers to `nonce`: ECDSA with SHA-256, plain r ‖ s.
fn chip_answer(nonce: &[u8]) -> Vec<u8> {
    let key = openssl::ec::EcKey::private_key_from_pem(AA_KEY).unwrap();
    let digest = hash(MessageDigest::sha256(), nonce).unwrap();
    let sig = EcdsaSig::sign(&digest, &key).unwrap();
    let mut out = sig.r().to_vec_padded(32).unwrap();
    out.extend_from_slice(&sig.s().to_vec_padded(32).unwrap());
    out
}

fn files() -> ChipFiles<'static> {
    ChipFiles {
        sod: SOD,
        dg1: DG1,
        dg2: DG2,
        dg14: Some(DG14),
        dg15: Some(DG15),
    }
}

#[test]
fn a_chip_that_proves_itself_passes() {
    let answer = chip_answer(NONCE);
    let proof = ChipProof {
        nonce: NONCE,
        active: Some(&answer),
        chip_authenticated: true,
    };
    let chip = check_chip(&files(), &proof, &trust(CSCA)).unwrap();
    assert_eq!(chip.authenticated.groups, vec![1, 2, 14, 15]);
    assert_eq!(chip.authenticated.listed, vec![1, 2, 14, 15]);
    assert_eq!(chip.mrz.number, "L898902C3");
}

#[test]
fn copies_and_cut_files_are_refused() {
    let answer = chip_answer(NONCE);
    let good = ChipProof {
        nonce: NONCE,
        active: Some(&answer),
        chip_authenticated: true,
    };
    let trust = trust(CSCA);

    // DG15 left out so active authentication is skipped
    let mut cut = files();
    cut.dg15 = None;
    assert_eq!(
        check_chip(&cut, &good, &trust).unwrap_err(),
        MrtdError::MissingFile(15)
    );
    let mut cut = files();
    cut.dg14 = None;
    assert_eq!(
        check_chip(&cut, &good, &trust).unwrap_err(),
        MrtdError::MissingFile(14)
    );

    // no answer, an answer to another nonce, an old answer replayed
    let mut p = good;
    p.active = None;
    assert_eq!(
        check_chip(&files(), &p, &trust).unwrap_err(),
        MrtdError::ChipNotProven
    );
    let other = chip_answer(&[9; 8]);
    let mut p = good;
    p.active = Some(&other);
    assert_eq!(
        check_chip(&files(), &p, &trust).unwrap_err(),
        MrtdError::ChipNotProven
    );

    // chip authentication offered by DG14 but not reported
    let mut p = good;
    p.chip_authenticated = false;
    assert_eq!(
        check_chip(&files(), &p, &trust).unwrap_err(),
        MrtdError::ChipNotProven
    );

    // a country that did not sign it
    assert_eq!(
        check_chip(&files(), &good, &self::trust(OTHER)).unwrap_err(),
        MrtdError::Untrusted
    );
}

#[test]
fn dg14_says_whether_chip_authentication_is_offered() {
    assert!(needs_chip_authentication(DG14).unwrap());
    // only an ActiveAuthenticationInfo
    let aa_only = [
        0x6e, 0x14, 0x31, 0x12, 0x30, 0x10, 0x06, 0x06, 0x67, 0x81, 0x08, 0x01, 0x01, 0x05, 0x02,
        0x01, 0x01, 0x06, 0x03, 0x2a, 0x86, 0x48,
    ];
    assert!(!needs_chip_authentication(&aa_only).unwrap());
}

#[test]
fn ec_active_authentication_needs_its_hash_from_dg14() {
    let answer = chip_answer(NONCE);
    assert_eq!(
        active_authentication(DG15, Some(DG14), NONCE, &answer),
        Ok(())
    );
    assert!(matches!(
        active_authentication(DG15, None, NONCE, &answer),
        Err(MrtdError::Malformed(_))
    ));
}

/// An RSA chip: ISO/IEC 9796-2 scheme 1, block 6A ‖ M1 ‖ H(M1 ‖ nonce) ‖ trailer.
fn rsa_answer(
    rsa: &Rsa<openssl::pkey::Private>,
    nonce: &[u8],
    md: MessageDigest,
    trailer: &[u8],
) -> Vec<u8> {
    let k = rsa.size() as usize;
    let m1: Vec<u8> = (0..k - 1 - md.size() - trailer.len())
        .map(|i| i.to_le_bytes()[0].wrapping_mul(7).wrapping_add(3))
        .collect();
    let mut message = m1.clone();
    message.extend_from_slice(nonce);
    let mut block = vec![0x6a];
    block.extend_from_slice(&m1);
    block.extend_from_slice(&hash(md, &message).unwrap());
    block.extend_from_slice(trailer);
    let mut out = vec![0; k];
    let n = rsa
        .private_encrypt(&block, &mut out, Padding::NONE)
        .unwrap();
    out.truncate(n);
    out
}

fn dg15_of(rsa: &Rsa<openssl::pkey::Private>) -> Vec<u8> {
    let spki = PKey::from_rsa(rsa.clone())
        .unwrap()
        .public_key_to_der()
        .unwrap();
    let [hi, lo] = u16::try_from(spki.len()).unwrap().to_be_bytes();
    let mut out = vec![0x6f, 0x82, hi, lo];
    out.extend_from_slice(&spki);
    out
}

#[test]
fn rsa_chips_answer_by_iso_9796_2() {
    let rsa = Rsa::generate(2048).unwrap();
    let dg15 = dg15_of(&rsa);
    for (md, trailer) in [
        (MessageDigest::sha1(), vec![0xbc]),
        (MessageDigest::sha1(), vec![0x33, 0xcc]),
        (MessageDigest::sha256(), vec![0x34, 0xcc]),
    ] {
        let answer = rsa_answer(&rsa, NONCE, md, &trailer);
        assert_eq!(active_authentication(&dg15, None, NONCE, &answer), Ok(()));
        assert_eq!(
            active_authentication(&dg15, None, &[0; 8], &answer),
            Err(MrtdError::ChipNotProven)
        );
    }
    // n − m: the same block, the other representative (ISO/IEC 9796-2 allows both)
    let answer = rsa_answer(&rsa, NONCE, MessageDigest::sha1(), &[0xbc]);
    let s = BigNum::from_slice(&answer).unwrap();
    let mut d = BigNum::new().unwrap();
    let mut ctx = BigNumContext::new().unwrap();
    let mut m = BigNum::new().unwrap();
    m.mod_exp(&s, rsa.e(), rsa.n(), &mut ctx).unwrap();
    d.checked_sub(rsa.n(), &m).unwrap();
    let mut twin = BigNum::new().unwrap();
    twin.mod_exp(&d, rsa.d(), rsa.n(), &mut ctx).unwrap();
    assert_eq!(
        active_authentication(&dg15, None, NONCE, &twin.to_vec()),
        Ok(())
    );
    // a different key's answer
    let stranger = Rsa::generate(2048).unwrap();
    let answer = rsa_answer(&stranger, NONCE, MessageDigest::sha1(), &[0xbc]);
    assert!(active_authentication(&dg15, None, NONCE, &answer).is_err());
}
