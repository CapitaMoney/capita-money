// Capita Money – capita-euro-door
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

//! `capita-euro-door` – the euro door of Capita Money.
//!
//! ```text
//! capita-euro-door <configuration file>      run the door
//! ```

use std::path::Path;
use std::process::ExitCode;

use capita_euro_door::config::Config;
use capita_euro_door::http::{serve, App};

fn run() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: capita-euro-door <configuration file>")?;
    let config = Config::load(Path::new(&path))?;
    let door = config.door()?;
    if door.settings().currency != "EUR" {
        eprintln!(
            "capita-euro-door: currency {} – DEMO MONEY, tickets are for test chains only.",
            door.settings().currency
        );
    }
    serve(&App::new(door), &config.listen)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("capita-euro-door: {problem}");
            ExitCode::FAILURE
        }
    }
}
