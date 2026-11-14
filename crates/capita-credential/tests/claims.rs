// Capita Money – capita-credential
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

//! Worked examples: which claims count, which are refused, and why.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    missing_docs
)]

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use capita_core::attributes::{AttrKey, AttrValue, Attribute, Residence, VerificationLevel};
use capita_core::types::{AccountId, CountryCode, Duration, Timestamp};
use capita_credential::check::{
    attestation, check_claim, check_revocation, person_id, still_counts, Crypto,
};
use capita_credential::did::{check_did, did_key};
use capita_credential::encoding::{claim_bytes, person_bytes, revocation_bytes};
use capita_credential::errors::CredentialError;
use capita_credential::types::{
    Audience, Claim, Issuer, IssuerId, IssuerStatus, PublicKey, Realm, Revocation, Serial,
    Signature, SubjectRef, MAX_ATTRIBUTES, MAX_ATTRIBUTE_KEYS, MAX_KEYS,
};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};

/// The host of these tests: Ed25519 by a library, BLAKE2b-256. No Sr25519.
struct Host;

impl Crypto for Host {
    fn verify(key: &PublicKey, message: &[u8], signature: &Signature) -> bool {
        match (key, signature) {
            (PublicKey::Ed25519(key), Signature::Ed25519(signature)) => {
                VerifyingKey::from_bytes(key).is_ok_and(|key| {
                    key.verify(message, &ed25519_dalek::Signature::from_bytes(signature))
                        .is_ok()
                })
            }
            _ => false,
        }
    }

    fn hash(data: &[u8]) -> [u8; 32] {
        Blake2b::<U32>::digest(data).into()
    }
}

const NOW: u64 = 1_800_000_000;
const DAY: u64 = 86_400;
const AUDIENCE: Audience = Audience([7; 32]);
const ID: IssuerId = IssuerId(3);

fn at(offset_days: i64) -> Timestamp {
    Timestamp::from_secs(NOW.checked_add_signed(offset_days * 86_400).unwrap())
}

fn secret(n: u8) -> SigningKey {
    SigningKey::from_bytes(&[n; 32])
}

fn public(n: u8) -> PublicKey {
    PublicKey::Ed25519(secret(n).verifying_key().to_bytes())
}

/// An identity service that reads documents and sends letters: level up to
/// 3, residence, attribute 1; claims hold two years at most.
fn issuer() -> Issuer {
    Issuer {
        did: b"did:web:id.example.org".to_vec(),
        keys: vec![public(1)],
        realm: Realm(1),
        max_level: VerificationLevel(3),
        residence: true,
        attributes: vec![AttrKey(1)],
        max_validity: Some(Duration::from_secs(2 * 365 * DAY)),
        status: IssuerStatus::Active,
    }
}

/// Level 2, resident of France, attribute 1 = 42; issued yesterday, valid
/// one year.
fn claim() -> Claim {
    Claim {
        issuer: ID,
        serial: Serial([9; 16]),
        subject: AccountId([5; 32]),
        subject_ref: SubjectRef([6; 32]),
        replaces: None,
        level: VerificationLevel(2),
        residence: Some(Residence {
            country: CountryCode(*b"FR"),
            proven_at: at(-30),
        }),
        attributes: vec![Attribute {
            key: AttrKey(1),
            value: AttrValue(42),
        }],
        issued_at: at(-1),
        expires_at: Some(at(364)),
    }
}

fn sign(n: u8, bytes: &[u8]) -> Signature {
    Signature::Ed25519(secret(n).sign(bytes).to_bytes())
}

fn signed(claim: &Claim) -> Signature {
    sign(1, &claim_bytes(&AUDIENCE, claim))
}

/// Checks `claim`, signed by key 1 for the audience of the tests, against
/// `issuer` at `NOW`.
fn check(claim: &Claim, issuer: &Issuer) -> Result<(), CredentialError> {
    check_claim::<Host>(claim, &signed(claim), ID, issuer, &AUDIENCE, at(0)).map(|_| ())
}

#[test]
fn a_valid_claim_becomes_an_attestation() {
    let (claim, issuer) = (claim(), issuer());
    let stated = check_claim::<Host>(&claim, &signed(&claim), ID, &issuer, &AUDIENCE, at(0));
    let stated = stated.unwrap();
    assert_eq!(
        stated.person,
        person_id::<Host>(Realm(1), &SubjectRef([6; 32]))
    );
    assert_eq!(stated.level, VerificationLevel(2));
    assert_eq!(stated.residence, claim.residence);
    assert_eq!(stated.attributes, claim.attributes);
    assert_eq!(stated.issued_at, at(-1));
    assert_eq!(stated.expires_at, Some(at(364)));
    // the stored claim says the same later, without its signature
    assert_eq!(attestation::<Host>(&claim, issuer.realm), stated);
}

