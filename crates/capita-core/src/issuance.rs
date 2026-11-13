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

//! Issuance: how much per-capita money an account may claim (ADR 0002).
//!
//! Nothing is issued until claimed. The effective rate of an account is the
//! rate of its ladder step, limited by its cap. Between two claims the rate
//! may change – a new parameter version starts, a verification expires, the
//! seniority ramp rises – so the time is walked piece by piece and split
//! wherever the rate changes.

use core::cmp::max;

use alloc::vec;

use crate::account::{AccountState, AccountStatus, IssuanceState};
use crate::attributes::VerificationLevel;
use crate::errors::{ArithmeticError, CoreError};
use crate::params::{check_sequence, in_force_at, next_change, Params, MAX_SEGMENTS};
use crate::plan::{begin, next_op, require_active, require_settled, Plan};
use crate::ports::{Event, LedgerOp, MintKind, TransferPurpose};
use crate::types::{mul_div, Amount, Duration, Rate, Timestamp};

/// Verification level that counts at `t`: the attested level until it
/// expires, afterwards none.
#[must_use]
pub fn level_at(state: &AccountState, t: Timestamp) -> VerificationLevel {
    match state.level_until {
        Some(until) if t >= until => VerificationLevel(0),
        _ => state.level,
    }
}

/// Rate of the ladder step that counts at `t`: the step of the account if
/// its verification level reaches it, otherwise the highest step the level
/// does reach (the step "falls back"); nothing if there is none.
#[must_use]
pub fn step_rate(params: &Params, state: &AccountState, t: Timestamp) -> Rate {
    let level = level_at(state, t);
    params
        .ladder
        .iter()
        .find(|s| s.id == state.tier && s.min_level <= level)
        .or_else(|| params.ladder.iter().rev().find(|s| s.min_level <= level))
        .map_or(Rate::ZERO, |s| s.rate)
}

/// Cap of the account at `t`, as an amount per `params.caps.period`.
///
/// - No residence, or a country without an entry: the global cap.
/// - Otherwise the cap of the country (GNI per person × share, scaled from
///   a year to the cap period), never below the global cap.
/// - With a seniority ramp: the cap rises from the starting level of the
///   account to that target in equal steps, once per cap period, and
///   reaches it after `caps.ramp`. A target at or below the starting level
///   applies at once.
pub fn cap_at(params: &Params, state: &AccountState, t: Timestamp) -> Result<Amount, CoreError> {
    let caps = &params.caps;
    let floor = caps.global_cap;
    let entry = state
        .residence
        .and_then(|r| caps.by_country.iter().find(|c| c.country == r.country));
    let Some(entry) = entry else {
        return Ok(floor);
    };
    let per_year = entry.pct.of(entry.gni_per_capita)?;
    let per_period = mul_div(per_year.cents(), caps.period.secs(), Duration::YEAR.secs())?;
    let target = max(floor, Amount::from_cents(per_period));
    if caps.ramp.is_zero() {
        return Ok(target);
    }
    let from = state.cap_ramp.from;
    let from = mul_div(from.amount().cents(), caps.period.secs(), from.per().secs())?;
    let from = max(floor, Amount::from_cents(from));
    if from >= target {
        return Ok(target);
    }
    let age = t.secs().saturating_sub(state.cap_ramp.since.secs());
    let period = caps.period.secs();
    let done = age
        .checked_div(period)
        .ok_or(ArithmeticError::DivisionByZero)?;
    let total = caps.ramp.secs().div_ceil(period);
    if done >= total {
        return Ok(target);
    }
    let gap = target.checked_sub(from)?;
    let risen = mul_div(gap.cents(), done, total)?;
    Ok(from.checked_add(Amount::from_cents(risen))?)
}

/// Effective rate at `t`: the step rate, limited by the cap.
pub fn effective_rate(
    params: &Params,
    state: &AccountState,
    t: Timestamp,
) -> Result<Rate, CoreError> {
    let cap = Rate::new(cap_at(params, state, t)?, params.caps.period)?;
    Ok(step_rate(params, state, t).min_value(cap))
}

