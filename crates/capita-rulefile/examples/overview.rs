// Capita Money – capita-rulefile
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

//! Prints a rules file as a list, the open rules first:
//!
//! ```text
//! cargo run -p capita-rulefile --example overview -- rules/alpha.toml
//! ```

use std::{env, fs, process};

use capita_rulefile::{RuleSet, Status};

fn main() {
    let Some(path) = env::args().nth(1) else {
        eprintln!("usage: overview <rules file>");
        process::exit(2);
    };
    let set = match fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| RuleSet::from_toml(&text).map_err(|e| e.0))
    {
        Ok(set) => set,
        Err(problem) => {
            eprintln!("{path}: {problem}");
            process::exit(1);
        }
    };
    let lines = set.lines();
    println!("{} – {} ({})", set.set.name, set.set.title, set.set.diary);
    for (status, heading) in [
        (Status::Example, "Examples – numbers without a decision"),
        (
            Status::Default,
            "Defaults – working assumptions until decided",
        ),
        (Status::Decided, "Decided"),
    ] {
        let of_status: Vec<_> = lines.iter().filter(|l| l.status == status).collect();
        println!("\n{heading} ({})", of_status.len());
        for line in of_status {
            println!("  {} = {}   [{}]", line.path, line.value, line.source);
        }
    }
}
