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

//! Face check of the identity service: is the person in front of the camera
//! alive, and the person on the identity document?
//!
//! 1. The service issues a [`Challenge`]: a random order of head movements,
//!    valid for a short time, usable once.
//! 2. The app films the person: straight, the movements, straight again.
//! 3. [`FaceCheck::check`] takes the document photo (from the chip of the
//!    document, later task 1a) and the frames; it finds one face per frame
//!    ([`detect`]), checks the movements and that it is one take ([`live`]),
//!    and compares both straight looks with the document photo ([`align`],
//!    [`embed`]).
//!
//! Runs in the identity service, never on the phone: a changed app could
//! otherwise simply answer "same person". Nothing is kept: pictures and face
//! prints live in memory for the length of one call (ADR 0015).

pub mod align;
pub mod detect;
pub mod embed;
pub mod live;

use detect::{Detector, Face};
use embed::Recogniser;
use image::RgbImage;
use live::{Fault, Limits, Pose};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A point in a picture, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Right.
    pub x: f32,
    /// Down.
    pub y: f32,
}

/// Something went wrong before any decision was possible.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// A model file is missing or unusable.
    Model(String),
    /// A picture is unreadable.
    Picture(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Model(m) => write!(f, "model: {m}"),
            Error::Picture(m) => write!(f, "picture: {m}"),
        }
    }
}
impl std::error::Error for Error {}

/// File names of the two models in the models folder (`fetch-models.sh`).
pub const DETECTOR_FILE: &str = "face_detection_yunet_2023mar.onnx";
/// See [`DETECTOR_FILE`].
pub const RECOGNISER_FILE: &str = "face_recognition_sface_2021dec.onnx";

/// Values of the check. Technical values (ADR 0015), not rules.
#[derive(Clone, Debug)]
pub struct Settings {
    /// Same person from this likeness (cosine) on. 0.363 = value published
    /// with SFace for its test sets.
    pub same_person: f32,
    /// Movements per challenge.
    pub moves: usize,
    /// How long a challenge may be answered.
    pub valid_for: Duration,
    /// Fewest and most frames per answer.
    pub frames: (usize, usize),
    /// Limits of the movement check.
    pub limits: Limits,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            same_person: 0.363,
            moves: 2,
            valid_for: Duration::from_secs(60),
            frames: (4, 60),
            limits: Limits::default(),
        }
    }
}

/// Movements the person is asked to make.
#[derive(Clone, Debug, PartialEq)]
pub struct Challenge {
    /// Random, 32 hex digits; names the challenge in the answer.
    pub id: String,
    /// In this order, between a straight look at the start and at the end.
    pub moves: Vec<Pose>,
}

/// The answer to a challenge was not accepted.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    /// Unknown, used or expired challenge.
    Challenge,
    /// Too few or too many frames.
    FrameCount,
    /// The document photo shows no face, or more than one.
    DocumentPhoto,
    /// The frames do not show the movements of a living person in one take.
    NotLive(Fault),
    /// Alive, but not the person of the document.
    NotSamePerson {
        /// Likeness found (cosine).
        likeness: f32,
    },
}

/// Accepted: alive and the person of the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Passed {
    /// Likeness of the weaker of the two straight looks (cosine).
    pub likeness: f32,
}

/// Detector, recogniser and the open challenges.
pub struct FaceCheck {
    detector: Detector,
    recogniser: Recogniser,
    /// Values of the check.
    pub settings: Settings,
    open: Mutex<HashMap<String, (Vec<Pose>, Instant)>>,
}

impl FaceCheck {
    /// Load both models from `folder`.
    pub fn load(folder: &Path, settings: Settings) -> Result<Self, Error> {
        Ok(Self {
            detector: Detector::load(&folder.join(DETECTOR_FILE))?,
            recogniser: Recogniser::load(&folder.join(RECOGNISER_FILE))?,
            settings,
            open: Mutex::new(HashMap::new()),
        })
    }

