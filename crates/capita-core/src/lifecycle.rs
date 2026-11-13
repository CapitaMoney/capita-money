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

//! Life of an account: opening, verification, ladder step, status.
//!
//! Whenever something changes that the issuance rate depends on, everything
//! accrued so far is first accounted under the old conditions
//! (`carry_forward`). No change reaches back in time.

use alloc::vec;
use alloc::vec::Vec;

use crate::account::{AccountState, AccountStatus, CapRamp, IssuanceState, LoanState};
use crate::attributes::{Attestation, VerificationLevel};
use crate::errors::CoreError;
use crate::issuance::{cap_at, carry_forward, effective_rate, level_at};
use crate::params::{check_sequence, in_force_at, ClosedBalance, Params};
use crate::plan::{begin, next_op, require_active, require_settled, Plan};
use crate::ports::{BurnReason, Event, LedgerOp, MintKind, TierGate};
use crate::types::{AccountId, Amount, Balance, Rate, TierId, Timestamp};

/// Plans the opening of a new account: lowest ladder step, unverified, and
/// the one-time start grant.
pub fn plan_open(id: AccountId, params: &Params, now: Timestamp) -> Result<Plan, CoreError> {
    let first = params
        .ladder
        .first()
        .ok_or(CoreError::InvalidParams("ladder is empty"))?;
    let mut state = AccountState {
        id,
        person: None,
        status: AccountStatus::Active,
        opened_at: now,
        tier: first.id,
        tier_since: now,
        level: VerificationLevel(0),
        level_until: None,
        residence: None,
        cap_ramp: CapRamp {
            from: Rate::new(params.caps.global_cap, params.caps.period)?,
            since: now,
        },
        start_grant_reserved: params.start_grant,
        issuance: IssuanceState {
            anchor: now,
            rate: Rate::ZERO,
            paid_since_anchor: Amount::ZERO,
            paid_until: now,
            last_claim: now,
            carried: Amount::ZERO,
            carried_ripe_at: now,
        },
        loan: LoanState::default(),
        demurrage_settled_at: now,
        demurrage_carry: 0,
        last_seen: now,
        next_op_seq: 0,
        revision: 1,
    };
    state.issuance.rate = effective_rate(params, &state, now)?;
    let mut ops = Vec::new();
    let mut events = vec![Event::AccountOpened {
        account: id,
        at: now,
    }];
    if !params.start_grant.is_zero() {
        ops.push(LedgerOp::Mint {
            id: next_op(&mut state)?,
            to: id,
            amount: params.start_grant,
            kind: MintKind::Issued,
        });
        events.push(Event::StartGranted {
            account: id,
            amount: params.start_grant,
            at: now,
        });
    }
    Ok(Plan { state, ops, events })
}

/// Plans what follows from a statement of the identity provider.
///
/// - `attestation` `None`: the verification was revoked or never existed;
///   the account counts as unverified from `now`.
/// - Otherwise the account is bound to the person, and level, validity and
///   residence are taken over. `held_by_person` is the account the person
///   already holds, if any (`AccountStore::account_of`): one person, one
///   account.
///
/// A move to another country restarts the seniority ramp from the cap
/// reached so far: to a poorer country the local cap applies at once, to a
/// richer one the difference is ramped up from the day of the move.
///
/// Offene Frage: A2b – details of residence and localisation. An attestation
/// without residence leaves a residence proven earlier untouched.
pub fn plan_attest(
    state: &AccountState,
    list: &[Params],
    attestation: Option<&Attestation>,
    held_by_person: Option<AccountId>,
    now: Timestamp,
) -> Result<Plan, CoreError> {
    check_sequence(list)?;
    let mut next = begin(state, now)?;
    if state.status == AccountStatus::Closed {
        return Err(CoreError::AccountNotActive(state.status));
    }
    let params = in_force_at(list, now)?;
    let Some(att) = attestation else {
        if level_at(state, now) != VerificationLevel(0) {
            carry_forward(&mut next, list, now)?;
        }
        next.level = VerificationLevel(0);
        next.level_until = None;
        return Ok(Plan::of(next));
    };
    if att.issued_at > now || att.expires_at.is_some_and(|end| end <= now) {
        return Err(CoreError::AttestationNotValid);
    }
    if state.person.is_some_and(|p| p != att.person) {
        return Err(CoreError::PersonMismatch);
    }
    if held_by_person.is_some_and(|other| other != state.id) {
        return Err(CoreError::DuplicatePerson);
    }
    let moved = match (state.residence, att.residence) {
        (Some(old), Some(new)) => old.country != new.country,
        (None, Some(_)) => true,
        (_, None) => false,
    };
    if moved || level_at(state, now) != att.level {
        carry_forward(&mut next, list, now)?;
    }
    if state.residence.is_some() && moved {
        next.cap_ramp = CapRamp {
            from: Rate::new(cap_at(params, state, now)?, params.caps.period)?,
            since: now,
        };
    }
    next.person = Some(att.person);
    next.level = att.level;
    next.level_until = att.expires_at;
    if att.residence.is_some() {
        next.residence = att.residence;
    }
    Ok(Plan::of(next))
}

