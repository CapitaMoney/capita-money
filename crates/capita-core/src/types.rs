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

//! Primitive value types: money, time, identifiers.
//!
//! All of them are plain integers or byte arrays so that the same data model
//! works in a database row, in JSON and in chain storage (ADR 0004).

use core::cmp::Ordering;

use crate::errors::{ArithmeticError, CoreError};

/// `floor(a × b / d)` with a 128-bit intermediate; an error if `d` is zero
/// or the result does not fit 64 bits.
pub(crate) fn mul_div(a: u64, b: u64, d: u64) -> Result<u64, ArithmeticError> {
    let product = u128::from(a)
        .checked_mul(u128::from(b))
        .ok_or(ArithmeticError::Overflow)?;
    let quotient = product
        .checked_div(u128::from(d))
        .ok_or(ArithmeticError::DivisionByZero)?;
    u64::try_from(quotient).map_err(|_| ArithmeticError::Overflow)
}

/// A non-negative amount of money in cents (1 Capita = 100 cents).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Amount(u64);

impl Amount {
    /// Nothing.
    pub const ZERO: Self = Self(0);
    /// Cents in one Capita.
    pub const CENTS_PER_CAPITA: u64 = 100;

    /// Amount from a number of cents.
    #[must_use]
    pub const fn from_cents(cents: u64) -> Self {
        Self(cents)
    }

    /// Amount from whole Capita.
    pub fn from_capita(capita: u64) -> Result<Self, ArithmeticError> {
        capita
            .checked_mul(Self::CENTS_PER_CAPITA)
            .map(Self)
            .ok_or(ArithmeticError::Overflow)
    }

    /// Number of cents.
    #[must_use]
    pub const fn cents(self) -> u64 {
        self.0
    }

    /// `true` for an amount of zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Sum, or an error on overflow.
    pub fn checked_add(self, rhs: Self) -> Result<Self, ArithmeticError> {
        self.0
            .checked_add(rhs.0)
            .map(Self)
            .ok_or(ArithmeticError::Overflow)
    }

    /// Difference, or an error if `rhs` is larger.
    pub fn checked_sub(self, rhs: Self) -> Result<Self, ArithmeticError> {
        self.0
            .checked_sub(rhs.0)
            .map(Self)
            .ok_or(ArithmeticError::Underflow)
    }
}

/// A signed account balance in cents. Negative = drawn credit line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Balance(i64);

impl Balance {
    /// An empty account.
    pub const ZERO: Self = Self(0);

    /// Balance from signed cents.
    #[must_use]
    pub const fn from_cents(cents: i64) -> Self {
        Self(cents)
    }

    /// Signed number of cents.
    #[must_use]
    pub const fn cents(self) -> i64 {
        self.0
    }

    /// The part above zero (what demurrage may be charged on).
    #[must_use]
    pub fn positive_part(self) -> Amount {
        Amount(u64::try_from(self.0).unwrap_or(0))
    }

    /// `Ok` if `amount` can be paid without the balance falling below minus
    /// `credit_line`.
    pub fn check_debit(self, amount: Amount, credit_line: Amount) -> Result<(), CoreError> {
        let after = i128::from(self.0)
            .checked_sub(i128::from(amount.cents()))
            .ok_or(ArithmeticError::Underflow)?;
        let lowest = i128::from(credit_line.cents())
            .checked_neg()
            .ok_or(ArithmeticError::Underflow)?;
        if after < lowest {
            return Err(CoreError::InsufficientFunds);
        }
        Ok(())
    }

    /// Balance after receiving `amount`.
    pub fn checked_credit(self, amount: Amount) -> Result<Self, ArithmeticError> {
        let cents = i64::try_from(amount.cents()).map_err(|_| ArithmeticError::Overflow)?;
        self.0
            .checked_add(cents)
            .map(Self)
            .ok_or(ArithmeticError::Overflow)
    }

    /// Balance after paying `amount`. Does not check the credit line.
    pub fn checked_debit(self, amount: Amount) -> Result<Self, ArithmeticError> {
        let cents = i64::try_from(amount.cents()).map_err(|_| ArithmeticError::Overflow)?;
        self.0
            .checked_sub(cents)
            .map(Self)
            .ok_or(ArithmeticError::Underflow)
    }
}

/// A span of time in whole seconds.
///
/// The core has no calendar and no time zones: "month" and "year" are fixed
/// numbers of seconds (ADR 0003).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Duration(u64);

impl Duration {
    /// No time.
    pub const ZERO: Self = Self(0);
    /// 60 seconds.
    pub const MINUTE: Self = Self(60);
    /// 60 minutes.
    pub const HOUR: Self = Self(3_600);
    /// 24 hours.
    pub const DAY: Self = Self(86_400);
    /// 7 days.
    pub const WEEK: Self = Self(604_800);
    /// One twelfth of [`Duration::YEAR`] (30 days 10 h 30 min).
    pub const MONTH: Self = Self(2_629_800);
    /// 365.25 days.
    pub const YEAR: Self = Self(31_557_600);

    /// Duration from seconds.
    #[must_use]
    pub const fn from_secs(secs: u64) -> Self {
        Self(secs)
    }

    /// Number of seconds.
    #[must_use]
    pub const fn secs(self) -> u64 {
        self.0
    }

