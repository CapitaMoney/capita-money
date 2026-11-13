// Capita Money – capita-rulefile
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

//! How values are written in a rules file: money, time and shares as a
//! person writes them, read into the exact integers the core works with.
//!
//! | Written     | Meaning |
//! |-------------|---------|
//! | `"1.00"`    | money: Capita with at most two decimals; `_` may group digits |
//! | `"14 days"` | time: a whole number and a unit; `"off"` = none |
//! | `"50 %"`    | share: per cent with at most four decimals; `"off"` = 0 % |

use core::fmt;
use core::str::FromStr;

use capita_core::types::{Amount, Duration, Share};
use serde::de::{Deserialize, Deserializer, Error as _};

/// The word for "none" of a time or a share.
pub const OFF: &str = "off";

/// Splits `text` into the digits before and after the decimal point.
/// Underscores between digits are dropped. `None` if `text` is no plain
/// decimal number.
fn decimal(text: &str) -> Option<(String, String)> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if text.ends_with('.') || whole.starts_with('_') || whole.ends_with('_') {
        return None;
    }
    let whole: String = whole.chars().filter(|c| *c != '_').collect();
    let all_digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if whole.is_empty() || !all_digits(&whole) || !all_digits(fraction) {
        return None;
    }
    Some((whole, fraction.to_owned()))
}

/// `whole` and `fraction` as one integer with `places` decimal places:
/// `("12", "5")` with two places is 1250.
fn scaled(whole: &str, fraction: &str, places: usize) -> Option<u64> {
    if fraction.len() > places {
        return None;
    }
    let mut digits = String::from(whole);
    digits.push_str(fraction);
    for _ in fraction.len()..places {
        digits.push('0');
    }
    digits.parse().ok()
}

/// An amount of money, written as Capita with at most two decimals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Money(pub Amount);

impl FromStr for Money {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        decimal(text.trim())
            .and_then(|(whole, fraction)| scaled(&whole, &fraction, 2))
            .map(|cents| Self(Amount::from_cents(cents)))
            .ok_or_else(|| {
                format!("\"{text}\" is not an amount of money – write it like \"1.00\" (at most two decimals)")
            })
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cents = self.0.cents();
        let per = Amount::CENTS_PER_CAPITA;
        write!(
            f,
            "{}.{:02}",
            cents.checked_div(per).unwrap_or(0),
            cents.checked_rem(per).unwrap_or(0)
        )
    }
}

/// A span of time, written as a whole number and a unit, or `off`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span(pub Duration);

/// Seconds of one unit of time, by the names a rules file may use.
fn unit_secs(unit: &str) -> Option<u64> {
    let unit = unit.strip_suffix('s').unwrap_or(unit);
    Some(match unit {
        "second" => 1,
        "minute" => Duration::MINUTE.secs(),
        "hour" => Duration::HOUR.secs(),
        "day" => Duration::DAY.secs(),
        "week" => Duration::WEEK.secs(),
        "month" => Duration::MONTH.secs(),
        "year" => Duration::YEAR.secs(),
        _ => return None,
    })
}

impl FromStr for Span {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let trimmed = text.trim();
        if trimmed == OFF {
            return Ok(Self(Duration::ZERO));
        }
        let mut words = trimmed.split_whitespace();
        let secs = match (words.next(), words.next(), words.next()) {
            (Some(number), Some(unit), None) => number
                .parse::<u64>()
                .ok()
                .zip(unit_secs(unit))
                .and_then(|(n, secs)| n.checked_mul(secs)),
            _ => None,
        };
        secs.map(|secs| Self(Duration::from_secs(secs)))
            .ok_or_else(|| {
                format!(
                    "\"{text}\" is not a span of time – write it like \"14 days\" \
                 (seconds, minutes, hours, days, weeks, months, years) or \"off\""
                )
            })
    }
}

/// A share of something, written in per cent with at most four decimals,
/// or `off`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Percent(pub Share);

impl FromStr for Percent {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let trimmed = text.trim();
        if trimmed == OFF {
            return Ok(Self(Share::ZERO));
        }
        trimmed
            .strip_suffix('%')
            .and_then(|number| decimal(number.trim_end()))
            // per cent with four decimals = parts per million
            .and_then(|(whole, fraction)| scaled(&whole, &fraction, 4))
            .and_then(|ppm| u32::try_from(ppm).ok())
            .and_then(|ppm| Share::from_ppm(ppm).ok())
            .map(Self)
            .ok_or_else(|| {
                format!(
                    "\"{text}\" is not a share – write it like \"50 %\" \
                     (0 to 100, at most four decimals) or \"off\""
                )
            })
    }
}

macro_rules! read_from_text {
    ($($name:ident),*) => {$(
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(D::Error::custom)
            }
        }
    )*};
}

read_from_text!(Money, Span, Percent);

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secs = self.0.secs();
        if secs == 0 {
            return f.write_str(OFF);
        }
        // the largest unit that divides the span without remainder
        let units = [
            (Duration::YEAR.secs(), "year"),
            (Duration::MONTH.secs(), "month"),
            (Duration::WEEK.secs(), "week"),
            (Duration::DAY.secs(), "day"),
            (Duration::HOUR.secs(), "hour"),
            (Duration::MINUTE.secs(), "minute"),
        ];
        let (count, unit) = units
            .iter()
            .find(|(unit_secs, _)| secs.checked_rem(*unit_secs) == Some(0))
            .map_or((secs, "second"), |(unit_secs, name)| {
                (secs.checked_div(*unit_secs).unwrap_or(secs), *name)
            });
        let plural = if count == 1 { "" } else { "s" };
        write!(f, "{count} {unit}{plural}")
    }
}

impl fmt::Display for Percent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ppm = self.0.ppm();
        if ppm == 0 {
            return f.write_str(OFF);
        }
        let whole = ppm.checked_div(10_000).unwrap_or(0);
        let fraction = format!("{:04}", ppm.checked_rem(10_000).unwrap_or(0));
        let fraction = fraction.trim_end_matches('0');
        if fraction.is_empty() {
            write!(f, "{whole} %")
        } else {
            write!(f, "{whole}.{fraction} %")
        }
    }
}
