// SPDX-License-Identifier: AGPL-3.0-only
//! The phone check against certificate chains made here: a stand-in for
//! Google's and Apple's roots, a phone's key with its description, and
//! every way a check must fail.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::collections::BTreeSet;

use capita_attest::android::{self, BootState, SecurityLevel};
use capita_attest::apple::{self, DEVELOPMENT, PRODUCTION};
use capita_attest::{signed, AttestError};
use ciborium::Value;
use openssl::asn1::{Asn1Integer, Asn1Object, Asn1OctetString, Asn1Time};
use openssl::bn::{BigNum, BigNumContext};
use openssl::ec::{EcGroup, EcKey, PointConversionForm};
use openssl::hash::MessageDigest;
use openssl::nid::Nid;
use openssl::pkey::{PKey, Private};
use openssl::sha::sha256;
use openssl::sign::Signer;
use openssl::x509::{X509Builder, X509Extension, X509NameBuilder, X509};

// ---------------------------------------------------------------- DER

fn tlv(tag: &[u8], content: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    let n = content.len();
    if n < 128 {
        out.push(n as u8);
    } else if n < 256 {
        out.extend_from_slice(&[0x81, n as u8]);
    } else {
        out.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]);
    }
    out.extend_from_slice(content);
    out
}
fn seq(parts: &[Vec<u8>]) -> Vec<u8> {
    tlv(&[0x30], &parts.concat())
}
fn set(parts: &[Vec<u8>]) -> Vec<u8> {
    tlv(&[0x31], &parts.concat())
}
fn int(n: u64) -> Vec<u8> {
    let mut b: Vec<u8> = n
        .to_be_bytes()
        .into_iter()
        .skip_while(|x| *x == 0)
        .collect();
    if b.first().is_none_or(|x| x & 0x80 != 0) {
        b.insert(0, 0);
    }
    tlv(&[0x02], &b)
}
fn enumerated(n: u64) -> Vec<u8> {
    let mut i = int(n);
    i[0] = 0x0a;
    i
}
fn octets(b: &[u8]) -> Vec<u8> {
    tlv(&[0x04], b)
}
fn boolean(b: bool) -> Vec<u8> {
    vec![0x01, 0x01, if b { 0xff } else { 0 }]
}
/// `[n] EXPLICIT`, tag numbers above 30 in the long form.
fn ctx(n: u32, inner: Vec<u8>) -> Vec<u8> {
    if n < 31 {
        return tlv(&[0xa0 | n as u8], &inner);
    }
    let mut digits = vec![(n & 0x7f) as u8];
    let mut rest = n >> 7;
    while rest > 0 {
        digits.push(0x80 | (rest & 0x7f) as u8);
        rest >>= 7;
    }
    digits.reverse();
    let mut tag = vec![0xbf];
    tag.extend_from_slice(&digits);
    tlv(&tag, &inner)
}

// ---------------------------------------------------------------- certificates

fn ec_key(nid: Nid) -> PKey<Private> {
    PKey::from_ec_key(EcKey::generate(&EcGroup::from_curve_name(nid).unwrap()).unwrap()).unwrap()
}

struct Cert<'a> {
    cn: &'a str,
    key: &'a PKey<Private>,
    issuer: &'a str,
    issuer_key: &'a PKey<Private>,
    serial: u32,
    extension: Option<(&'a str, Vec<u8>)>,
    from: i64,
    until: i64,
}

fn name(cn: &str) -> openssl::x509::X509Name {
    let mut n = X509NameBuilder::new().unwrap();
    n.append_entry_by_text("CN", cn).unwrap();
    n.build()
}