#[test]
fn the_signed_bytes_are_fixed() {
    // A test vector for issuers written in other languages.
    let bytes = claim_bytes(&AUDIENCE, &claim());
    let mut expected = Vec::new();
    expected.extend_from_slice(b"capita-claim-v2");
    expected.extend_from_slice(&[7; 32]); // audience
    expected.extend_from_slice(&[0, 0, 0, 3]); // issuer
    expected.extend_from_slice(&[9; 16]); // serial
    expected.extend_from_slice(&[5; 32]); // subject
    expected.extend_from_slice(&[6; 32]); // subject_ref
    expected.push(0); // replaces nothing
    expected.push(2); // level
    expected.push(1); // residence present
    expected.extend_from_slice(b"FR");
    expected.extend_from_slice(&(NOW - 30 * DAY).to_be_bytes());
    expected.extend_from_slice(&[0, 1]); // one attribute
    expected.extend_from_slice(&[0, 1]); // key 1
    expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 42]); // value 42
    expected.extend_from_slice(&(NOW - DAY).to_be_bytes());
    expected.push(1); // expiry present
    expected.extend_from_slice(&(NOW + 364 * DAY).to_be_bytes());
    assert_eq!(bytes, expected);
    assert_eq!(
        bytes.len(),
        15 + 32 + 4 + 16 + 32 + 32 + 1 + 1 + 11 + 2 + 10 + 8 + 9
    );

    // without residence, attributes and expiry
    let bare = Claim {
        residence: None,
        attributes: vec![],
        expires_at: None,
        ..claim()
    };
    let bytes = claim_bytes(&AUDIENCE, &bare);
    assert_eq!(
        bytes.len(),
        15 + 32 + 4 + 16 + 32 + 32 + 1 + 1 + 1 + 2 + 8 + 1
    );
    assert_eq!(
        &bytes[bytes.len() - 12..],
        [&[0u8, 0, 0][..], &(NOW - DAY).to_be_bytes(), &[0]].concat()
    );

    // a claim that replaces an earlier person key: 32 bytes more, after
    // the person key
    let rebound = Claim {
        replaces: Some(SubjectRef([8; 32])),
        ..bare.clone()
    };
    let longer = claim_bytes(&AUDIENCE, &rebound);
    let at_replaces = 15 + 32 + 4 + 16 + 32 + 32;
    assert_eq!(longer.len(), bytes.len() + 32);
    assert_eq!(longer[at_replaces], 1);
    assert_eq!(&longer[at_replaces + 1..at_replaces + 33], &[8; 32]);
    assert_eq!(&longer[at_replaces + 33..], &bytes[at_replaces + 1..]);
    let stated = attestation::<Host>(&rebound, Realm(1));
    assert_eq!(
        stated.replaces,
        Some(person_id::<Host>(Realm(1), &SubjectRef([8; 32])))
    );
    assert_eq!(attestation::<Host>(&bare, Realm(1)).replaces, None);

    let revocation = Revocation {
        issuer: ID,
        serial: Serial([9; 16]),
    };
    let mut expected = b"capita-revocation-v1".to_vec();
    expected.extend_from_slice(&[7; 32]);
    expected.extend_from_slice(&[0, 0, 0, 3]);
    expected.extend_from_slice(&[9; 16]);
    assert_eq!(revocation_bytes(&AUDIENCE, &revocation), expected);

    let mut expected = b"capita-person-v1".to_vec();
    expected.extend_from_slice(&[0, 0, 0, 1]);
    expected.extend_from_slice(&[6; 32]);
    assert_eq!(person_bytes(Realm(1), &SubjectRef([6; 32])), expected);
}

