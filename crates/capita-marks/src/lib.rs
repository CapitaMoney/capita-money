// Capita Money – capita-marks
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

//! # capita-marks
//!
//! Personal copies of sold digital goods and the fingerprints of a work
//! (`docs/adr/0019-sold-digital-goods.md`). Social DRM: no encryption, no
//! device binding, no special reader – the buyer gets an ordinary file that
//! carries a hidden mark pointing to the sale (concept layer L3), and the
//! mark reader finds it again in a leaked file (L4). The fingerprints let
//! the work registry recognise a known work (L2, `capita_goods::registry`).
//!
//! | Kind | Mark | Text for the print |
//! |---|---|---|
//! | EPUB | metadata, hidden element, zero-width frames in every chapter | the chapters in reading order |
//! | PDF | document information, invisible text on every page | the text of the pages |
//! | plain text | zero-width frames | the text |
//! | anything else | – (exact hash only) | – |
//!
//! The buyer sees one sentence at download ([`BUYER_NOTICE`]) and one line
//! in the copy ([`VISIBLE_LINE`]) – no ids, hashes or marks (rule 16).
//! A skilled attacker can strip marks; the aim is deterrence and tracing,
//! not perfect prevention.

#![forbid(unsafe_code)]

pub mod epub;
pub mod payload;
pub mod pdf;
pub mod text;
pub mod xml;

use capita_goods::fingerprint::{ContentHash, Fingerprint, TextPrint};
use payload::{Frame, Payload};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;

/// The line every personal copy shows.
pub const VISIBLE_LINE: &str = "Personal copy, sold via Capita Money.";
/// The one sentence the buyer reads at download.
pub const BUYER_NOTICE: &str = "Your copy is personal. Sharing it can be traced to your account.";

/// What can go wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarksError {
    /// This kind of file gets no personal copy (yet).
    NotSupported(&'static str),
    /// The file is damaged or not what it says.
    Broken(String),
}

impl fmt::Display for MarksError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSupported(what) => write!(f, "no personal copy for {what} yet"),
            Self::Broken(why) => write!(f, "the file cannot be read: {why}"),
        }
    }
}

impl std::error::Error for MarksError {}

/// The kinds of files the service tells apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// An EPUB e-book.
    Epub,
    /// A PDF document.
    Pdf,
    /// Plain text (UTF-8).
    Text,
    /// Anything else: images, audio, video, software.
    Other,
}

impl Kind {
    /// The kind of `bytes`.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        if pdf::is_pdf(bytes) {
            Self::Pdf
        } else if epub::is_epub(bytes) {
            Self::Epub
        } else if !bytes.is_empty() && !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok() {
            Self::Text
        } else {
            Self::Other
        }
    }
}

/// Where a mark was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layer {
    /// EPUB package metadata.
    Metadata,
    /// EPUB hidden element.
    HiddenElement,
    /// Zero-width frames or invisible PDF text.
    InvisibleText,
    /// PDF document information.
    DocumentInfo,
}

/// One mark found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    /// Where.
    pub layer: Layer,
    /// What it says.
    pub payload: Payload,
}

/// What the mark reader concludes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reading {
    /// The payload most marks agree on; `None` if no mark was found.
    pub payload: Option<Payload>,
    /// Every mark found.
    pub found: Vec<Found>,
    /// More than one payload was found: copies were mixed (two buyers'
    /// copies compared and combined). Each payload counts as evidence.
    pub mixed: bool,
}

/// The text of a file as the print sees it: marks and the visible line
/// removed, so a personal copy has the print of the original.
pub fn plain_text(bytes: &[u8]) -> Result<Option<String>, MarksError> {
    let raw = match Kind::of(bytes) {
        Kind::Epub => epub::text(bytes)?,
        Kind::Pdf => pdf::text(bytes)?,
        Kind::Text => String::from_utf8_lossy(bytes).into_owned(),
        Kind::Other => return Ok(None),
    };
    Ok(Some(strip_marks(&raw)))
}

/// `text` without the visible line and the `CPM:` tokens of a PDF mark.
/// Zero-width characters need no removal: the print skips them.
#[must_use]
pub fn strip_marks(text: &str) -> String {
    let mut out = text.replace(VISIBLE_LINE, " ");
    while let Some(i) = out.find(pdf::TOKEN) {
        let end = i
            .saturating_add(pdf::TOKEN.len())
            .saturating_add(16)
            .min(out.len());
        if out.is_char_boundary(end) {
            out.replace_range(i..end, " ");
        } else {
            out.replace_range(i..i.saturating_add(pdf::TOKEN.len()), " ");
        }
    }
    out
}

/// The fingerprints of a file: SHA-256 of the bytes and the print of its
/// text (none for a file without text).
pub fn fingerprint(bytes: &[u8]) -> Result<(Kind, Fingerprint), MarksError> {
    let kind = Kind::of(bytes);
    let mut hash = [0u8; 32];
    for (h, d) in hash.iter_mut().zip(Sha256::digest(bytes).iter()) {
        *h = *d;
    }
    let text = plain_text(bytes)?.and_then(|t| TextPrint::of_text(&t));
    Ok((
        kind,
        Fingerprint {
            hash: ContentHash(hash),
            text,
        },
    ))
}

/// The personal copy of `bytes` for `payload`.
pub fn personalise(bytes: &[u8], payload: &Payload) -> Result<Vec<u8>, MarksError> {
    match Kind::of(bytes) {
        Kind::Epub => epub::personalise(bytes, payload),
        Kind::Pdf => pdf::personalise(bytes, payload),
        Kind::Text => {
            let src = String::from_utf8_lossy(bytes);
            let frame = Frame::of(payload);
            let chars = text::frame_text(&frame);
            let inserts = text::positions(&src, &[(0, src.len())], &frame)
                .into_iter()
                .map(|at| (at, chars.clone()))
                .collect();
            let mut out = xml::splice(&src, inserts);
            out.push_str("\n\n");
            out.push_str(VISIBLE_LINE);
            out.push('\n');
            Ok(out.into_bytes())
        }
        Kind::Other => Err(MarksError::NotSupported(
            "images, audio, video and software (step 2 of the market)",
        )),
    }
}

/// Reads the marks of a (leaked) file.
pub fn read(bytes: &[u8]) -> Result<Reading, MarksError> {
    let found = match Kind::of(bytes) {
        Kind::Epub => epub::read(bytes)?,
        Kind::Pdf => pdf::read(bytes)?,
        Kind::Text => text::find_frames(&String::from_utf8_lossy(bytes))
            .iter()
            .filter_map(Frame::payload)
            .map(|payload| Found {
                layer: Layer::InvisibleText,
                payload,
            })
            .collect(),
        Kind::Other => Vec::new(),
    };
    let mut votes: BTreeMap<Payload, usize> = BTreeMap::new();
    for f in &found {
        let n = votes.entry(f.payload).or_insert(0);
        *n = n.saturating_add(1);
    }
    let payload = votes.iter().max_by_key(|(_, n)| **n).map(|(p, _)| *p);
    Ok(Reading {
        payload,
        mixed: votes.len() > 1,
        found,
    })
}
