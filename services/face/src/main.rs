// Capita Money – capita-face
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

//! `capita-face` – try the face check on your own pictures.
//!
//! ```text
//! capita-face <models> faces <picture>
//! capita-face <models> compare <document photo> <selfie>
//! capita-face <models> challenge
//! capita-face <models> check <moves, e.g. left,up> <document photo> <frame> <frame> ...
//! ```
//! Prints results only; keeps nothing.

use capita_face::live::Pose;
use capita_face::{read_picture, FaceCheck, Settings};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  capita-face <models folder> faces <picture>
  capita-face <models folder> compare <document photo> <selfie>
  capita-face <models folder> challenge
  capita-face <models folder> check <moves, e.g. left,up> <document photo> <frame>...
models: ./fetch-models.sh <models folder>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let [models, cmd, rest @ ..] = args else {
        return Err(USAGE.into());
    };
    let fc = FaceCheck::load(Path::new(models), Settings::default()).map_err(|e| e.to_string())?;
    let pic = |p: &String| read_picture(&PathBuf::from(p)).map_err(|e| e.to_string());
    match (cmd.as_str(), rest) {
        ("faces", [p]) => {
            let faces = fc.faces(&pic(p)?).map_err(|e| e.to_string())?;
            println!("faces: {}", faces.len());
            for f in faces {
                let (turn, height) = capita_face::live::angles(&f);
                println!("score {:.2}  box {:.0},{:.0} {:.0}×{:.0}  eyes {:.0} px  turn {turn:+.2}  nose height {height:.2}", f.score, f.x, f.y, f.w, f.h, f.eye_distance());
            }
            Ok(ExitCode::SUCCESS)
        }
        ("compare", [a, b]) => match fc.compare(&pic(a)?, &pic(b)?).map_err(|e| e.to_string())? {
            None => {
                println!("no face in one of the pictures");
                Ok(ExitCode::from(1))
            }
            Some(l) => {
                let same = l >= fc.settings.same_person;
                println!(
                    "likeness {l:.3} (same person from {:.3}): {}",
                    fc.settings.same_person,
                    if same {
                        "same person"
                    } else {
                        "not the same person"
                    }
                );
                Ok(if same {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                })
            }
        },
        ("challenge", []) => {
            let c = fc.challenge().map_err(|e| e.to_string())?;
            let words: Vec<&str> = c.moves.iter().map(|m| m.word()).collect();
            println!(
                "look straight, then: {}, then straight again",
                words.join(", ")
            );
            println!("moves: {}", words.join(","));
            Ok(ExitCode::SUCCESS)
        }
        ("check", [moves, doc, frames @ ..]) => {
            let moves = moves
                .split(',')
                .map(|w| Pose::from_word(w.trim()).ok_or(format!("unknown move: {w}")))
                .collect::<Result<Vec<_>, _>>()?;
            let frames = frames.iter().map(pic).collect::<Result<Vec<_>, _>>()?;
            match fc
                .check_moves(&moves, &pic(doc)?, &frames)
                .map_err(|e| e.to_string())?
            {
                Ok(p) => {
                    println!("passed: alive, same person (likeness {:.3})", p.likeness);
                    Ok(ExitCode::SUCCESS)
                }
                Err(r) => {
                    println!("refused: {r:?}");
                    Ok(ExitCode::from(1))
                }
            }
        }
        _ => Err(USAGE.into()),
    }
}