    /// A new challenge: random movements, no movement twice in a row.
    pub fn challenge(&self) -> Result<Challenge, Error> {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|e| Error::Model(format!("random: {e}")))?;
        let id: String = seed.iter().take(16).map(|b| format!("{b:02x}")).collect();
        let mut moves: Vec<Pose> = Vec::with_capacity(self.settings.moves);
        for b in seed.iter().skip(16).take(self.settings.moves) {
            let left: Vec<Pose> = live::MOVES
                .iter()
                .copied()
                .filter(|m| moves.last() != Some(m))
                .collect();
            if let Some(m) = left.get(usize::from(*b) % left.len().max(1)) {
                moves.push(*m);
            }
        }
        let now = Instant::now();
        if let Ok(mut open) = self.open.lock() {
            open.retain(|_, (_, t)| now.duration_since(*t) <= self.settings.valid_for);
            open.insert(id.clone(), (moves.clone(), now));
        }
        Ok(Challenge { id, moves })
    }

    /// All faces in a picture.
    pub fn faces(&self, img: &RgbImage) -> Result<Vec<Face>, Error> {
        self.detector.detect(img)
    }

    /// Likeness of the strongest face in each of two pictures; `None` if one shows no face.
    pub fn compare(&self, a: &RgbImage, b: &RgbImage) -> Result<Option<f32>, Error> {
        let (Some(pa), Some(pb)) = (self.print_of_strongest(a)?, self.print_of_strongest(b)?)
        else {
            return Ok(None);
        };
        Ok(Some(pa.cosine(&pb)))
    }

    fn print_of_strongest(&self, img: &RgbImage) -> Result<Option<embed::Print>, Error> {
        let faces = self.detector.detect(img)?;
        let Some(f) = faces.first() else {
            return Ok(None);
        };
        self.print(img, f).map(Some)
    }

    fn print(&self, img: &RgbImage, f: &Face) -> Result<embed::Print, Error> {
        let crop = align::crop(img, &f.landmarks)
            .ok_or_else(|| Error::Picture("face cannot be cut out".into()))?;
        self.recogniser.print(&crop)
    }

    /// Check the answer to challenge `id`. The challenge is used up either way.
    pub fn check(
        &self,
        id: &str,
        document: &RgbImage,
        frames: &[RgbImage],
    ) -> Result<Result<Passed, Refusal>, Error> {
        let taken = self.open.lock().ok().and_then(|mut open| open.remove(id));
        let Some((moves, issued)) = taken else {
            return Ok(Err(Refusal::Challenge));
        };
        if issued.elapsed() > self.settings.valid_for {
            return Ok(Err(Refusal::Challenge));
        }
        self.check_moves(&moves, document, frames)
    }

    /// The check without the challenge bookkeeping (tests, command line).
    pub fn check_moves(
        &self,
        moves: &[Pose],
        document: &RgbImage,
        frames: &[RgbImage],
    ) -> Result<Result<Passed, Refusal>, Error> {
        let (lo, hi) = self.settings.frames;
        if frames.len() < lo || frames.len() > hi {
            return Ok(Err(Refusal::FrameCount));
        }
        let doc_faces = self.detector.detect(document)?;
        let [doc_face] = doc_faces.as_slice() else {
            return Ok(Err(Refusal::DocumentPhoto));
        };
        let faces = frames
            .iter()
            .map(|f| self.detector.detect(f))
            .collect::<Result<Vec<_>, _>>()?;
        let take = match live::check(&faces, moves, &self.settings.limits) {
            Ok(t) => t,
            Err(fault) => return Ok(Err(Refusal::NotLive(fault))),
        };
        let pick = |n: usize| -> Result<embed::Print, Error> {
            let (img, f) = frames
                .get(n)
                .zip(faces.get(n).and_then(|v| v.first()))
                .ok_or_else(|| Error::Picture("frame".into()))?;
            self.print(img, f)
        };
        let doc = self.print(document, doc_face)?;
        let likeness = live::likeness(&doc, &pick(take.first)?, &pick(take.last)?);
        Ok(if likeness >= self.settings.same_person {
            Ok(Passed { likeness })
        } else {
            Err(Refusal::NotSamePerson { likeness })
        })
    }
}

/// Read a JPEG or PNG picture.
pub fn read_picture(path: &Path) -> Result<RgbImage, Error> {
    image::open(path)
        .map(|i| i.to_rgb8())
        .map_err(|e| Error::Picture(format!("{}: {e}", path.display())))
}

/// Read a picture from bytes (as sent by the app).
pub fn picture_from_bytes(bytes: &[u8]) -> Result<RgbImage, Error> {
    image::load_from_memory(bytes)
        .map(|i| i.to_rgb8())
        .map_err(|e| Error::Picture(e.to_string()))
}
