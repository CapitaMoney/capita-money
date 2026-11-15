// Capita Money – capita-issuer
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

//! The hand-over box (ADR 0025): two devices of the app pass a few words
//! to each other – a laptop and the phone that verifies it, a new device
//! and the phone that links it.
//!
//! One device makes a box name of 16 random bytes and shows it to the
//! other as a QR code; nobody else knows it. Each side puts what the other
//! needs into a named slot and takes what the other put. A box lives ten
//! minutes from its first word; what is taken is gone. Nothing in a box is
//! secret – a claim is useful only to its subject, a key only with its
//! signature – the box only saves typing.

use std::collections::BTreeMap;

/// How long a box lives, in seconds, from its first word.
pub const BOX_SECS: u64 = 10 * 60;
/// Longest word in a slot, in bytes.
pub const MAX_WORD: usize = 4 * 1024;
/// Slots per box.
pub const MAX_SLOTS: usize = 4;
/// Boxes open at once.
pub const MAX_BOXES: usize = 10_000;

/// Why a word was not taken in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayRefusal {
    /// The box name is not 32 hexadecimal characters.
    BoxName,
    /// The slot name is not 1–16 small letters.
    SlotName,
    /// The word is longer than [`MAX_WORD`].
    TooLong,
    /// The box has [`MAX_SLOTS`] slots already.
    Full,
    /// Too many boxes are open.
    Busy,
}

impl RelayRefusal {
    /// A word for programs.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::BoxName | Self::SlotName | Self::TooLong | Self::Full => "request",
            Self::Busy => "busy",
        }
    }

    /// A sentence for people.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::BoxName => "box: 16 bytes in hexadecimal",
            Self::SlotName => "slot: 1 to 16 small letters",
            Self::TooLong => "word too long",
            Self::Full => "box full",
            Self::Busy => "too many hand-overs open; try again later",
        }
    }
}

struct Box {
    opened: u64,
    slots: BTreeMap<String, String>,
}

/// All boxes, in memory only.
#[derive(Default)]
pub struct Relay {
    boxes: BTreeMap<String, Box>,
}

fn box_name(name: &str) -> Option<String> {
    let name = name.strip_prefix("0x").unwrap_or(name);
    (name.len() == 32 && name.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| name.to_ascii_lowercase())
}

fn slot_name(name: &str) -> bool {
    (1..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase())
}

impl Relay {
    fn sweep(&mut self, now: u64) {
        self.boxes
            .retain(|_, b| now.saturating_sub(b.opened) < BOX_SECS);
    }

    /// Puts `word` into `slot` of box `name` (replacing what was there).
    ///
    /// # Errors
    /// A wrong name, a word too long, a full box, too many boxes.
    pub fn put(
        &mut self,
        name: &str,
        slot: &str,
        word: &str,
        now: u64,
    ) -> Result<(), RelayRefusal> {
        let name = box_name(name).ok_or(RelayRefusal::BoxName)?;
        if !slot_name(slot) {
            return Err(RelayRefusal::SlotName);
        }
        if word.len() > MAX_WORD {
            return Err(RelayRefusal::TooLong);
        }
        self.sweep(now);
        if !self.boxes.contains_key(&name) && self.boxes.len() >= MAX_BOXES {
            return Err(RelayRefusal::Busy);
        }
        let entry = self.boxes.entry(name).or_insert_with(|| Box {
            opened: now,
            slots: BTreeMap::new(),
        });
        if !entry.slots.contains_key(slot) && entry.slots.len() >= MAX_SLOTS {
            return Err(RelayRefusal::Full);
        }
        entry.slots.insert(slot.to_owned(), word.to_owned());
        Ok(())
    }

    /// Takes the word out of `slot` of box `name`, if there is one.
    ///
    /// # Errors
    /// A wrong name.
    pub fn take(
        &mut self,
        name: &str,
        slot: &str,
        now: u64,
    ) -> Result<Option<String>, RelayRefusal> {
        let name = box_name(name).ok_or(RelayRefusal::BoxName)?;
        if !slot_name(slot) {
            return Err(RelayRefusal::SlotName);
        }
        self.sweep(now);
        Ok(self.boxes.get_mut(&name).and_then(|b| b.slots.remove(slot)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAME: &str = "00112233445566778899aabbccddeeff";

    #[test]
    fn a_word_is_taken_once_and_lives_ten_minutes() {
        let mut relay = Relay::default();
        assert_eq!(relay.take(NAME, "claim", 0), Ok(None));
        assert_eq!(relay.put(NAME, "claim", "{}", 100), Ok(()));
        assert_eq!(
            relay.take(&NAME.to_uppercase(), "claim", 101),
            Ok(Some("{}".into()))
        );
        assert_eq!(relay.take(NAME, "claim", 102), Ok(None));
        assert_eq!(relay.put(NAME, "key", "a", 200), Ok(()));
        // the box was opened at 100
        assert_eq!(relay.take(NAME, "key", 100 + BOX_SECS), Ok(None));
    }

    #[test]
    fn names_sizes_and_limits() {
        let mut relay = Relay::default();
        assert_eq!(relay.put("00", "claim", "", 0), Err(RelayRefusal::BoxName));
        assert_eq!(relay.put(NAME, "Claim", "", 0), Err(RelayRefusal::SlotName));
        assert_eq!(relay.put(NAME, "", "", 0), Err(RelayRefusal::SlotName));
        let long = "x".repeat(MAX_WORD + 1);
        assert_eq!(
            relay.put(NAME, "claim", &long, 0),
            Err(RelayRefusal::TooLong)
        );
        for slot in ["a", "b", "c", "d"] {
            assert_eq!(relay.put(NAME, slot, "", 0), Ok(()));
        }
        assert_eq!(relay.put(NAME, "e", "", 0), Err(RelayRefusal::Full));
        assert_eq!(relay.put(NAME, "a", "again", 0), Ok(()));
    }
}