/// Plans a move to another step of the ladder (up or down).
///
/// The step must exist, the verification level must reach it, and the gate
/// must have unlocked it (Offene Frage: B3 – default [`crate::ports::NoGate`]).
pub fn plan_set_tier(
    state: &AccountState,
    list: &[Params],
    gate: &impl TierGate,
    to: TierId,
    now: Timestamp,
) -> Result<Plan, CoreError> {
    check_sequence(list)?;
    let mut next = begin(state, now)?;
    require_active(state)?;
    let params = in_force_at(list, now)?;
    let step = params
        .ladder
        .iter()
        .find(|s| s.id == to)
        .ok_or(CoreError::UnknownTier)?;
    if step.min_level > level_at(state, now) {
        return Err(CoreError::LevelTooLow);
    }
    if !gate.unlocked(to, now) {
        return Err(CoreError::TierGated);
    }
    if to == state.tier {
        return Ok(Plan::of(next));
    }
    carry_forward(&mut next, list, now)?;
    next.tier = to;
    next.tier_since = now;
    let mut plan = Plan::of(next);
    plan.events.push(Event::TierChanged {
        account: state.id,
        from: state.tier,
        to,
        at: now,
    });
    Ok(plan)
}

/// Plans a change of the life-cycle state.
///
/// - Freezing sets aside what accrued so far; a frozen account accrues
///   nothing and cannot claim. Reopening (frozen to active) restarts
///   accrual at `now`.
/// - Closing is final. What was accrued or set aside lapses; a positive
///   balance is burnt if `closed_balance` says so. Recorded debt stays on
///   record.
///
/// Offene Frage: B5 – death, exit, duplicate: who may freeze or close, and
/// what happens to balance and debt. The defaults destroy nothing.
pub fn plan_set_status(
    state: &AccountState,
    balance: Balance,
    list: &[Params],
    to: AccountStatus,
    now: Timestamp,
) -> Result<Plan, CoreError> {
    check_sequence(list)?;
    let mut next = begin(state, now)?;
    if state.status == AccountStatus::Closed {
        return Err(CoreError::AccountNotActive(state.status));
    }
    if to == state.status {
        return Ok(Plan::of(next));
    }
    let params = in_force_at(list, now)?;
    if state.status == AccountStatus::Active {
        carry_forward(&mut next, list, now)?;
    }
    let mut ops = Vec::new();
    let mut events = vec![Event::StatusChanged {
        account: state.id,
        from: state.status,
        to,
        at: now,
    }];
    match to {
        AccountStatus::Active => {
            // Nothing accrues for the time spent frozen.
            next.issuance.anchor = now;
            next.issuance.paid_since_anchor = Amount::ZERO;
            next.issuance.paid_until = now;
            next.issuance.rate = effective_rate(params, &next, now)?;
        }
        AccountStatus::Frozen => {}
        AccountStatus::Closed => {
            next.issuance.carried = Amount::ZERO;
            next.start_grant_reserved = Amount::ZERO;
            let rest = balance.positive_part();
            if params.closed_balance == ClosedBalance::Burn && !rest.is_zero() {
                require_settled(state, params, now)?;
                ops.push(LedgerOp::Burn {
                    id: next_op(&mut next)?,
                    from: state.id,
                    amount: rest,
                    reason: BurnReason::AccountClosed,
                });
                events.push(Event::Burnt {
                    account: state.id,
                    amount: rest,
                    reason: BurnReason::AccountClosed,
                    at: now,
                });
            }
        }
    }
    next.status = to;
    Ok(Plan {
        state: next,
        ops,
        events,
    })
}