#[test]
fn a_claim_counts_for_one_host_one_account_one_content() {
    let (claim, issuer) = (claim(), issuer());
    let signature = signed(&claim);
    let bad = Err(CredentialError::BadSignature);
    // another host
    let other = Audience([8; 32]);
    assert_eq!(
        check_claim::<Host>(&claim, &signature, ID, &issuer, &other, at(0)),
        bad
    );
    // any field changed after signing
    let changed: Vec<Claim> = vec![
        Claim {
            subject: AccountId([4; 32]),
            ..claim.clone()
        },
        Claim {
            subject_ref: SubjectRef([4; 32]),
            replaces: None,
            ..claim.clone()
        },
        Claim {
            level: VerificationLevel(3),
            ..claim.clone()
        },
        Claim {
            serial: Serial([8; 16]),
            ..claim.clone()
        },
        Claim {
            residence: None,
            ..claim.clone()
        },
        Claim {
            attributes: vec![],
            ..claim.clone()
        },
        Claim {
            issued_at: at(-2),
            ..claim.clone()
        },
        Claim {
            expires_at: Some(at(700)),
            ..claim.clone()
        },
    ];
    for changed in changed {
        assert_eq!(
            check_claim::<Host>(&changed, &signature, ID, &issuer, &AUDIENCE, at(0)),
            bad
        );
    }
    // signed by somebody who is not the issuer
    let forged = sign(2, &claim_bytes(&AUDIENCE, &claim));
    assert_eq!(
        check_claim::<Host>(&claim, &forged, ID, &issuer, &AUDIENCE, at(0)),
        bad
    );
    // a signature of another scheme never matches
    let Signature::Ed25519(raw) = signature else {
        unreachable!()
    };
    assert_eq!(
        check_claim::<Host>(
            &claim,
            &Signature::Sr25519(raw),
            ID,
            &issuer,
            &AUDIENCE,
            at(0)
        ),
        bad
    );
    // checked against the entry of another issuer
    assert_eq!(
        check_claim::<Host>(&claim, &signature, IssuerId(4), &issuer, &AUDIENCE, at(0)),
        Err(CredentialError::WrongIssuer)
    );
}

#[test]
fn any_key_of_the_issuer_signs_while_keys_are_replaced() {
    let claim = claim();
    let mut issuer = issuer();
    let by_new_key = sign(2, &claim_bytes(&AUDIENCE, &claim));
    let check = |issuer: &Issuer| {
        check_claim::<Host>(&claim, &by_new_key, ID, issuer, &AUDIENCE, at(0)).map(|_| ())
    };
    assert_eq!(check(&issuer), Err(CredentialError::BadSignature));
    issuer.keys.push(public(2));
    assert_eq!(check(&issuer), Ok(()));
    issuer.keys.remove(0);
    assert_eq!(check(&issuer), Ok(()));
}

#[test]
fn only_an_active_issuer_issues() {
    let claim = claim();
    for status in [IssuerStatus::Suspended, IssuerStatus::Revoked] {
        let issuer = Issuer { status, ..issuer() };
        assert_eq!(
            check(&claim, &issuer),
            Err(CredentialError::IssuerNotActive)
        );
    }
}

#[test]
fn time_limits() {
    let issuer = issuer();
    let with = |issued: i64, expires: Option<i64>| Claim {
        issued_at: at(issued),
        expires_at: expires.map(at),
        residence: None,
        ..claim()
    };
    // dated in the future
    assert_eq!(
        check(&with(1, Some(30)), &issuer),
        Err(CredentialError::NotYetValid)
    );
    // issued this very second
    assert_eq!(check(&with(0, Some(30)), &issuer), Ok(()));
    // expired: the end itself no longer counts
    assert_eq!(
        check(&with(-30, Some(0)), &issuer),
        Err(CredentialError::Expired)
    );
    assert_eq!(
        check(&with(-30, Some(-1)), &issuer),
        Err(CredentialError::Expired)
    );
    // ends before it begins
    assert_eq!(
        check(&with(-30, Some(-30)), &issuer),
        Err(CredentialError::ExpiryBeforeIssue)
    );
    // exactly as long as the issuer may state, and one day more
    assert_eq!(check(&with(-1, Some(729)), &issuer), Ok(()));
    assert_eq!(
        check(&with(-1, Some(730)), &issuer),
        Err(CredentialError::ValidityTooLong)
    );
    // without an end: only from an issuer without a limit
    assert_eq!(
        check(&with(-1, None), &issuer),
        Err(CredentialError::ValidityTooLong)
    );
    let unlimited = Issuer {
        max_validity: None,
        ..issuer.clone()
    };
    assert_eq!(check(&with(-1, None), &unlimited), Ok(()));
    assert_eq!(check(&with(-1, Some(100_000)), &unlimited), Ok(()));
    // a limit beyond the end of time limits nothing
    let endless = Issuer {
        max_validity: Some(Duration::from_secs(u64::MAX)),
        ..issuer
    };
    assert_eq!(check(&with(-1, Some(100_000)), &endless), Ok(()));
    assert_eq!(
        check(&with(-1, None), &endless),
        Err(CredentialError::ValidityTooLong)
    );
}

