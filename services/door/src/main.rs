// Capita Money – capita-door-service
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

//! `capita-door` – the euro door of Capita Money.
//!
//! ```text
//! capita-door <configuration file>        run the door
//! capita-door <configuration file> key    print the public key (for the list of doors of the chain,
//!                                         and for identity services that accept its tickets)
//! ```

use std::path::Path;
use std::process::ExitCode;

use capita_door_service::chain::Node;
use capita_door_service::config::Config;
use capita_door_service::hex;
use capita_door_service::http::{serve, App};

const USAGE: &str = "usage: capita-door <configuration file> [key]";

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or(USAGE)?;
    let config = Config::load(Path::new(&path))?;
    let door = config.door()?;
    match args.next().as_deref() {
        None => {
            let currency = &door.settings().currency;
            if currency != "EUR" {
                eprintln!("capita-door: currency {currency}, counted like euro – test money only.");
            }
            let node = config.chain.as_ref().map(|chain| Node::new(&chain.rpc_url));
            let app = App::new(door, config.merchant()?, node, config.demo_page);
            serve(&app, &config.listen)
        }
        Some("key") => {
            println!("public key (Ed25519)  0x{}", hex(&door.public_key()));
            println!("door number           {}", door.settings().door.0);
            Ok(())
        }
        Some(_) => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("capita-door: {problem}");
            ExitCode::FAILURE
        }
    }
}
