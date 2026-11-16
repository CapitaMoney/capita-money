// Capita Money – capita-core
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

//! Free transactions instead of fees (rules diary 32).
//!
//! The system takes no fee (rules diary 12), so a fee cannot keep the ledger
//! from being filled with rubbish. What does it instead: every verified
//! person may send a number of transactions per day, free. A key nobody
//! stands behind may only open an account and present its claim – a few
//! times a day, and only so many such keys per block.
//!
//! This module counts; who the sender is and what a transaction asks for,
//! the host says.

use crate::types::Timestamp;

/// Seconds of a day. Days are days of UTC, as everywhere in the rules
/// (ADR 0026).
const DAY_SECS: u64 = 86_400;

/// Who sends a transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sender {
    /// A key of the operation of the system itself (in the alpha: the key
    /// of the authority). Not counted.
    Service,
    /// An account a verified person stands behind: one person, one account,
    /// one quota.
    Person,
    /// A key nobody stands behind yet. It may only open an account and
    /// present its claim.
    NewKey,
}

/// The numbers of the rule (`spam` of a rule set).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Limits {
    /// Transactions a verified person may send per day.
    pub person_per_day: u32,
    /// Transactions a key without a person may send per day.
    pub new_key_per_day: u32,
    /// Transactions of keys without a person one block takes.
    pub new_keys_per_block: u32,
}

impl Limits {
    /// The number of transactions per day for `sender`; `None`: not counted.
    #[must_use]
    pub const fn per_day(&self, sender: Sender) -> Option<u32> {
        match sender {
            Sender::Service => None,
            Sender::Person => Some(self.person_per_day),
            Sender::NewKey => Some(self.new_key_per_day),
        }
    }
}

/// The day `now` falls on: days of UTC since the Unix epoch.
#[must_use]
pub fn day_of(now: Timestamp) -> u32 {
    u32::try_from(now.secs() / DAY_SECS).unwrap_or(u32::MAX)
}

/// What an account sent on the last day it sent anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub struct Usage {
    /// The day counted (`day_of`).
    pub day: u32,
    /// Transactions sent on that day.
    pub used: u32,
}

impl Usage {
    /// Transactions sent on `today`: a new day starts at zero. A day before
    /// the one counted (a clock that went backwards) counts as that day –
    /// nothing is handed out twice.
    #[must_use]
    pub const fn used_on(self, today: u32) -> u32 {
        if today > self.day {
            0
        } else {
            self.used
        }
    }

    /// How many transactions are left on `today` under `limit`.
    #[must_use]
    pub const fn left(self, today: u32, limit: u32) -> u32 {
        limit.saturating_sub(self.used_on(today))
    }

    /// Counts one transaction on `today`. `None`: the quota of the day is
    /// used up.
    #[must_use]
    pub const fn spend(self, today: u32, limit: u32) -> Option<Self> {
        let used = self.used_on(today);
        if used >= limit {
            return None;
        }
        Some(Self {
            day: if today > self.day { today } else { self.day },
            used: used.saturating_add(1),
        })
    }
}
