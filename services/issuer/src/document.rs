// Capita Money – capita-issuer
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The document check (ADR 0021): the chip of a passport or identity card,
//! read by the phone, and the person in front of its camera.
//!
//! 1. `POST /v1/document/start` – the service answers a random challenge
//!    of head movements (ADR 0017).
//! 2. The phone reads the chip (EF.SOD, DG1, DG2) and films the movements.
//! 3. `POST /v1/document/finish` – the service checks the chip against its
//!    trust list (`capita-mrtd`), refuses expired documents, compares the
//!    chip photo with the film and checks the movements (`capita-face`).
//!    Name and date of birth from the chip make the person key; the claim
//!    is signed as by every other check.
//!
//! Nothing is kept: chip data, photo and frames live for one call.

use std::path::PathBuf;

use capita_face::{FaceCheck, Refusal as FaceRefusal, Settings};
use capita_mrtd::groups::ImageFormat;
use capita_mrtd::pa::TrustList;
use capita_mrtd::{read_chip, Chip, MrtdError};
use capita_person_key::canonical::{Date, Name};
use serde::Deserialize;

use crate::issue::Person;

/// `[checks.document]` of the configuration.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentConfig {
    /// Country signing certificates: a folder of PEM/DER files or an ICAO
    /// master list (`.ml`).
    pub trust: PathBuf,
    /// The face models (`services/face/fetch-models.sh`).
    pub face_models: PathBuf,
    /// Same person from this likeness on (default of `capita-face`).
    #[serde(default)]
    pub same_person: Option<f32>,
    /// Head movements per challenge (default of `capita-face`). More where
    /// no phone check is possible (ADR 0023).
    #[serde(default)]
    pub moves: Option<usize>,
}

/// Why a document check fails.
#[derive(Clone, Debug, PartialEq)]
pub enum DocumentRefusal {
    /// The chip data are not genuine, or not readable.
    Chip(MrtdError),
    /// The document has expired.
    Expired,
    /// A date of the chip is not a day of the calendar.
    BadDate,
    /// The chip photo or a frame is not a readable picture.
    Picture(String),
    /// The face check said no.
    Face(String),
    /// The service failed.
    System(String),
}

impl DocumentRefusal {
    /// Short code for the app.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Chip(_) => "chip",
            Self::Expired => "expired",
            Self::BadDate => "chip",
            Self::Picture(_) => "picture",
            Self::Face(_) => "face",
            Self::System(_) => "system",
        }
    }

    /// What went wrong, in words.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Chip(e) => e.to_string(),
            Self::Expired => "the document has expired".into(),
            Self::BadDate => "a date on the chip is not a day of the calendar".into(),
            Self::Picture(e) | Self::Face(e) | Self::System(e) => e.clone(),
        }
    }
}

/// The chip check and the face check, loaded once.
pub struct DocumentCheck {
    trust: TrustList,
    face: FaceCheck,
}

/// What the phone read from the chip.
pub struct ChipData {
    /// EF.SOD.
    pub sod: Vec<u8>,
    /// DG1.
    pub dg1: Vec<u8>,
    /// DG2.
    pub dg2: Vec<u8>,
}

impl DocumentCheck {
    /// Loads trust list and face models.
    ///
    /// # Errors
    /// What could not be loaded.
    pub fn load(config: &DocumentConfig) -> Result<Self, String> {
        let trust = TrustList::load(&config.trust).map_err(|e| e.to_string())?;
        if trust.is_empty() {
            return Err(format!(
                "{}: no country signing certificate",
                config.trust.display()
            ));
        }
        let mut settings = Settings::default();
        if let Some(same) = config.same_person {
            settings.same_person = same;
        }
        if let Some(moves) = config.moves {
            settings.moves = moves;
        }
        let face = FaceCheck::load(&config.face_models, settings).map_err(|e| format!("{e:?}"))?;
        Ok(Self { trust, face })
    }

    /// How many country signing certificates the trust list holds.
    #[must_use]
    pub fn countries(&self) -> usize {
        self.trust.len()
    }