/// Next moment after `t` at which the seniority ramp of the account rises.
fn next_ramp_step(
    params: &Params,
    state: &AccountState,
    t: Timestamp,
) -> Result<Option<Timestamp>, CoreError> {
    let caps = &params.caps;
    if caps.ramp.is_zero() || state.residence.is_none() {
        return Ok(None);
    }
    let since = state.cap_ramp.since;
    let period = caps.period.secs();
    let Some(age) = t.secs().checked_sub(since.secs()) else {
        // The ramp has not started yet: its first step comes one period
        // after its start.
        return Ok(Some(since.checked_add(caps.period)?));
    };
    let done = age
        .checked_div(period)
        .ok_or(ArithmeticError::DivisionByZero)?;
    if done >= caps.ramp.secs().div_ceil(period) {
        return Ok(None);
    }
    let steps = done.checked_add(1).ok_or(ArithmeticError::Overflow)?;
    let offset = steps.checked_mul(period).ok_or(ArithmeticError::Overflow)?;
    Ok(Some(since.checked_add(Duration::from_secs(offset))?))
}

/// Earliest moment in the open interval (`t`, `end`) at which the effective
/// rate may change.
fn next_break(
    list: &[Params],
    state: &AccountState,
    t: Timestamp,
    end: Timestamp,
) -> Result<Option<Timestamp>, CoreError> {
    let rules = next_change(list, t);
    let expiry = state.level_until.filter(|until| *until > t);
    let ramp = next_ramp_step(in_force_at(list, t)?, state, t)?;
    Ok([rules, expiry, ramp]
        .into_iter()
        .flatten()
        .min()
        .filter(|b| *b < end))
}

/// Result of [`accrue`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accrued {
    /// Entitlement for the time from `from` to the end of the accrual.
    pub amount: Amount,
    /// Start of the time covered (later than the previous end if older
    /// entitlement lapsed).
    pub from: Timestamp,
    /// Bookkeeping after the accrual.
    pub issuance: IssuanceState,
}

/// Entitlement of the account from where its bookkeeping stands up to
/// `upto`. Pure: returns the amount and the new bookkeeping, changes
/// nothing.
///
/// `list` holds every parameter version in force between
/// `state.issuance.paid_until` and `upto` (`ParamsStore::in_force`).
/// Entitlement older than the claim window lapses.
pub fn accrue(
    state: &AccountState,
    list: &[Params],
    upto: Timestamp,
) -> Result<Accrued, CoreError> {
    let mut iss = state.issuance;
    if upto <= iss.paid_until {
        return Ok(Accrued {
            amount: Amount::ZERO,
            from: iss.paid_until,
            issuance: iss,
        });
    }
    let window = in_force_at(list, upto)?.claim_window;
    if upto.duration_since(iss.paid_until)? > window {
        let start = upto.checked_sub(window)?;
        iss.anchor = start;
        iss.paid_since_anchor = Amount::ZERO;
        iss.paid_until = start;
        iss.rate = effective_rate(in_force_at(list, start)?, state, start)?;
    }
    let from = iss.paid_until;
    let mut total = Amount::ZERO;
    let mut t = from;
    let mut pieces: u32 = 0;
    loop {
        let rate = effective_rate(in_force_at(list, t)?, state, t)?;
        if !rate.same_value(iss.rate) {
            // The rate changes at `t`: close the segment, start a new one.
            let due = iss.rate.accrued(t.duration_since(iss.anchor)?)?;
            total = total.checked_add(due.checked_sub(iss.paid_since_anchor)?)?;
            iss.anchor = t;
            iss.paid_since_anchor = Amount::ZERO;
            iss.rate = rate;
        }
        let Some(next) = next_break(list, state, t, upto)? else {
            break;
        };
        pieces = pieces.checked_add(1).ok_or(ArithmeticError::Overflow)?;
        if pieces > MAX_SEGMENTS {
            return Err(CoreError::TooManySegments);
        }
        t = next;
    }
    let due = iss.rate.accrued(upto.duration_since(iss.anchor)?)?;
    total = total.checked_add(due.checked_sub(iss.paid_since_anchor)?)?;
    iss.paid_since_anchor = due;
    iss.paid_until = upto;
    Ok(Accrued {
        amount: total,
        from,
        issuance: iss,
    })
}

