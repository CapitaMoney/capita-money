// SPDX-License-Identifier: AGPL-3.0-only
//! "A month" as people mean it (owner 2026-11-16: "from 17th to 17th").
//!
//! A ban that starts on 17 November at 10:00 UTC ends on 17 December at
//! 10:00 UTC. A day the target month does not have becomes its last day:
//! 31 January plus one month is 28 (or 29) February.

use crate::GoodsError;
use capita_core::types::{Date, Duration, Timestamp};

/// Days of a month of the Gregorian calendar.
#[must_use]
pub const fn days_in_month(year: u16, month: u8) -> u8 {
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `t` plus `months` calendar months, same time of day (UTC).
pub fn add_months(t: Timestamp, months: u8) -> Result<Timestamp, GoodsError> {
    let date = Date::of(t)?;
    let midnight = date.start().ok_or(GoodsError::Calendar)?;
    let time_of_day = t.duration_since(midnight)?;
    let index = u32::from(date.month())
        .checked_sub(1)
        .and_then(|m| m.checked_add(u32::from(months)))
        .ok_or(GoodsError::Arithmetic)?;
    let years = u16::try_from(index.checked_div(12).ok_or(GoodsError::Arithmetic)?)
        .map_err(|_| GoodsError::Arithmetic)?;
    let year = date
        .year()
        .checked_add(years)
        .ok_or(GoodsError::Arithmetic)?;
    let month = u8::try_from(index.checked_rem(12).ok_or(GoodsError::Arithmetic)?)
        .ok()
        .and_then(|m| m.checked_add(1))
        .ok_or(GoodsError::Arithmetic)?;
    let day = date.day().min(days_in_month(year, month));
    let start = Date::new(year, month, day)?
        .start()
        .ok_or(GoodsError::Calendar)?;
    Ok(start.checked_add(Duration::from_secs(time_of_day.secs()))?)
}
