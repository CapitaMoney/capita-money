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

//! Demurrage: a holding fee on positive balances (ADR 0007). Off by
//! default.
//!
//! Lazy like issuance: nothing is charged until the account is settled.
//! Charge for a span of time = (balance − free allowance) × rate × time ÷
//! period, rounded down; the sub-cent remainder is carried to the next
//! settlement. The host settles an account before money moves on it, so the
//! charge is always computed on the balance that was actually held.
//!
//! Offene Frage: B2 – whether demurrage is used at all, its rate and its
//! free allowance.

use alloc::vec::Vec;

use crate::account::{AccountState, AccountStatus};
use crate::errors::{ArithmeticError, CoreError};
use crate::params::{check_sequence, in_force_at, next_change, DemurrageParams, Params};
use crate::plan::{begin, next_op, Plan};
use crate::ports::{BurnReason, Event, LedgerOp};
use crate::types::{Amount, Balance, Duration, Share, Timestamp};

/// Charge on `balance` for `span` under one parameter version.
/// Returns the charge and the new remainder.
///
/// Never more than the chargeable part of the balance, and never an
/// overflow: once rate × time reaches the whole, the charge is simply
/// everything chargeable.
pub fn charge(
    rules: &DemurrageParams,
    balance: Amount,
    span: Duration,
    carry: u64,
) -> Result<(Amount, u64), CoreError> {
    if rules.rate.is_zero() || span.is_zero() {
        return Ok((Amount::ZERO, carry));
    }
    let Ok(chargeable) = balance.checked_sub(rules.free_allowance) else {
        return Ok((Amount::ZERO, 0));
    };
    if chargeable.is_zero() {
        return Ok((Amount::ZERO, 0));
    }
    if rules.period.is_zero() {
        return Err(ArithmeticError::DivisionByZero.into());
    }
    let whole = u128::from(Share::SCALE)
        .checked_mul(u128::from(rules.period.secs()))
        .ok_or(ArithmeticError::Overflow)?;
    let part = u128::from(rules.rate.ppm())
        .checked_mul(u128::from(span.secs()))
        .ok_or(ArithmeticError::Overflow)?;
    if part >= whole {
        return Ok((chargeable, 0));
    }
    let numerator = u128::from(chargeable.cents())
        .checked_mul(part)
        .and_then(|n| n.checked_add(u128::from(carry)))
        .ok_or(ArithmeticError::Overflow)?;
    let cents = numerator
        .checked_div(whole)
        .ok_or(ArithmeticError::DivisionByZero)?;
    let rest = numerator
        .checked_rem(whole)
        .ok_or(ArithmeticError::DivisionByZero)?;
    let cents = u64::try_from(cents).map_err(|_| ArithmeticError::Overflow)?;
    let rest = u64::try_from(rest).map_err(|_| ArithmeticError::Overflow)?;
    Ok((Amount::from_cents(cents).min(chargeable), rest))
}

/// Plans the settlement of demurrage on the account up to `now`.
///
/// `list`: every parameter version in force between
/// `state.demurrage_settled_at` and `now`. Time under a version without
/// demurrage costs nothing, so switching demurrage on never reaches back.
/// A negative balance is not charged, nor is a start grant not yet spent.
pub fn plan_demurrage(
    state: &AccountState,
    balance: Balance,
    list: &[Params],
    now: Timestamp,
) -> Result<Plan, CoreError> {
    check_sequence(list)?;
    let mut next = begin(state, now)?;
    if state.status == AccountStatus::Closed {
        return Err(CoreError::AccountNotActive(state.status));
    }
    // The start grant is earmarked; it is not charged.
    let mut left = balance
        .positive_part()
        .checked_sub(state.start_grant_reserved)
        .unwrap_or(Amount::ZERO);
    let mut total = Amount::ZERO;
    let mut carry = state.demurrage_carry;
    let mut t = state.demurrage_settled_at;
    while t < now {
        let params = in_force_at(list, t)?;
        let end = next_change(list, t).map_or(now, |change| change.min(now));
        let (cents, rest) = charge(&params.demurrage, left, end.duration_since(t)?, carry)?;
        left = left.checked_sub(cents)?;
        total = total.checked_add(cents)?;
        // A remainder is not carried from one version of the rules to the next.
        carry = if end < now { 0 } else { rest };
        t = end;
    }
    next.demurrage_settled_at = now;
    next.demurrage_carry = carry;
    let mut ops = Vec::new();
    let mut events = Vec::new();
    if !total.is_zero() {
        ops.push(LedgerOp::Burn {
            id: next_op(&mut next)?,
            from: state.id,
            amount: total,
            reason: BurnReason::Demurrage,
        });
        events.push(Event::Burnt {
            account: state.id,
            amount: total,
            reason: BurnReason::Demurrage,
            at: now,
        });
    }
    Ok(Plan {
        state: next,
        ops,
        events,
    })
}
