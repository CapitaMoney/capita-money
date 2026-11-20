// Capita Money – capita-issuer
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

//! `capita-issuer` – the identity service of Capita Money.
//!
//! ```text
//! capita-issuer <configuration file>                 run the service
//! capita-issuer <configuration file> key             print the public key and the did:key of the service
//! capita-issuer <configuration file> revoke 0x<serial>
//!                                                    sign the revocation of a claim
//!
//! Face dedup (rule 33, ADR 0030), for the Verein board:
//! capita-issuer <configuration file> cases           the verifications held, oldest first
//! capita-issuer <configuration file> case-of <family name> <given names> <YYYY-MM-DD>
//!                                                    the held case of the person a document names
//! capita-issuer <configuration file> decide <case> same|different
//!                                                    the board's verdict after the video call
//! capita-issuer <configuration file> forget 0x<account>
//!                                                    forget the face print of a closed account
//! ```
//!
//! The board's commands may run while the service runs: the face store
//! takes a lock for every reading and every change.

use std::path::Path;
use std::process::ExitCode;

use capita_credential::did::did_key;
use capita_credential::types::PublicKey;
use capita_issuer::checks::Challenger;
use capita_issuer::config::Config;
use capita_issuer::dedup::Verdict;
use capita_issuer::http::{serve, App};
use capita_issuer::issue::Person;
use capita_issuer::store::Store;
use capita_issuer::{hex, unhex_n};
use capita_person_key::canonical::{Date, Name};

const USAGE: &str = "usage: capita-issuer <configuration file> [key | revoke 0x<serial> | cases | case-of <family name> <given names> <YYYY-MM-DD> | decide <case> same|different | forget 0x<account>]";

/// A day as `YYYY-MM-DD`.
fn day(secs: u64) -> String {
    Date::from_days_since_epoch(secs / 86_400).map_or_else(
        || "?".into(),
        |d| format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day()),
    )
}

const NO_DEDUP: &str =
    "this service has no face store: [checks.document.dedup] of the configuration";

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or(USAGE)?;
    let config = Config::load(Path::new(&path))?;
    let mut service = config.service()?;
    match (args.next().as_deref(), args.next()) {
        (None, _) => {
            let challenger = config
                .checks
                .challenger
                .clone()
                .map(|settings| {
                    let secret = std::fs::read_to_string(&settings.client_secret_file)
                        .map_err(|e| format!("{}: {e}", settings.client_secret_file.display()))?;
                    Ok::<_, String>(Challenger::new(settings, secret.trim().to_owned()))
                })
                .transpose()?;
            if config.checks.test {
                eprintln!(
                    "capita-issuer: the TEST CHECK is on – this service verifies nobody. Test chains only."
                );
            }
            match (service.settings().rules.face_dedup, service.dedup().is_some()) {
                (true, _) => eprintln!(
                    "capita-issuer: face dedup ON – the sealed face print of every chip photo is kept (rule 33, ADR 0030)"
                ),
                (false, true) => eprintln!(
                    "capita-issuer: face dedup off by the rules (identity.face_dedup): its store is not used"
                ),
                (false, false) => {}
            }
            let mut app = App::new(service, config.checks.test, challenger);
            if let Some(document) = &config.checks.document {
                let check = capita_issuer::document::DocumentCheck::load(document)?;
                eprintln!(
                    "capita-issuer: document check on – {} country signing certificates",
                    check.countries()
                );
                app = app.with_document(check);
            }
            serve(&app, &config.listen)
        }
        (Some("key"), _) => {
            let key = service.public_key();
            let did = did_key(&PublicKey::Ed25519(key));
            println!("public key (Ed25519)  0x{}", hex(&key));
            println!("did                   {}", String::from_utf8_lossy(&did));
            Ok(())
        }
        (Some("revoke"), Some(serial)) => {
            let serial = unhex_n::<16>(&serial).ok_or("serial: 16 bytes in hexadecimal")?;
            let (revocation, signature) = service.revoke(serial);
            println!("credential.revoke     (sent by anybody)");
            println!("  revocation.issuer   {}", revocation.issuer.0);
            println!("  revocation.serial   0x{}", hex(&revocation.serial.0));
            println!("  signature           Ed25519 0x{}", hex(&signature));
            Ok(())
        }
        (Some("cases"), _) => {
            let dedup = service.dedup().ok_or(NO_DEDUP)?;
            let cases = dedup.store().cases()?;
            if cases.is_empty() {
                println!("no verification is held");
            }
            for case in cases {
                let strongest = case
                    .matches
                    .iter()
                    .map(|m| m.likeness)
                    .fold(f32::NEG_INFINITY, f32::max);
                println!(
                    "case {}  held since {}  {} match(es), strongest likeness {strongest:.3}",
                    case.id,
                    day(case.opened),
                    case.matches.len(),
                );
            }
            Ok(())
        }
        (Some("case-of"), Some(family)) => {
            let (Some(given), Some(born), None) = (args.next(), args.next(), args.next()) else {
                return Err(USAGE.into());
            };
            let born = Date::parse(&born).map_err(|e| format!("date of birth: {e}"))?;
            let person = Person::NameBirthdate {
                name: Name { family, given },
                born,
                place_of_birth: None,
            };
            let key = hex(&service.person_key_of(&person).map_err(|e| e.to_string())?);
            let dedup = service.dedup().ok_or(NO_DEDUP)?;
            match dedup.store().cases()?.into_iter().find(|c| c.person == key) {
                Some(case) => println!(
                    "case {}  held since {}  {} match(es)",
                    case.id,
                    day(case.opened),
                    case.matches.len()
                ),
                None => println!("no verification of this person is held"),
            }
            Ok(())
        }
        (Some("decide"), Some(case)) => {
            let verdict = args
                .next()
                .as_deref()
                .and_then(Verdict::from_word)
                .ok_or(USAGE)?;
            let dedup = service.dedup_mut().ok_or(NO_DEDUP)?;
            match dedup
                .store_mut()
                .decide(&case, verdict, capita_issuer::http::now())?
            {
                Some(_) => println!(
                    "case {case}: {} – {}",
                    verdict.as_str(),
                    match verdict {
                        Verdict::Same => "a new verification of this person is refused; the account that exists stays (rule 21)",
                        Verdict::Different => "the person may verify again now; this pair is not held again",
                    }
                ),
                None => println!("no open case {case}"),
            }
            Ok(())
        }
        (Some("forget"), Some(account)) => {
            let account = unhex_n::<32>(&account).ok_or("account: 32 bytes in hexadecimal")?;
            let person = service
                .store()
                .by_account(&hex(&account))
                .ok_or("no claim was issued for this account")?
                .person;
            let dedup = service.dedup_mut().ok_or(NO_DEDUP)?;
            if dedup.store_mut().forget(&person)? {
                println!("face print forgotten");
            } else {
                println!("no face print was kept for this account");
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("capita-issuer: {problem}");
            ExitCode::FAILURE
        }
    }
}
