// SPDX-License-Identifier: AGPL-3.0-only
//! `capita-marks` – personal copies and fingerprints from the command line,
//! for the market service and for checking a leaked file by hand.
//!
//! ```text
//! capita-marks fingerprint book.epub
//! capita-marks compare original.epub found.epub
//! capita-marks payload --secret service.key --sale 7KQF-3MZD
//! capita-marks mark --secret service.key --sale 7KQF-3MZD book.epub copy.epub
//! capita-marks read found.epub
//! ```
//! Output is JSON on standard output.

use capita_goods::fingerprint::Fingerprint;
use capita_marks::payload::Payload;
use capita_marks::{fingerprint, personalise, read, Kind};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::path::PathBuf;
use std::process::ExitCode;

/// Personal copies of sold digital goods and the fingerprints of a work.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Exact hash and text print of a file.
    Fingerprint {
        /// The file.
        file: PathBuf,
    },
    /// How alike two files are.
    Compare {
        /// The first file.
        a: PathBuf,
        /// The second file.
        b: PathBuf,
    },
    /// The payload a sale's copies carry.
    Payload {
        #[command(flatten)]
        sale: Sale,
    },
    /// Writes the personal copy of a file for a sale.
    Mark {
        #[command(flatten)]
        sale: Sale,
        /// The original.
        input: PathBuf,
        /// Where the personal copy goes.
        output: PathBuf,
    },
    /// Reads the marks of a file.
    Read {
        /// The file.
        file: PathBuf,
    },
}

#[derive(clap::Args)]
struct Sale {
    /// File with the secret of the market service (at least 32 bytes).
    #[arg(long)]
    secret: PathBuf,
    /// Id of the sale.
    #[arg(long)]
    sale: String,
    /// Generation of the licence: 0 for the buyer, +1 for each gift.
    #[arg(long, default_value_t = 0)]
    generation: u8,
}

impl Sale {
    fn payload(&self) -> Result<Payload, String> {
        let secret =
            std::fs::read(&self.secret).map_err(|e| format!("{}: {e}", self.secret.display()))?;
        Payload::for_sale(&secret, &self.sale, self.generation)
    }
}

fn load(path: &PathBuf) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn print_json(kind: Kind, f: &Fingerprint) -> serde_json::Value {
    json!({
        "kind": kind,
        "sha256": f.hash.to_string(),
        "text_print": f.text.map(|t| t.0.to_vec()),
    })
}

fn run(cli: Cli) -> Result<serde_json::Value, String> {
    match cli.command {
        Command::Fingerprint { file } => {
            let (kind, f) = fingerprint(&load(&file)?).map_err(|e| e.to_string())?;
            Ok(print_json(kind, &f))
        }
        Command::Compare { a, b } => {
            let (_, fa) = fingerprint(&load(&a)?).map_err(|e| e.to_string())?;
            let (_, fb) = fingerprint(&load(&b)?).map_err(|e| e.to_string())?;
            let similarity = match (fa.text, fb.text) {
                (Some(x), Some(y)) => Some(x.similarity(&y).ppm()),
                _ => None,
            };
            Ok(json!({
                "same_file": fa.hash == fb.hash,
                "text_similarity_ppm": similarity,
            }))
        }
        Command::Payload { sale } => Ok(json!({ "payload": sale.payload()?.to_string() })),
        Command::Mark {
            sale,
            input,
            output,
        } => {
            let payload = sale.payload()?;
            let copy = personalise(&load(&input)?, &payload).map_err(|e| e.to_string())?;
            std::fs::write(&output, copy).map_err(|e| format!("{}: {e}", output.display()))?;
            Ok(json!({ "payload": payload.to_string(), "output": output }))
        }
        Command::Read { file } => {
            let r = read(&load(&file)?).map_err(|e| e.to_string())?;
            let found: Vec<_> = r
                .found
                .iter()
                .map(|f| json!({ "layer": f.layer, "payload": f.payload.to_string() }))
                .collect();
            Ok(json!({
                "payload": r.payload.map(|p| p.to_string()),
                "mixed": r.mixed,
                "found": found,
            }))
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(v) => {
            println!("{v:#}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("capita-marks: {e}");
            ExitCode::FAILURE
        }
    }
}