fn make(c: &Cert<'_>) -> X509 {
    let mut b = X509Builder::new().unwrap();
    b.set_version(2).unwrap();
    let serial: Asn1Integer = BigNum::from_u32(c.serial)
        .unwrap()
        .to_asn1_integer()
        .unwrap();
    b.set_serial_number(&serial).unwrap();
    b.set_subject_name(&name(c.cn)).unwrap();
    b.set_issuer_name(&name(c.issuer)).unwrap();
    b.set_pubkey(c.key).unwrap();
    b.set_not_before(&Asn1Time::from_unix(c.from).unwrap())
        .unwrap();
    b.set_not_after(&Asn1Time::from_unix(c.until).unwrap())
        .unwrap();
    if let Some((oid, der)) = &c.extension {
        let oid = Asn1Object::from_str(oid).unwrap();
        let value = Asn1OctetString::new_from_bytes(der).unwrap();
        b.append_extension(X509Extension::new_from_der(&oid, false, &value).unwrap())
            .unwrap();
    }
    b.sign(c.issuer_key, MessageDigest::sha256()).unwrap();
    b.build()
}

fn sign_der(key: &PKey<Private>, message: &[u8]) -> Vec<u8> {
    let mut s = Signer::new(MessageDigest::sha256(), key).unwrap();
    s.sign_oneshot_to_vec(message).unwrap()
}

const NOW: i64 = 1_794_700_000; // 2026-11-15
const YEAR: i64 = 365 * 86_400;

// ---------------------------------------------------------------- Android

const PACKAGE: &str = "com.capitamoney.app";

#[derive(Clone)]
struct Desc {
    level: u64,
    challenge: Vec<u8>,
    origin: u64,
    algorithm: u64,
    locked: bool,
    boot: u64,
    boot_key: Vec<u8>,
    package: String,
    signer: [u8; 32],
}

impl Desc {
    fn good() -> Self {
        Self {
            level: 1,
            challenge: b"nonce of the service, 32 bytes!!".to_vec(),
            origin: 0,
            algorithm: 3,
            locked: true,
            boot: 0,
            boot_key: vec![7; 32],
            package: PACKAGE.into(),
            signer: sha256(b"the app's signing certificate"),
        }
    }
    fn der(&self) -> Vec<u8> {
        let app_id = seq(&[
            set(&[seq(&[octets(self.package.as_bytes()), int(1)])]),
            set(&[octets(&self.signer)]),
        ]);
        let software = seq(&[ctx(709, octets(&app_id))]);
        let trust = seq(&[
            octets(&self.boot_key),
            boolean(self.locked),
            enumerated(self.boot),
            octets(&[0; 32]),
        ]);
        let hardware = seq(&[
            ctx(1, set(&[int(2)])),
            ctx(2, int(self.algorithm)),
            ctx(3, int(256)),
            ctx(5, set(&[int(4)])),
            ctx(10, int(1)),
            ctx(702, int(self.origin)),
            ctx(704, trust),
            ctx(706, int(202_610)),
        ]);
        seq(&[
            int(300),
            enumerated(self.level),
            int(300),
            enumerated(self.level),
            octets(&self.challenge),
            octets(b""),
            software,
            hardware,
        ])
    }
}

struct Maker {
    root: PKey<Private>,
    intermediate: PKey<Private>,
}

impl Maker {
    fn new() -> Self {
        Self {
            root: ec_key(Nid::SECP384R1),
            intermediate: ec_key(Nid::X9_62_PRIME256V1),
        }
    }
    /// A chain for a fresh session key described by `desc`.
    fn chain(&self, desc: &Desc) -> (PKey<Private>, Vec<Vec<u8>>) {
        let session = ec_key(Nid::X9_62_PRIME256V1);
        let leaf = make(&Cert {
            cn: "Android Keystore Key",
            key: &session,
            issuer: "phone chip",
            issuer_key: &self.intermediate,
            serial: 1,
            extension: Some(("1.3.6.1.4.1.11129.2.1.17", desc.der())),
            from: 0,
            until: 2_000_000_000,
        });
        let intermediate = make(&Cert {
            cn: "phone chip",
            key: &self.intermediate,
            issuer: "root",
            issuer_key: &self.root,
            serial: 0xfa2b,
            extension: None,
            from: NOW - YEAR,
            until: NOW + YEAR,
        });
        let root = make(&Cert {
            cn: "root",
            key: &self.root,
            issuer: "root",
            issuer_key: &self.root,
            serial: 9,
            extension: None,
            from: NOW - YEAR,
            until: NOW + 10 * YEAR,
        });
        (
            session,
            vec![
                leaf.to_der().unwrap(),
                intermediate.to_der().unwrap(),
                root.to_der().unwrap(),
            ],
        )
    }
    fn policy(&self) -> android::Policy {
        let public = PKey::public_key_from_der(&self.root.public_key_to_der().unwrap()).unwrap();
        android::Policy {
            roots: vec![public],
            revoked: BTreeSet::new(),
            packages: vec![PACKAGE.into()],
            signers: vec![Desc::good().signer],
            boot_keys: vec![vec![9; 32]],
            lowest: SecurityLevel::TrustedEnvironment,
        }
    }
}

