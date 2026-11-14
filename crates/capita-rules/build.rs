// Capita Money – capita-rules
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

//! Turns every rules file of `rules/` into Rust source (one module per
//! file) and stops the build if a file is not valid.

use std::path::{Path, PathBuf};
use std::{env, fs, process};

use capita_rulefile::{codegen, RuleSet};

fn generate() -> Result<(), String> {
    let manifest = env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?;
    let out_dir = env::var("OUT_DIR").map_err(|e| e.to_string())?;
    let dir = Path::new(&manifest).join("../../rules");
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("{}: no rules file (*.toml)", dir.display()));
    }

    let mut sets = Vec::new();
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
        let shown = file.strip_prefix(&dir).unwrap_or(file).display();
        let text = fs::read_to_string(file).map_err(|e| format!("rules/{shown}: {e}"))?;
        let set = RuleSet::from_toml(&text).map_err(|e| format!("rules/{shown}: {e}"))?;
        if file.file_stem().and_then(|stem| stem.to_str()) != Some(set.set.name.as_str()) {
            return Err(format!(
                "rules/{shown}: set.name is \"{}\" – it must equal the name of the file",
                set.set.name
            ));
        }
        sets.push(set);
    }
    let source = codegen::library(&sets).map_err(|e| e.to_string())?;
    fs::write(Path::new(&out_dir).join("sets.rs"), source).map_err(|e| e.to_string())
}

fn main() {
    if let Err(problem) = generate() {
        eprintln!("\nThe rules are not valid.\n{problem}\n");
        process::exit(1);
    }
}