/// Accounts everything accrued up to `now` under the present conditions and
/// sets it aside (`carried`) for the next claim; a frozen account accrues
/// nothing. Called before a step,
/// level, residence or status changes, so that the change never reaches
/// back in time. With ripening switched on, the amount set aside becomes
/// mintable one ripening period after `now`.
pub(crate) fn carry_forward(
    state: &mut AccountState,
    list: &[Params],
    now: Timestamp,
) -> Result<(), CoreError> {
    if state.status != AccountStatus::Active {
        return Ok(());
    }
    let accrued = accrue(state, list, now)?;
    state.issuance = accrued.issuance;
    if accrued.amount.is_zero() {
        return Ok(());
    }
    let ripe_at = now.checked_add(in_force_at(list, now)?.ripening)?;
    let iss = &mut state.issuance;
    iss.carried_ripe_at = if iss.carried.is_zero() {
        ripe_at
    } else {
        max(iss.carried_ripe_at, ripe_at)
    };
    iss.carried = iss.carried.checked_add(accrued.amount)?;
    Ok(())
}

/// Plans a claim of per-capita money.
///
/// Mints the entitlement accrued since the last claim (up to `now` minus
/// the ripening delay) plus anything set aside earlier that is ripe. While
/// debt is recorded, `repayment_share` of the amount goes on to the lender.
/// A claim with nothing to mint returns a plan without operations.
///
/// With demurrage switched on and debt recorded, the host settles the
/// account of the lender up to `now` before it applies the plan (the core
/// does not see that account here).
///
/// `list`: every parameter version in force between
/// `state.issuance.paid_until` and `now`.
///
/// Offene Frage: B4 claim or automatic – "automatic" is the host calling
/// this function on a schedule.
pub fn plan_claim(
    state: &AccountState,
    list: &[Params],
    now: Timestamp,
) -> Result<Plan, CoreError> {
    check_sequence(list)?;
    let mut next = begin(state, now)?;
    require_active(state)?;
    let params = in_force_at(list, now)?;
    require_settled(state, params, now)?;

    // Ripening = delayed mint: only what accrued up to the horizon is ripe.
    let mut issued = Amount::ZERO;
    let mut covered = (state.issuance.paid_until, state.issuance.paid_until);
    if let Ok(horizon) = now.checked_sub(params.ripening) {
        let accrued = accrue(state, list, horizon)?;
        next.issuance = accrued.issuance;
        issued = accrued.amount;
        covered = (accrued.from, accrued.issuance.paid_until);
    }
    if !next.issuance.carried.is_zero() && now >= next.issuance.carried_ripe_at {
        issued = issued.checked_add(next.issuance.carried)?;
        next.issuance.carried = Amount::ZERO;
    }
    if issued.is_zero() {
        return Ok(Plan::of(next));
    }
    next.issuance.last_claim = now;

    let mut ops = vec![LedgerOp::Mint {
        id: next_op(&mut next)?,
        to: state.id,
        amount: issued,
        kind: MintKind::Issued,
    }];
    let mut repaid = Amount::ZERO;
    if !state.loan.outstanding.is_zero() {
        let lender = state
            .loan
            .lender
            .ok_or(CoreError::InvalidState("debt without a lender"))?;
        repaid = params
            .loans
            .repayment_share
            .of(issued)?
            .min(issued)
            .min(state.loan.outstanding);
        if !repaid.is_zero() {
            ops.push(LedgerOp::Transfer {
                id: next_op(&mut next)?,
                from: state.id,
                to: lender,
                amount: repaid,
                purpose: TransferPurpose::LoanRepayment,
            });
            next.loan.outstanding = state.loan.outstanding.checked_sub(repaid)?;
            if next.loan.outstanding.is_zero() {
                next.loan.lender = None;
            }
        }
    }
    let events = vec![Event::Claimed {
        account: state.id,
        issued,
        repaid,
        from: covered.0,
        to: covered.1,
    }];
    Ok(Plan {
        state: next,
        ops,
        events,
    })
}

/// Plans the mint of money bought with ordinary money. The payment itself
/// was handled and confirmed outside the core. Purchased money is available
/// at once; no function exists for the way back.
pub fn plan_purchase(
    state: &AccountState,
    params: &Params,
    amount: Amount,
    now: Timestamp,
) -> Result<Plan, CoreError> {
    let mut next = begin(state, now)?;
    require_active(state)?;
    require_settled(state, params, now)?;
    if amount.is_zero() {
        return Err(CoreError::ZeroAmount);
    }
    let ops = vec![LedgerOp::Mint {
        id: next_op(&mut next)?,
        to: state.id,
        amount,
        kind: MintKind::Purchased,
    }];
    let events = vec![Event::Purchased {
        account: state.id,
        amount,
        at: now,
    }];
    Ok(Plan {
        state: next,
        ops,
        events,
    })
}