#[test]
fn a_real_phone_with_the_app_passes_and_its_key_signs() {
    let maker = Maker::new();
    let desc = Desc::good();
    let (session, chain) = maker.chain(&desc);
    let phone = android::check(&chain, &desc.challenge, &maker.policy()).unwrap();
    assert_eq!(phone.level, SecurityLevel::TrustedEnvironment);
    assert_eq!(phone.boot, BootState::Verified);
    assert_eq!(phone.patch_level, Some(202_610));
    let request = b"digest of chip data and film";
    assert!(signed(&phone.key, request, &sign_der(&session, request)));
    assert!(!signed(
        &phone.key,
        b"another request",
        &sign_der(&session, request)
    ));
    let other = ec_key(Nid::X9_62_PRIME256V1);
    assert!(!signed(&phone.key, request, &sign_der(&other, request)));
}

#[test]
fn strongbox_and_self_signed_systems_with_a_listed_key_pass() {
    let maker = Maker::new();
    let mut desc = Desc::good();
    desc.level = 2;
    desc.boot = 1;
    desc.boot_key = vec![9; 32];
    let (_, chain) = maker.chain(&desc);
    let phone = android::check(&chain, &desc.challenge, &maker.policy()).unwrap();
    assert_eq!(phone.level, SecurityLevel::StrongBox);
    assert_eq!(phone.boot, BootState::SelfSigned);
}

#[test]
fn phones_that_must_not_pass() {
    let maker = Maker::new();
    let policy = maker.policy();
    let refused = |change: &dyn Fn(&mut Desc), why: AttestError| {
        let mut desc = Desc::good();
        change(&mut desc);
        let (_, chain) = maker.chain(&desc);
        assert_eq!(
            android::check(&chain, &Desc::good().challenge, &policy).unwrap_err(),
            why
        );
    };
    refused(
        &|d| d.challenge = b"an old nonce".to_vec(),
        AttestError::Challenge,
    );
    refused(&|d| d.level = 0, AttestError::NotHardware);
    refused(&|d| d.locked = false, AttestError::Unlocked);
    refused(&|d| d.boot = 2, AttestError::Unlocked);
    refused(&|d| d.boot = 1, AttestError::Unlocked); // self-signed, unknown key
    refused(&|d| d.origin = 2, AttestError::WrongKey); // imported
    refused(&|d| d.algorithm = 1, AttestError::WrongKey); // RSA
    refused(
        &|d| d.package = "com.example.fake".into(),
        AttestError::WrongApp,
    );
    refused(
        &|d| d.signer = sha256(b"someone else's key"),
        AttestError::WrongApp,
    );
}

#[test]
fn chains_that_must_not_pass() {
    let maker = Maker::new();
    let desc = Desc::good();
    let (_, chain) = maker.chain(&desc);

    // another maker's root
    let other = Maker::new();
    assert_eq!(
        android::check(&chain, &desc.challenge, &other.policy()).unwrap_err(),
        AttestError::Untrusted
    );
    // a revoked certificate
    let mut policy = maker.policy();
    policy.revoked.insert(android::revocation_key("0000FA2B"));
    assert_eq!(
        android::check(&chain, &desc.challenge, &policy).unwrap_err(),
        AttestError::Revoked("fa2b".into())
    );
    // a key certificate from another chain
    let (_, foreign) = other.chain(&desc);
    let mixed = vec![foreign[0].clone(), chain[1].clone(), chain[2].clone()];
    assert_eq!(
        android::check(&mixed, &desc.challenge, &maker.policy()).unwrap_err(),
        AttestError::Chain
    );
    // the root alone
    assert!(matches!(
        android::check(&chain[2..], &desc.challenge, &maker.policy()),
        Err(AttestError::Malformed(_))
    ));
    // a software keystore demands only the trusted environment or better
    let mut strict = maker.policy();
    strict.lowest = SecurityLevel::StrongBox;
    assert_eq!(
        android::check(&chain, &desc.challenge, &strict).unwrap_err(),
        AttestError::NotHardware
    );
}

