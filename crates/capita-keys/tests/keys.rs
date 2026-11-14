// Capita Money – capita-keys
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The bytes and checks of `capita-keys` against the vectors the app made
//! (`app/tests/make-key-vectors.mjs`), and passkey assertions that must be
//! refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]

use capita_keys::check::{verify_proof, Verify};
use capita_keys::encoding::{account_of, act_challenge, add_challenge, base64url, blake2_256, sha256};
use capita_keys::types::{Audience, DeviceKey, KeyError, PasskeyProof, Proof};
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::Value;

struct Dalek;

impl Verify for Dalek {
    fn ed25519(key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
        let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(key) else {
            return false;
        };
        key.verify_strict(message, &ed25519_dalek::Signature::from_bytes(signature))
            .is_ok()
    }
}

const RP: &str = "app.capitamoney.com";
const ORIGIN: &str = "https://app.capitamoney.com";

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors.json")).unwrap()
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn arr<const N: usize>(s: &str) -> [u8; N] {
    hex(s).try_into().unwrap()
}

fn text<'a>(v: &'a Value, path: &[&str]) -> &'a str {
    let mut x = v;
    for p in path {
        x = &x[*p];
    }
    x.as_str().unwrap()
}

fn relying_parties() -> [[u8; 32]; 1] {
    [sha256(RP.as_bytes())]
}

fn origin_ok(origin: &[u8]) -> bool {
    origin == ORIGIN.as_bytes()
}

fn check(key: &DeviceKey, challenge: &[u8; 32], proof: &Proof) -> Result<(), KeyError> {
    verify_proof::<Dalek>(key, challenge, proof, &relying_parties(), origin_ok)
}

fn from_app(v: &Value, which: &str) -> PasskeyProof {
    PasskeyProof {
        authenticator_data: hex(text(v, &["passkey", which, "authenticator_data"])),
        client_data_json: text(v, &["passkey", which, "client_data_json"])
            .as_bytes()
            .to_vec(),
        signature: arr(text(v, &["passkey", which, "signature"])),
    }
}

#[test]
fn ed25519_keys_sign_what_the_app_signs() {
    let v = vectors();
    let audience = Audience(arr(text(&v, &["audience"])));
    let remark = hex(text(&v, &["remark_call"]));
    let key = DeviceKey::Ed25519(arr(text(&v, &["ed25519", "public"])));
    let account = account_of(&key);
    assert_eq!(account.to_vec(), hex(text(&v, &["ed25519", "account"])));

    let open = act_challenge(&audience, &account, 0, &blake2_256(&remark));
    assert_eq!(open.to_vec(), hex(text(&v, &["ed25519", "open_challenge"])));
    let proof = Proof::Ed25519(arr(text(&v, &["ed25519", "open", "signature"])));
    assert_eq!(check(&key, &open, &proof), Ok(()));

    let act = act_challenge(&audience, &account, 1, &blake2_256(&remark));
    assert_eq!(act.to_vec(), hex(text(&v, &["ed25519", "act_challenge"])));
    assert_eq!(check(&key, &act, &proof), Err(KeyError::BadSignature));
    let proof = Proof::Ed25519(arr(text(&v, &["ed25519", "act", "signature"])));
    assert_eq!(check(&key, &act, &proof), Ok(()));
}

#[test]
fn a_new_key_proves_it_is_held() {
    let v = vectors();
    let audience = Audience(arr(text(&v, &["audience"])));
    let account: [u8; 32] = arr(text(&v, &["ed25519", "account"]));
    let added = DeviceKey::Ed25519(arr(text(&v, &["ed25519", "add_public"])));
    let challenge = add_challenge(&audience, &account, &added);
    assert_eq!(challenge.to_vec(), hex(text(&v, &["ed25519", "add_challenge"])));
    let possession = Proof::Ed25519(arr(text(&v, &["ed25519", "add_possession", "signature"])));
    assert_eq!(check(&added, &challenge, &possession), Ok(()));
    let call = hex(text(&v, &["ed25519", "add_key_call"]));
    assert_eq!(
        act_challenge(&audience, &account, 2, &blake2_256(&call)).to_vec(),
        hex(text(&v, &["ed25519", "add_act_challenge"]))
    );
}

#[test]
fn passkeys_sign_what_the_app_signs() {
    let v = vectors();
    let audience = Audience(arr(text(&v, &["audience"])));
    let remark = hex(text(&v, &["remark_call"]));
    let key = DeviceKey::Passkey(arr(text(&v, &["passkey", "public"])));
    let account = account_of(&key);
    assert_eq!(account.to_vec(), hex(text(&v, &["passkey", "account"])));

    let open = act_challenge(&audience, &account, 0, &blake2_256(&remark));
    assert_eq!(open.to_vec(), hex(text(&v, &["passkey", "open_challenge"])));
    let proof = from_app(&v, "open");
    assert_eq!(check(&key, &open, &Proof::Passkey(proof.clone())), Ok(()));

    let mut other = proof.clone();
    other.signature = arr(text(&v, &["passkey", "open_other_s"]));
    assert_eq!(check(&key, &open, &Proof::Passkey(other)), Ok(()));

    let act = act_challenge(&audience, &account, 1, &blake2_256(&remark));
    assert_eq!(
        check(&key, &act, &Proof::Passkey(proof)),
        Err(KeyError::WrongChallenge)
    );
    assert_eq!(check(&key, &act, &Proof::Passkey(from_app(&v, "act"))), Ok(()));
}

