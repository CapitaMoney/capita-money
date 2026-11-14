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

//! Error types shared by the whole core.
//!
//! The core never panics and never saturates silently: every failed
//! calculation surfaces as an error value.

use core::fmt;

use crate::account::AccountStatus;
use crate::types::Timestamp;

/// A calculation left the range of its integer type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ArithmeticError {
    /// Result above the maximum of the type.
    Overflow,
    /// Result below the minimum of the type.
    Underflow,
    /// Division by zero (for example a rate with a period of zero seconds).
    DivisionByZero,
}

impl fmt::Display for ArithmeticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Overflow => "arithmetic overflow",
            Self::Underflow => "arithmetic underflow",
            Self::DivisionByZero => "division by zero",
        })
    }
}

impl core::error::Error for ArithmeticError {}

/// Errors of the core rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CoreError {
    /// See [`ArithmeticError`].
    Arithmetic(ArithmeticError),
    /// The host passed a time earlier than one the core has already seen.
    /// Guards against clock manipulation and replayed requests.
    ClockWentBackwards {
        /// Latest time seen so far.
        last: Timestamp,
        /// Time passed in by the host.
        now: Timestamp,
    },
    /// A value violates its documented range (for example a share above
    /// 100 %).
    OutOfRange(&'static str),
    /// A parameter set, or a sequence of them, breaks a structural rule.
    InvalidParams(&'static str),
    /// No parameter version covers the requested time.
    NoParamsInForce,
    /// The stored account state contradicts itself; nothing is done.
    InvalidState(&'static str),
    /// The account is frozen or closed (or, for a reopening, not frozen).
    AccountNotActive(AccountStatus),
    /// An amount of zero where money has to move.
    ZeroAmount,
    /// Payer and payee, or lender and borrower, are the same account.
    SameAccount,
    /// The payment would take the balance below minus the credit line.
    InsufficientFunds,
    /// Demurrage is switched on and must be settled up to `now` before
    /// money moves.
    DemurrageNotSettled,
    /// The step is not on the ladder of the parameters in force.
    UnknownTier,
    /// The verification level does not reach the step asked for.
    LevelTooLow,
    /// The step is not unlocked yet (market-size gate, B3).
    TierGated,
    /// The attestation belongs to another person than the account.
    PersonMismatch,
    /// The person already holds another account.
    DuplicatePerson,
    /// The attestation is no longer, or not yet, valid.
    AttestationNotValid,
    /// The account to move is not the account of the person named.
    NotAccountOfPerson,
    /// The account a person wants to move to is bound to a person already.
    MoveTargetBound,
    /// An account with a credit line or recorded debt cannot move.
    MoveWithDebt,
    /// The credit line asked for exceeds the maximum.
    CreditLineTooHigh,
    /// The loan would exceed the maximum recorded debt.
    DebtLimitExceeded,
    /// The account already owes money to another lender.
    OtherLenderOutstanding,
    /// More would be repaid than is owed.
    RepaymentExceedsDebt,
    /// The start grant has been spent before.
    StartGrantAlreadySpent,
    /// A sponsor filter uses an attribute key that is not published.
    UnknownSponsorKey,
    /// A claim would have to be split into more pieces than the core
    /// handles in one call (a guard against unbounded work).
    TooManySegments,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Arithmetic(e) => write!(f, "{e}"),
            Self::ClockWentBackwards { last, now } => write!(
                f,
                "clock went backwards: last seen {}, now {}",
                last.secs(),
                now.secs()
            ),
            Self::OutOfRange(what) => write!(f, "out of range: {what}"),
            Self::InvalidParams(what) => write!(f, "invalid parameters: {what}"),
            Self::NoParamsInForce => f.write_str("no parameter version in force"),
            Self::InvalidState(what) => write!(f, "invalid account state: {what}"),
            Self::AccountNotActive(status) => write!(f, "account not active: {status:?}"),
            Self::ZeroAmount => f.write_str("amount is zero"),
            Self::SameAccount => f.write_str("both sides are the same account"),
            Self::InsufficientFunds => f.write_str("insufficient funds"),
            Self::DemurrageNotSettled => f.write_str("demurrage not settled"),
            Self::UnknownTier => f.write_str("unknown ladder step"),
            Self::LevelTooLow => f.write_str("verification level too low"),
            Self::TierGated => f.write_str("ladder step not unlocked"),
            Self::PersonMismatch => f.write_str("attestation is for another person"),
            Self::DuplicatePerson => f.write_str("person already holds an account"),
            Self::AttestationNotValid => f.write_str("attestation not valid"),
            Self::NotAccountOfPerson => f.write_str("not the account of this person"),
            Self::MoveTargetBound => f.write_str("target account belongs to a person already"),
            Self::MoveWithDebt => f.write_str("account with credit line or debt cannot move"),
            Self::CreditLineTooHigh => f.write_str("credit line above the maximum"),
            Self::DebtLimitExceeded => f.write_str("debt above the maximum"),
            Self::OtherLenderOutstanding => f.write_str("debt to another lender outstanding"),
            Self::RepaymentExceedsDebt => f.write_str("repayment exceeds the debt"),
            Self::StartGrantAlreadySpent => f.write_str("start grant already spent"),
            Self::UnknownSponsorKey => f.write_str("attribute key not published for sponsors"),
            Self::TooManySegments => f.write_str("too many accrual segments"),
        }
    }
}

impl core::error::Error for CoreError {}

impl From<ArithmeticError> for CoreError {
    fn from(e: ArithmeticError) -> Self {
        Self::Arithmetic(e)
    }
}
