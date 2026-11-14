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

//! A pocket identity service for test chains: signs a claim, or takes one
//! back, and prints what goes into the transaction.
//!
//! ```text
//! cargo run -p capita-credential --example issue -- \
//!     --audience 0x<genesis hash of the chain> --subject 0x<account, 32 bytes> --level 1
//! ```
//!
//! Without `--seed` it signs with the key of the identity service every
//! test chain trusts from its first block – a secret everybody knows. It
//! checks nobody: a real service verifies a person first, and guards its
//! key.
//!
//! Options: `--issuer N` (number on the list, default 0) · `--person 0x…`
//! (pseudonym of the person, 32 bytes; default: the account, so every
//! account is its own person) · `--replaces 0x…` (the person key the person
//! had before, 32 bytes) · `--serial 0x…` (16 bytes; default: derived
//! from account and time) · `--country FR` (states a residence, proven now)
//! · `--days N` (validity, default 365; 0 = without end) · `--seed 0x…`
//! (Ed25519 secret key, 32 bytes) · `--revoke` (sign the revocation of
//! `--serial` instead of a claim).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    missing_docs
)]

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use capita_core::attributes::{Residence, VerificationLevel};
use capita_core::types::{AccountId, CountryCode, Timestamp};
use capita_credential::did::did_key;
use capita_credential::encoding::{claim_bytes, revocation_bytes};
use capita_credential::types::{
    Audience, Claim, IssuerId, PublicKey, Revocation, Serial, SubjectRef,
};
use ed25519_dalek::{Signer, SigningKey};

/// Secret key of the identity service of the test chains
/// (`runtime/src/genesis_config_presets.rs`).
const DEV_ISSUER_SEED: [u8; 32] = *b"capita-money development issuer!";

fn hex(bytes: &[u8]) -> String {
    let digits: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("0x{digits}")
}

fn bytes<const N: usize>(name: &str, text: &str) -> [u8; N] {
    let digits = text.strip_prefix("0x").unwrap_or(text);
    let parsed: Option<Vec<u8>> = (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok())
        .collect();
    parsed
        .and_then(|bytes| <[u8; N]>::try_from(bytes).ok())
        .unwrap_or_else(|| panic!("--{name}: expected {N} bytes in hexadecimal"))
}

fn main() {
    let mut options = BTreeMap::new();
    let mut revoke = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.strip_prefix("--") {
            Some("revoke") => revoke = true,
            Some(name) => {
                let value = args
                    .next()
                    .unwrap_or_else(|| panic!("--{name} needs a value"));
                options.insert(name.to_owned(), value);
            }
            None => panic!("unexpected argument {arg}; see the head of examples/issue.rs"),
        }
    }
    let number = |name: &str, default: u64| -> u64 {
        options.get(name).map_or(default, |text| {
            text.parse()
                .unwrap_or_else(|_| panic!("--{name}: not a number"))
        })
    };
    let audience = Audience(bytes(
        "audience",
        options
            .get("audience")
            .expect("--audience 0x<genesis hash> is needed"),
    ));
    let issuer = IssuerId(u32::try_from(number("issuer", 0)).expect("--issuer: too large"));
    let seed = options
        .get("seed")
        .map_or(DEV_ISSUER_SEED, |text| bytes("seed", text));
    let key = SigningKey::from_bytes(&seed);
    let public = PublicKey::Ed25519(key.verifying_key().to_bytes());
    println!(
        "issuer {} signs with {}",
        issuer.0,
        String::from_utf8_lossy(&did_key(&public))
    );

    if revoke {
        let serial = Serial(bytes(
            "serial",
            options.get("serial").expect("--revoke needs --serial"),
        ));
        let revocation = Revocation { issuer, serial };
        let signature = key.sign(&revocation_bytes(&audience, &revocation));
        println!("\ncredential.revoke");
        println!("  revocation.issuer   {}", issuer.0);
        println!("  revocation.serial   {}", hex(&serial.0));
        println!(
            "  signature           Ed25519 {}",
            hex(&signature.to_bytes())
        );
        return;
    }

    let subject: [u8; 32] = bytes(
        "subject",
        options
            .get("subject")
            .expect("--subject 0x<account> is needed"),
    );
    // A minute back: the clock of this computer may run ahead of the chain.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after 1970")
        .as_secs()
        - 60;
    let serial = options.get("serial").map_or_else(
        || {
            let mut hash = Blake2b::<U32>::new();
            hash.update(subject);
            hash.update(now.to_be_bytes());
            let digest: [u8; 32] = hash.finalize().into();
            <[u8; 16]>::try_from(&digest[..16]).unwrap()
        },
        |text| bytes("serial", text),
    );
    let days = number("days", 365);
    let claim = Claim {
        issuer,
        serial: Serial(serial),
        subject: AccountId(subject),
        subject_ref: SubjectRef(
            options
                .get("person")
                .map_or(subject, |text| bytes("person", text)),
        ),
        replaces: options
            .get("replaces")
            .map(|text| SubjectRef(bytes("replaces", text))),
        level: VerificationLevel(u8::try_from(number("level", 1)).expect("--level: too large")),
        residence: options.get("country").map(|country| Residence {
            country: CountryCode(
                <[u8; 2]>::try_from(country.as_bytes()).expect("--country: two letters"),
            ),
            proven_at: Timestamp::from_secs(now),
        }),
        attributes: vec![],
        issued_at: Timestamp::from_secs(now),
        expires_at: (days > 0).then(|| Timestamp::from_secs(now + days * 86_400)),
    };
    let signature = key.sign(&claim_bytes(&audience, &claim));

    println!("\ncredential.present      (sent by the subject)");
    println!("  claim.issuer        {}", claim.issuer.0);
    println!("  claim.serial        {}", hex(&claim.serial.0));
    println!("  claim.subject       {}", hex(&claim.subject.0));
    println!("  claim.subjectRef    {}", hex(&claim.subject_ref.0));
    match &claim.replaces {
        None => println!("  claim.replaces      none"),
        Some(previous) => println!("  claim.replaces      {}", hex(&previous.0)),
    }
    println!("  claim.level         {}", claim.level.0);
    match &claim.residence {
        None => println!("  claim.residence     none"),
        Some(residence) => println!(
            "  claim.residence     country {} ({}), provenAt {}",
            hex(&residence.country.0),
            String::from_utf8_lossy(&residence.country.0),
            residence.proven_at.secs()
        ),
    }
    println!("  claim.attributes    none");
    println!("  claim.issuedAt      {}", claim.issued_at.secs());
    match claim.expires_at {
        None => println!("  claim.expiresAt     none"),
        Some(end) => println!("  claim.expiresAt     {}", end.secs()),
    }
    println!(
        "  signature           Ed25519 {}",
        hex(&signature.to_bytes())
    );
    println!("  name                optional: a name for the identity, 3 to 42 of a-z A-Z 0-9 - _");
}