#[test]
fn an_issuer_states_no_more_than_it_is_trusted_for() {
    let issuer = issuer();
    // level
    for (level, expected) in [
        (0, Err(CredentialError::LevelNotAllowed)),
        (1, Ok(())),
        (3, Ok(())),
        (4, Err(CredentialError::LevelNotAllowed)),
    ] {
        let claim = Claim {
            level: VerificationLevel(level),
            ..claim()
        };
        assert_eq!(check(&claim, &issuer), expected);
    }
    // residence
    let no_letters = Issuer {
        residence: false,
        ..issuer.clone()
    };
    assert_eq!(
        check(&claim(), &no_letters),
        Err(CredentialError::ResidenceNotAllowed)
    );
    let mut later = claim();
    later.residence = Some(Residence {
        country: CountryCode(*b"FR"),
        proven_at: at(0),
    });
    assert_eq!(
        check(&later, &issuer),
        Err(CredentialError::ResidenceFromFuture)
    );
    // attributes
    let mut other = claim();
    other.attributes = vec![Attribute {
        key: AttrKey(2),
        value: AttrValue(1),
    }];
    assert_eq!(
        check(&other, &issuer),
        Err(CredentialError::AttributeNotAllowed)
    );
    let mut many = claim();
    many.attributes = vec![many.attributes[0]; MAX_ATTRIBUTES];
    assert_eq!(check(&many, &issuer), Ok(()));
    many.attributes.push(many.attributes[0]);
    assert_eq!(
        check(&many, &issuer),
        Err(CredentialError::TooManyAttributes)
    );
}

#[test]
fn one_person_per_realm() {
    let person = SubjectRef([6; 32]);
    // two issuers of one realm name the same person
    let first = issuer();
    let second = Issuer {
        did: b"did:web:other.example.net".to_vec(),
        keys: vec![public(2)],
        ..issuer()
    };
    assert_eq!(first.realm, second.realm);
    assert_eq!(
        attestation::<Host>(&claim(), first.realm).person,
        attestation::<Host>(&claim(), second.realm).person
    );
    // another realm, another person; another pseudonym, another person
    assert_ne!(
        person_id::<Host>(Realm(1), &person),
        person_id::<Host>(Realm(2), &person)
    );
    assert_ne!(
        person_id::<Host>(Realm(1), &person),
        person_id::<Host>(Realm(1), &SubjectRef([5; 32]))
    );
    // the identifier is the BLAKE2b-256 hash of tag, realm and pseudonym
    assert_eq!(
        person_id::<Host>(Realm(1), &person).0,
        Host::hash(&person_bytes(Realm(1), &person))
    );
}

#[test]
fn an_accepted_claim_counts_until_it_expires_or_its_issuer_is_struck_off() {
    let (claim, issuer) = (claim(), issuer());
    assert!(still_counts(&claim, &issuer, at(0)));
    assert!(still_counts(&claim, &issuer, at(363)));
    assert!(!still_counts(&claim, &issuer, at(364)));
    let suspended = Issuer {
        status: IssuerStatus::Suspended,
        ..issuer.clone()
    };
    assert!(still_counts(&claim, &suspended, at(0)));
    let revoked = Issuer {
        status: IssuerStatus::Revoked,
        ..issuer.clone()
    };
    assert!(!still_counts(&claim, &revoked, at(0)));
    let endless = Claim {
        expires_at: None,
        ..claim
    };
    assert!(still_counts(&endless, &issuer, at(100_000)));
}

#[test]
fn a_revocation_needs_the_signature_of_the_issuer() {
    let issuer = issuer();
    let revocation = Revocation {
        issuer: ID,
        serial: Serial([9; 16]),
    };
    let bytes = revocation_bytes(&AUDIENCE, &revocation);
    let check = |signature: &Signature, id: IssuerId, issuer: &Issuer, audience: &Audience| {
        check_revocation::<Host>(&revocation, signature, id, issuer, audience)
    };
    let good = sign(1, &bytes);
    assert_eq!(check(&good, ID, &issuer, &AUDIENCE), Ok(()));
    // a suspended or revoked issuer may still take back
    let revoked = Issuer {
        status: IssuerStatus::Revoked,
        ..issuer.clone()
    };
    assert_eq!(check(&good, ID, &revoked, &AUDIENCE), Ok(()));
    let bad = Err(CredentialError::BadSignature);
    assert_eq!(check(&sign(2, &bytes), ID, &issuer, &AUDIENCE), bad);
    assert_eq!(check(&good, ID, &issuer, &Audience([8; 32])), bad);
    assert_eq!(
        check(&good, IssuerId(4), &issuer, &AUDIENCE),
        Err(CredentialError::WrongIssuer)
    );
    // the signature under a claim is no revocation, and the other way round
    let claim = claim();
    assert_eq!(check(&signed(&claim), ID, &issuer, &AUDIENCE), bad);
    assert_eq!(
        check_claim::<Host>(&claim, &good, ID, &issuer, &AUDIENCE, at(0)).map(|_| ()),
        bad
    );
}

