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

//! Checks with the real models. They need the models folder in
//! `CAPITA_FACE_MODELS` (`./fetch-models.sh`); without it they say so and pass.
//! No photos of real people are part of the code: these pictures are drawn.

#![allow(clippy::panic)]

use capita_face::live::Pose;
use capita_face::{align, embed::Recogniser, FaceCheck, Refusal, Settings, RECOGNISER_FILE};
use image::{Rgb, RgbImage};
use std::path::PathBuf;

fn models() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var("CAPITA_FACE_MODELS").ok()?);
    if p.join(RECOGNISER_FILE).exists() {
        Some(p)
    } else {
        None
    }
}

/// A drawn "face": oval, two eyes, a mouth. Not a face to the detector.
fn drawing(shift: i64) -> RgbImage {
    RgbImage::from_fn(440, 480, |x, y| {
        let (x, y) = (i64::from(x) - shift, i64::from(y));
        let inside = |cx: i64, cy: i64, rx: i64, ry: i64| {
            (x - cx) * (x - cx) * ry * ry + (y - cy) * (y - cy) * rx * rx <= rx * rx * ry * ry
        };
        if inside(180, 200, 14, 14) || inside(262, 200, 14, 14) {
            Rgb([40, 40, 40])
        } else if inside(222, 300, 35, 10) {
            Rgb([160, 60, 60])
        } else if inside(220, 250, 110, 150) {
            Rgb([210, 170, 140])
        } else {
            Rgb([220, 210, 200])
        }
    })
}

#[test]
fn a_drawing_is_no_face_and_is_refused() {
    let Some(m) = models() else {
        eprintln!("CAPITA_FACE_MODELS not set: skipped");
        return;
    };
    let Ok(fc) = FaceCheck::load(&m, Settings::default()) else {
        panic!("models do not load")
    };
    assert_eq!(fc.faces(&drawing(0)).map(|f| f.len()), Ok(0));
    let frames = vec![drawing(0); 6];
    let r = fc.check_moves(&[Pose::Left, Pose::Up], &drawing(0), &frames);
    assert_eq!(r, Ok(Err(Refusal::DocumentPhoto)));
}

#[test]
fn prints_of_the_same_cut_out_agree_and_of_different_ones_less() {
    let Some(m) = models() else {
        eprintln!("CAPITA_FACE_MODELS not set: skipped");
        return;
    };
    let Ok(r) = Recogniser::load(&m.join(RECOGNISER_FILE)) else {
        panic!("recogniser does not load")
    };
    let lm = [
        (180.3, 200.1),
        (262.7, 195.4),
        (225.0, 250.2),
        (190.0, 300.0),
        (255.0, 297.0),
    ]
    .map(|(x, y)| capita_face::Point { x, y });
    let a = align::crop(&drawing(0), &lm);
    let b = align::crop(&drawing(0), &lm);
    let c = align::crop(&drawing(60), &lm);
    let (Some(a), Some(b), Some(c)) = (a, b, c) else {
        panic!("no cut-out")
    };
    let (Ok(pa), Ok(pb), Ok(pc)) = (r.print(&a), r.print(&b), r.print(&c)) else {
        panic!("no print")
    };
    assert!((pa.cosine(&pb) - 1.0).abs() < 1e-4);
    assert!(pa.cosine(&pc) < 0.99);
}

#[test]
fn a_challenge_is_random_and_usable_once() {
    let Some(m) = models() else {
        eprintln!("CAPITA_FACE_MODELS not set: skipped");
        return;
    };
    let Ok(fc) = FaceCheck::load(&m, Settings::default()) else {
        panic!("models do not load")
    };
    let (Ok(a), Ok(b)) = (fc.challenge(), fc.challenge()) else {
        panic!("no challenge")
    };
    assert_eq!(a.id.len(), 32);
    assert_ne!(a.id, b.id);
    assert_eq!(a.moves.len(), 2);
    assert!(a.moves.windows(2).all(|w| w.first() != w.get(1)));
    assert!(!a.moves.contains(&Pose::Straight));
    let frames = vec![drawing(0); 6];
    assert_eq!(
        fc.check(&a.id, &drawing(0), &frames),
        Ok(Err(Refusal::DocumentPhoto))
    );
    assert_eq!(
        fc.check(&a.id, &drawing(0), &frames),
        Ok(Err(Refusal::Challenge))
    );
    assert_eq!(
        fc.check("unknown", &drawing(0), &frames),
        Ok(Err(Refusal::Challenge))
    );
    assert_eq!(
        fc.check(&b.id, &drawing(0), &frames[..2]),
        Ok(Err(Refusal::FrameCount))
    );
}