// ---------------------------------------------------------------- Apple

const APP_ID: &str = "ABCDE12345.com.capitamoney.app";

struct Apple {
    root: PKey<Private>,
    intermediate: PKey<Private>,
    root_cert: X509,
}

struct Attested {
    key: PKey<Private>,
    key_id: [u8; 32],
    object: Vec<u8>,
}

fn point(key: &PKey<Private>) -> Vec<u8> {
    let ec = key.ec_key().unwrap();
    let mut ctx = BigNumContext::new().unwrap();
    ec.public_key()
        .to_bytes(ec.group(), PointConversionForm::UNCOMPRESSED, &mut ctx)
        .unwrap()
}

fn cbor(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(value, &mut out).unwrap();
    out
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

impl Apple {
    fn new() -> Self {
        let root = ec_key(Nid::SECP384R1);
        let root_cert = make(&Cert {
            cn: "App Attestation Root CA",
            key: &root,
            issuer: "App Attestation Root CA",
            issuer_key: &root,
            serial: 1,
            extension: None,
            from: NOW - YEAR,
            until: NOW + 20 * YEAR,
        });
        Self {
            root,
            intermediate: ec_key(Nid::SECP384R1),
            root_cert,
        }
    }
    fn policy(&self) -> apple::Policy {
        apple::Policy {
            root: self.root_cert.clone(),
            app_ids: vec![APP_ID.into()],
            development: false,
        }
    }
    fn attest(&self, challenge: &[u8], app_id: &str, aaguid: &[u8; 16], until: i64) -> Attested {
        let key = ec_key(Nid::X9_62_PRIME256V1);
        let key_id = sha256(&point(&key));
        let mut data = sha256(app_id.as_bytes()).to_vec();
        data.push(0x40);
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(aaguid);
        data.extend_from_slice(&32u16.to_be_bytes());
        data.extend_from_slice(&key_id);
        data.push(0xa0); // the credential key in COSE form; not read
        let mut signed_data = data.clone();
        signed_data.extend_from_slice(&sha256(challenge));
        let nonce = sha256(&signed_data);
        let credential = make(&Cert {
            cn: "credential",
            key: &key,
            issuer: "App Attestation CA 1",
            issuer_key: &self.intermediate,
            serial: 5,
            extension: Some(("1.2.840.113635.100.8.2", seq(&[ctx(1, octets(&nonce))]))),
            from: NOW - 86_400,
            until,
        });
        let intermediate = make(&Cert {
            cn: "App Attestation CA 1",
            key: &self.intermediate,
            issuer: "App Attestation Root CA",
            issuer_key: &self.root,
            serial: 2,
            extension: None,
            from: NOW - YEAR,
            until: NOW + 5 * YEAR,
        });
        let object = cbor(&Value::Map(vec![
            (text("fmt"), text("apple-appattest")),
            (
                text("attStmt"),
                Value::Map(vec![
                    (
                        text("x5c"),
                        Value::Array(vec![
                            Value::Bytes(credential.to_der().unwrap()),
                            Value::Bytes(intermediate.to_der().unwrap()),
                        ]),
                    ),
                    (text("receipt"), Value::Bytes(vec![1, 2, 3])),
                ]),
            ),
            (text("authData"), Value::Bytes(data)),
        ]));
        Attested {
            key,
            key_id,
            object,
        }
    }
}

fn assert_over(key: &PKey<Private>, client_data: &[u8], count: u32, app_id: &str) -> Vec<u8> {
    let mut data = sha256(app_id.as_bytes()).to_vec();
    data.push(0x01);
    data.extend_from_slice(&count.to_be_bytes());
    let mut message = data.clone();
    message.extend_from_slice(&sha256(client_data));
    let nonce = sha256(&message);
    cbor(&Value::Map(vec![
        (text("signature"), Value::Bytes(sign_der(key, &nonce))),
        (text("authenticatorData"), Value::Bytes(data)),
    ]))
}

#[test]
fn an_iphone_app_key_certified_for_the_nonce_signs_the_request() {
    let apple = Apple::new();
    let challenge = b"nonce of the service, 32 bytes!!";
    let request = b"digest of chip data and film";
    let a = apple.attest(challenge, APP_ID, PRODUCTION, NOW + 86_400);
    let assertion = assert_over(&a.key, request, 1, APP_ID);
    let key = apple::check(
        &a.object,
        &a.key_id,
        &assertion,
        challenge,
        request,
        NOW,
        &apple.policy(),
    )
    .unwrap();
    assert_eq!(key.app_id, APP_ID);
    assert!(!key.development);
}

#[test]
fn iphone_proofs_that_must_not_pass() {
    let apple = Apple::new();
    let policy = apple.policy();
    let challenge = b"nonce of the service, 32 bytes!!";
    let request = b"digest of chip data and film";
    let good = apple.attest(challenge, APP_ID, PRODUCTION, NOW + 86_400);
    let attest = |a: &Attested, c: &[u8], now: i64, p: &apple::Policy| {
        apple::attestation(&a.object, &a.key_id, c, now, p)
    };

    assert_eq!(
        attest(&good, b"an old nonce", NOW, &policy).unwrap_err(),
        AttestError::Challenge
    );
    assert_eq!(
        attest(&good, challenge, NOW + 2 * 86_400, &policy).unwrap_err(),
        AttestError::Expired
    );
    assert_eq!(
        apple::attestation(&good.object, &[0; 32], challenge, NOW, &policy).unwrap_err(),
        AttestError::WrongKey
    );
    let other = Apple::new();
    assert_eq!(
        attest(&good, challenge, NOW, &other.policy()).unwrap_err(),
        AttestError::Chain
    );
    let fake = apple.attest(
        challenge,
        "XXXXX00000.com.example.fake",
        PRODUCTION,
        NOW + 86_400,
    );
    assert_eq!(
        attest(&fake, challenge, NOW, &policy).unwrap_err(),
        AttestError::WrongApp
    );

    // test builds: development keys only where the service allows them
    let dev = apple.attest(challenge, APP_ID, DEVELOPMENT, NOW + 86_400);
    assert_eq!(
        attest(&dev, challenge, NOW, &policy).unwrap_err(),
        AttestError::WrongApp
    );
    let mut test_service = apple.policy();
    test_service.development = true;
    assert!(
        attest(&dev, challenge, NOW, &test_service)
            .unwrap()
            .development
    );

    // assertions: another request, another key, a counter that did not rise
    let key = attest(&good, challenge, NOW, &policy).unwrap();
    let assertion = assert_over(&good.key, request, 1, APP_ID);
    assert_eq!(
        apple::assertion(&assertion, b"another request", &key, 0).unwrap_err(),
        AttestError::BadSignature
    );
    let stranger = assert_over(&ec_key(Nid::X9_62_PRIME256V1), request, 1, APP_ID);
    assert_eq!(
        apple::assertion(&stranger, request, &key, 0).unwrap_err(),
        AttestError::BadSignature
    );
    let replay = assert_over(&good.key, request, 0, APP_ID);
    assert_eq!(
        apple::assertion(&replay, request, &key, 0).unwrap_err(),
        AttestError::Counter
    );
    assert_eq!(
        apple::assertion(&assertion, request, &key, 1).unwrap_err(),
        AttestError::Counter
    );
    assert_eq!(apple::assertion(&assertion, request, &key, 0), Ok(1));
}