    /// A new challenge: its id and the head movements, as words.
    ///
    /// # Errors
    /// If no random numbers are available.
    pub fn start(&self) -> Result<(String, Vec<&'static str>), String> {
        let challenge = self.face.challenge().map_err(|e| format!("{e:?}"))?;
        Ok((
            challenge.id,
            challenge.moves.iter().map(|m| m.word()).collect(),
        ))
    }

    /// Checks chip and film. Answers the person the chip names.
    ///
    /// # Errors
    /// Why the check fails ([`DocumentRefusal`]).
    pub fn finish(
        &self,
        challenge: &str,
        chip: &ChipData,
        frames: &[Vec<u8>],
        now: u64,
    ) -> Result<Person, DocumentRefusal> {
        let read = read_chip(&chip.sod, &chip.dg1, &chip.dg2, &self.trust)
            .map_err(DocumentRefusal::Chip)?;
        let person = person_of(&read, now)?;
        let photo = photo_of(&read)?;
        let mut pictures = Vec::with_capacity(frames.len());
        for frame in frames {
            pictures.push(
                capita_face::picture_from_bytes(frame)
                    .map_err(|e| DocumentRefusal::Picture(format!("{e:?}")))?,
            );
        }
        match self.face.check(challenge, &photo, &pictures) {
            Ok(Ok(_passed)) => Ok(person),
            Ok(Err(refusal)) => Err(DocumentRefusal::Face(face_words(&refusal))),
            Err(e) => Err(DocumentRefusal::System(format!("{e:?}"))),
        }
    }
}

fn face_words(refusal: &FaceRefusal) -> String {
    match refusal {
        FaceRefusal::Challenge => "the movements were asked for too long ago; start again".into(),
        FaceRefusal::FrameCount => "too few or too many pictures".into(),
        FaceRefusal::DocumentPhoto => "the photo on the chip shows no single face".into(),
        FaceRefusal::NotLive(fault) => {
            format!("the film does not show the movements in one take ({fault:?})")
        }
        FaceRefusal::NotSamePerson { .. } => "the face is not the face of the document".into(),
    }
}

/// Name and date of birth from a checked chip; refuses an expired document.
///
/// # Errors
/// [`DocumentRefusal::Expired`], [`DocumentRefusal::BadDate`].
pub fn person_of(chip: &Chip, now: u64) -> Result<Person, DocumentRefusal> {
    let today = Date::from_days_since_epoch(now / 86_400)
        .ok_or_else(|| DocumentRefusal::System("clock outside the calendar".into()))?;
    let mrz = &chip.mrz;
    let expiry = Date::new(mrz.expiry.expiry_year(), mrz.expiry.mm, mrz.expiry.dd)
        .map_err(|_| DocumentRefusal::BadDate)?;
    if expiry < today {
        return Err(DocumentRefusal::Expired);
    }
    let born = Date::new(
        mrz.birth.birth_year(today.year()),
        mrz.birth.mm,
        mrz.birth.dd,
    )
    .map_err(|_| DocumentRefusal::BadDate)?;
    Ok(Person::NameBirthdate {
        name: Name {
            family: mrz.family_name.clone(),
            given: mrz.given_names.clone(),
        },
        born,
        place_of_birth: None,
    })
}

fn photo_of(chip: &Chip) -> Result<image::RgbImage, DocumentRefusal> {
    match chip.photo_format {
        ImageFormat::Jpeg => capita_face::picture_from_bytes(&chip.photo)
            .map_err(|e| DocumentRefusal::Picture(format!("{e:?}"))),
        ImageFormat::Jpeg2000 => {
            let image = jpeg2k::Image::from_bytes(&chip.photo)
                .map_err(|e| DocumentRefusal::Picture(format!("JPEG 2000: {e}")))?;
            let dynamic: image::DynamicImage = (&image)
                .try_into()
                .map_err(|e| DocumentRefusal::Picture(format!("JPEG 2000: {e}")))?;
            Ok(dynamic.to_rgb8())
        }
    }
}