#[test]
fn did_key_is_the_key_as_text() {
    // Example of the did:key specification (Ed25519).
    let key: [u8; 32] = [
        0x2e, 0x6f, 0xcc, 0xe3, 0x67, 0x01, 0xdc, 0x79, 0x14, 0x88, 0xe0, 0xd0, 0xb1, 0x74, 0x5c,
        0xc1, 0xe3, 0x3a, 0x4c, 0x1c, 0x9f, 0xcc, 0x41, 0xc6, 0x3b, 0xd3, 0x43, 0xdb, 0xbe, 0x09,
        0x70, 0xe6,
    ];
    assert_eq!(
        did_key(&PublicKey::Ed25519(key)),
        b"did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK".to_vec()
    );
    // the same bytes as a key of another type give another identifier
    let sr = did_key(&PublicKey::Sr25519(key));
    assert_ne!(sr, did_key(&PublicKey::Ed25519(key)));
    assert!(sr.starts_with(b"did:key:z"));
    // a did:key must be the identifier of the one key of the issuer
    let ed = PublicKey::Ed25519(key);
    let did = did_key(&ed);
    assert_eq!(check_did(&did, &[ed]), Ok(()));
    let mismatch = Err(CredentialError::DidKeyMismatch);
    assert_eq!(check_did(&did, &[public(1)]), mismatch);
    assert_eq!(check_did(&did, &[ed, public(1)]), mismatch);
    assert_eq!(check_did(&did, &[PublicKey::Sr25519(key)]), mismatch);
}

#[test]
fn the_form_of_a_did() {
    let keys = [public(1)];
    for good in [
        &b"did:web:id.example.org"[..],
        b"did:web:example.org:issuers:7",
        b"did:web:localhost%3A8443",
        b"did:ebsi:zfEmvX5twhXjQJiCWsukvQA",
        b"did:x:y",
    ] {
        assert_eq!(
            check_did(good, &keys),
            Ok(()),
            "{}",
            String::from_utf8_lossy(good)
        );
    }
    for bad in [
        &b""[..],
        b"did:",
        b"did:web",
        b"did:web:",
        b"did::x",
        b"web:id.example.org",
        b"DID:web:id.example.org",
        b"did:Web:id.example.org",
        b"did:web:id.example.org/path",
        b"did:web:id example",
        "did:web:exämple.org".as_bytes(),
    ] {
        assert_eq!(
            check_did(bad, &keys),
            Err(CredentialError::InvalidDid),
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
    let mut long = b"did:web:".to_vec();
    long.resize(256, b'a');
    assert_eq!(check_did(&long, &keys), Ok(()));
    long.push(b'a');
    assert_eq!(check_did(&long, &keys), Err(CredentialError::InvalidDid));
}

#[test]
fn an_entry_of_the_issuer_list_is_checked() {
    assert_eq!(issuer().validate(), Ok(()));
    let with_keys = |n: usize| Issuer {
        keys: vec![public(1); n],
        ..issuer()
    };
    assert_eq!(with_keys(0).validate(), Err(CredentialError::InvalidKeys));
    assert_eq!(with_keys(MAX_KEYS).validate(), Ok(()));
    assert_eq!(
        with_keys(MAX_KEYS + 1).validate(),
        Err(CredentialError::InvalidKeys)
    );
    let with_attributes = |n: usize| Issuer {
        attributes: vec![AttrKey(1); n],
        ..issuer()
    };
    assert_eq!(with_attributes(MAX_ATTRIBUTE_KEYS).validate(), Ok(()));
    assert_eq!(
        with_attributes(MAX_ATTRIBUTE_KEYS + 1).validate(),
        Err(CredentialError::TooManyAttributeKeys)
    );
    let bad_did = Issuer {
        did: b"id.example.org".to_vec(),
        ..issuer()
    };
    assert_eq!(bad_did.validate(), Err(CredentialError::InvalidDid));
    let self_named = Issuer {
        did: did_key(&public(1)),
        ..issuer()
    };
    assert_eq!(self_named.validate(), Ok(()));
    let wrong_key = Issuer {
        did: did_key(&public(2)),
        ..issuer()
    };
    assert_eq!(wrong_key.validate(), Err(CredentialError::DidKeyMismatch));
}
