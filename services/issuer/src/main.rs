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
//! ```

use std::path::Path;
use std::process::ExitCode;

use capita_credential::did::did_key;
use capita_credential::types::PublicKey;
use capita_issuer::checks::Challenger;
use capita_issuer::config::Config;
use capita_issuer::http::{serve, App};
use capita_issuer::{hex, unhex_n};

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: capita-issuer <configuration file> [key | revoke 0x<serial>]")?;
    let config = Config::load(Path::new(&path))?;
    let service = config.service()?;
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
            serve(
                &App::new(service, config.checks.test, challenger),
                &config.listen,
            )
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
        _ => Err("usage: capita-issuer <configuration file> [key | revoke 0x<serial>]".into()),
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