struct Phone {
    key: SigningKey,
}

impl Phone {
    fn new() -> Self {
        Self {
            key: SigningKey::from_slice(&[7u8; 32]).unwrap(),
        }
    }

    fn public(&self) -> DeviceKey {
        let point = self.key.verifying_key().to_encoded_point(true);
        DeviceKey::Passkey(point.as_bytes().try_into().unwrap())
    }

    fn assert(&self, challenge: &[u8; 32], rp: &str, flags: u8, client: &str) -> Proof {
        let mut data = sha256(rp.as_bytes()).to_vec();
        data.push(flags);
        data.extend_from_slice(&[0, 0, 0, 0]);
        let _ = challenge;
        let mut message = data.clone();
        message.extend_from_slice(&sha256(client.as_bytes()));
        let signature: Signature = self.key.sign(&message);
        let mut raw = [0u8; 64];
        raw.copy_from_slice(&signature.to_bytes());
        Proof::Passkey(PasskeyProof {
            authenticator_data: data,
            client_data_json: client.as_bytes().to_vec(),
            signature: raw,
        })
    }
}

fn client(kind: &str, challenge: &[u8; 32], origin: &str, cross: bool) -> String {
    let c = String::from_utf8(base64url(challenge)).unwrap();
    format!(r#"{{"type":"{kind}","challenge":"{c}","origin":"{origin}","crossOrigin":{cross}}}"#)
}

#[test]
fn passkey_assertions_that_do_not_count() {
    let phone = Phone::new();
    let key = phone.public();
    let challenge = [9u8; 32];
    let good = client("webauthn.get", &challenge, ORIGIN, false);

    let proof = phone.assert(&challenge, RP, 0x05, &good);
    assert_eq!(check(&key, &challenge, &proof), Ok(()));

    let other_site = phone.assert(&challenge, "evil.example", 0x05, &good);
    assert_eq!(check(&key, &challenge, &other_site), Err(KeyError::WrongRelyingParty));

    let not_verified = phone.assert(&challenge, RP, 0x01, &good);
    assert_eq!(check(&key, &challenge, &not_verified), Err(KeyError::NotVerified));

    let create = client("webauthn.create", &challenge, ORIGIN, false);
    let made = phone.assert(&challenge, RP, 0x05, &create);
    assert_eq!(check(&key, &challenge, &made), Err(KeyError::WrongType));

    let phishing = client("webauthn.get", &challenge, "https://evil.example", false);
    let phished = phone.assert(&challenge, RP, 0x05, &phishing);
    assert_eq!(check(&key, &challenge, &phished), Err(KeyError::WrongOrigin));

    let framed = client("webauthn.get", &challenge, ORIGIN, true);
    let in_frame = phone.assert(&challenge, RP, 0x05, &framed);
    assert_eq!(check(&key, &challenge, &in_frame), Err(KeyError::CrossOrigin));

    let escaped = good.replace("\"origin\"", "\"o\\u0072igin\"");
    let tricky = phone.assert(&challenge, RP, 0x05, &escaped);
    assert_eq!(check(&key, &challenge, &tricky), Err(KeyError::Malformed));

    let Proof::Passkey(mut forged) = proof.clone() else {
        panic!("a passkey proof");
    };
    forged.signature[5] ^= 1;
    assert_eq!(
        check(&key, &challenge, &Proof::Passkey(forged)),
        Err(KeyError::BadSignature)
    );

    let Proof::Passkey(mut short) = proof.clone() else {
        panic!("a passkey proof");
    };
    short.authenticator_data.truncate(36);
    assert_eq!(check(&key, &challenge, &Proof::Passkey(short)), Err(KeyError::Malformed));

    let ed = DeviceKey::Ed25519([1u8; 32]);
    assert_eq!(check(&ed, &challenge, &proof), Err(KeyError::KindMismatch));
    let someone_else = DeviceKey::Passkey({
        let other = SigningKey::from_slice(&[8u8; 32]).unwrap();
        other
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .try_into()
            .unwrap()
    });
    assert_eq!(check(&someone_else, &challenge, &proof), Err(KeyError::BadSignature));
}

#[test]
fn the_account_of_a_key_is_fixed() {
    // A change here changes every account ever opened: never.
    let key = DeviceKey::Ed25519([0u8; 32]);
    let mut message = b"capita/account/v1".to_vec();
    message.push(0);
    message.extend_from_slice(&[0u8; 32]);
    assert_eq!(account_of(&key), blake2_256(&message));
    assert_eq!(
        DeviceKey::Passkey([2u8; 33]).to_bytes().first().copied(),
        Some(1)
    );
}
