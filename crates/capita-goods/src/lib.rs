// Capita Money – capita-goods
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

//! # capita-goods
//!
//! The rules for sold digital goods (`docs/adr/0019-sold-digital-goods.md`,
//! rules diary 27 and 28): protection by identity, not by locks. A copy is
//! personal (a hidden mark points to the sale, `capita-marks`); whoever leaks
//! it, or sells a work of somebody else, goes up one ladder.
//!
//! | Module           | What |
//! |------------------|------|
//! | [`sanction`]     | the ladder: warning → selling ban → full ban, one calendar month each, reset after clean years; what a member may still do |
//! | [`compensation`] | 1 × price for the seller, from the balance only (no debt) |
//! | [`fingerprint`]  | exact hash of a file and the text print that survives small changes |
//! | [`registry`]     | first registration wins: the match of a new upload against the registered works |
//! | [`licence`]      | downloads of a sale, gifts |
//! | [`calendar`]     | "a month" as people mean it: the 17th to the 17th |
//!
//! Pure: no I/O, no clock, no randomness. All numbers come from the rules
//! file (`[goods]` of `rules/alpha.toml`) through [`GoodsParams`]. Who
//! decides that a violation is proven is not here: the decider (alpha: the
//! board of the Verein) hands a proven violation to [`sanction::Record::apply`].

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod calendar;
pub mod compensation;
pub mod fingerprint;
pub mod licence;
pub mod params;
pub mod registry;
pub mod sanction;

pub use params::GoodsParams;

use capita_core::errors::{ArithmeticError, CoreError};
use core::fmt;

/// What can go wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoodsError {
    /// A number ran over or under.
    Arithmetic,
    /// A day outside the calendar the core knows.
    Calendar,
    /// A time earlier than the last one on record.
    Clock,
    /// The values of the rules file do not fit; the text says which.
    InvalidParams(&'static str),
    /// All downloads of this sale are used.
    DownloadsUsed,
    /// Gifts are off in this rule set.
    GiftOff,
    /// A gift to the holder itself.
    SameHolder,
}

impl fmt::Display for GoodsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arithmetic => f.write_str("a number ran over or under"),
            Self::Calendar => f.write_str("a day outside the calendar"),
            Self::Clock => f.write_str("a time earlier than the last one on record"),
            Self::InvalidParams(what) => write!(f, "the goods rules do not fit: {what}"),
            Self::DownloadsUsed => f.write_str("all downloads of this sale are used"),
            Self::GiftOff => f.write_str("gifts are off"),
            Self::SameHolder => f.write_str("a gift to the holder itself"),
        }
    }
}

impl From<ArithmeticError> for GoodsError {
    fn from(_: ArithmeticError) -> Self {
        Self::Arithmetic
    }
}

impl From<CoreError> for GoodsError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::Arithmetic(_) => Self::Arithmetic,
            _ => Self::Calendar,
        }
    }
}
