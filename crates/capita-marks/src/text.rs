// SPDX-License-Identifier: AGPL-3.0-only
//! The invisible mark in running text: a frame written in zero-width
//! characters right after a space, many times over the text.
//!
//! `U+2060` (word joiner) opens and closes a frame; between them 80 bits,
//! `U+200B` for 0 and `U+200C` for 1. Readers show nothing; copy and paste
//! of the text keeps them. Where the frames stand depends on the payload,
//! so two copies differ in more places than the bits. A frame after every
//! [`SPACING`] spaces, at least one per document: a leaked chapter alone
//! still carries the mark.

use crate::payload::Frame;

/// Opens and closes a frame.
pub const EDGE: char = '\u{2060}';
/// Bit 0.
pub const ZERO: char = '\u{200B}';
/// Bit 1.
pub const ONE: char = '\u{200C}';
/// One frame after every this many spaces.
pub const SPACING: usize = 300;

/// The frame as characters.
#[must_use]
pub fn frame_text(frame: &Frame) -> String {
    let mut out = String::with_capacity(250);
    out.push(EDGE);
    out.extend(frame.bits().map(|b| if b { ONE } else { ZERO }));
    out.push(EDGE);
    out
}

/// Where in `src` frames go: byte offsets right after a space, inside the
/// given text `ranges`.
#[must_use]
pub fn positions(src: &str, ranges: &[(usize, usize)], frame: &Frame) -> Vec<usize> {
    let spaces: Vec<usize> = ranges
        .iter()
        .flat_map(|&(a, b)| {
            src.get(a..b)
                .unwrap_or("")
                .match_indices(' ')
                .map(move |(i, _)| a.saturating_add(i).saturating_add(1))
        })
        .collect();
    if spaces.is_empty() {
        return Vec::new();
    }
    let offset = usize::from(u16::from_be_bytes([frame.0[0], frame.0[1]]));
    let chosen: Vec<usize> = spaces
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            i.saturating_add(offset)
                .checked_rem(SPACING)
                .is_some_and(|r| r == 0)
        })
        .map(|(_, p)| *p)
        .collect();
    if !chosen.is_empty() {
        return chosen;
    }
    offset
        .checked_rem(spaces.len())
        .and_then(|i| spaces.get(i))
        .copied()
        .into_iter()
        .collect()
}

/// Every well-formed frame in `text` (check bytes not yet tested).
#[must_use]
pub fn find_frames(text: &str) -> Vec<Frame> {
    let mut out = Vec::new();
    let mut bits: Option<Vec<bool>> = None;
    for c in text.chars() {
        match (c, bits.as_mut()) {
            (EDGE, None) => bits = Some(Vec::with_capacity(80)),
            (EDGE, Some(b)) => {
                if let Some(f) = Frame::from_bits(b) {
                    out.push(f);
                    bits = None;
                } else {
                    // a closing edge of something else: start over here
                    bits = Some(Vec::with_capacity(80));
                }
            }
            (ZERO, Some(b)) => b.push(false),
            (ONE, Some(b)) => b.push(true),
            (_, Some(_)) => bits = None,
            (_, None) => {}
        }
    }
    out
}