    /// `true` for a duration of zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

/// A point in time: whole seconds since the Unix epoch, UTC.
///
/// Always supplied by the host (server clock or block time), never by a
/// device and never read by the core itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Timestamp(u64);

impl Timestamp {
    /// Timestamp from seconds since the Unix epoch.
    #[must_use]
    pub const fn from_secs(secs: u64) -> Self {
        Self(secs)
    }

    /// Seconds since the Unix epoch.
    #[must_use]
    pub const fn secs(self) -> u64 {
        self.0
    }

    /// Time elapsed since `earlier`; an error if `earlier` lies in the future.
    pub fn duration_since(self, earlier: Self) -> Result<Duration, CoreError> {
        self.0
            .checked_sub(earlier.0)
            .map(Duration)
            .ok_or(CoreError::ClockWentBackwards {
                last: earlier,
                now: self,
            })
    }

    /// This time plus `d`.
    pub fn checked_add(self, d: Duration) -> Result<Self, ArithmeticError> {
        self.0
            .checked_add(d.0)
            .map(Self)
            .ok_or(ArithmeticError::Overflow)
    }

    /// This time minus `d`; an error before the epoch.
    pub fn checked_sub(self, d: Duration) -> Result<Self, ArithmeticError> {
        self.0
            .checked_sub(d.0)
            .map(Self)
            .ok_or(ArithmeticError::Underflow)
    }
}

/// An issuance rate: `amount` per `per`, for example 1 cent per minute.
///
/// A rational number, so no rate needs a float (ADR 0003).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Rate {
    amount: Amount,
    per: Duration,
}

impl Rate {
    /// A rate of `amount` every `per`. `per` must not be zero.
    pub fn new(amount: Amount, per: Duration) -> Result<Self, ArithmeticError> {
        if per.is_zero() {
            return Err(ArithmeticError::DivisionByZero);
        }
        Ok(Self { amount, per })
    }

    /// Amount issued per period.
    #[must_use]
    pub const fn amount(self) -> Amount {
        self.amount
    }

    /// Length of the period (never zero).
    #[must_use]
    pub const fn per(self) -> Duration {
        self.per
    }

    /// A rate of nothing.
    pub const ZERO: Self = Self {
        amount: Amount::ZERO,
        per: Duration(1),
    };

    /// `true` if the rate issues nothing.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.amount.is_zero()
    }

    /// Amount accrued over `d`, rounded down: `floor(amount × d / per)`.
    pub fn accrued(self, d: Duration) -> Result<Amount, ArithmeticError> {
        mul_div(self.amount.cents(), d.secs(), self.per.secs()).map(Amount)
    }

    /// Compares two rates by value (1 cent per minute = 60 cents per hour).
    /// Exact: cross-multiplication in 128 bits cannot overflow.
    #[must_use]
    pub fn cmp_value(self, other: Self) -> Ordering {
        let left = u128::from(self.amount.cents()).checked_mul(u128::from(other.per.secs()));
        let right = u128::from(other.amount.cents()).checked_mul(u128::from(self.per.secs()));
        left.cmp(&right)
    }

    /// `true` if both rates issue the same amount per time.
    #[must_use]
    pub fn same_value(self, other: Self) -> bool {
        self.cmp_value(other) == Ordering::Equal
    }

    /// The lower of two rates.
    #[must_use]
    pub fn min_value(self, other: Self) -> Self {
        if self.cmp_value(other) == Ordering::Greater {
            other
        } else {
            self
        }
    }
}

/// A fraction in parts per million (0 ..= 1 000 000 = 0 % ..= 100 %).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Share(u32);

impl Share {
    /// 0 %.
    pub const ZERO: Self = Self(0);
    /// 100 %.
    pub const ONE: Self = Self(Self::SCALE);
    /// Parts in the whole.
    pub const SCALE: u32 = 1_000_000;

    /// Share from parts per million; an error above 100 %.
    pub fn from_ppm(ppm: u32) -> Result<Self, CoreError> {
        if ppm > Self::SCALE {
            return Err(CoreError::OutOfRange("share above 100 %"));
        }
        Ok(Self(ppm))
    }

    /// Parts per million.
    #[must_use]
    pub const fn ppm(self) -> u32 {
        self.0
    }

    /// `true` for 0 %.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// This share of `amount`, rounded down.
    pub fn of(self, amount: Amount) -> Result<Amount, ArithmeticError> {
        mul_div(amount.cents(), u64::from(self.0), u64::from(Self::SCALE)).map(Amount)
    }
}

/// Opaque account identifier (32 bytes).
///
/// Fits a chain public key; a Taler adapter maps it to a bank user name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AccountId(pub [u8; 32]);

/// Opaque identifier of one unique human being (32 bytes).
///
/// Produced by the identity adapter (for example a salted hash); the core
/// never sees names, documents or birth dates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PersonId(pub [u8; 32]);

/// Identifier of one ledger operation: account plus a per-account sequence
/// number. Ledgers must apply an operation at most once per `OpId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OpId {
    /// Account whose state change produced the operation.
    pub account: AccountId,
    /// Sequence number, taken from `AccountState::next_op_seq`.
    pub seq: u64,
}

/// Stable identifier of one step of the incentive ladder. Stable so that
/// steps can be inserted later without renumbering accounts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TierId(pub u16);

/// Version number of a parameter set; strictly increasing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParamsVersion(pub u32);

/// ISO 3166-1 alpha-2 country code in upper-case ASCII, for example `*b"BI"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CountryCode(pub [u8; 2]);
