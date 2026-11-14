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

//! A lost key: the account moves to a new one.
//!
//! No key is stored anywhere but with its holder. Whoever loses it verifies
//! again as the same person, with a new account under a new key, and the
//! old account moves there: the person, the balance, what was accrued and
//! not yet claimed, the step on the ladder, the seniority. The old account
//! is closed.
//!
//! The host decides when a move may happen (who asked, how long to wait,
//! whether the old key objected); this module decides what a move is.
//!
//! Offene Frage: 2discuss M – the waiting time, and who wins when the old
//! key objects: a thief holding the old key can stop the rightful owner as
//! well as the owner can stop a thief.
//! Offene Frage: B5 – a frozen account does not move (a freeze must not be
//! escaped by a new key); a stolen key frozen by its owner therefore needs
//! whoever may reopen accounts before it can move.
//! Offene Frage: C1 – an account with a credit line or recorded debt does
//! not move; other accounts that owe it money keep paying to the closed
//! account, and the host has to forward what arrives there.

use core::cmp::max;

use alloc::vec;
use alloc::vec::Vec;

use crate::account::{AccountState, AccountStatus, LoanState};
use crate::attributes::{Attestation, VerificationLevel};
use crate::errors::CoreError;
use crate::issuance::carry_forward;
use crate::params::{check_sequence, in_force_at, Params};
use crate::plan::{begin, next_op, require_active, require_settled};
use crate::ports::{Event, LedgerOp, TransferPurpose};
use crate::types::{Amount, Balance, Timestamp};

/// The outcome of [`plan_move`]: both accounts change together. The host
/// applies all of it or nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MovePlan {
    /// New state of the account left behind: closed, bound to nobody.
    pub from: AccountState,
    /// New state of the account of the person from now on.
    pub to: AccountState,
    /// Ledger operations, in order: the balance follows the person.
    pub ops: Vec<LedgerOp>,
    /// Events for the public record, in order.
    pub events: Vec<Event>,
}

/// Plans the move of the account `from` to the account `to`, on the
/// strength of a new statement of an identity service about the holder of
/// `to`.
///
/// - `attestation` must name the person `from` is bound to (or, with
///   `replaces`, the identifier that person was known under before).
/// - `to` must be an open account bound to nobody: the one the person
///   opened with the new key.
/// - Neither account may have a credit line or recorded debt.
///
/// `to` takes over person, verification as stated, residence, step,
/// seniority, the balance, an unspent start grant and everything accrued
/// under the old key up to `now` (set aside for the next claim; the claim
/// window applies as always). `from` is closed for good.
pub fn plan_move(
    from: &AccountState,
    from_balance: Balance,
    to: &AccountState,
    list: &[Params],
    attestation: &Attestation,
    now: Timestamp,
) -> Result<MovePlan, CoreError> {
    check_sequence(list)?;
    let mut old = begin(from, now)?;
    let mut new = begin(to, now)?;
    if from.id == to.id {
        return Err(CoreError::SameAccount);
    }
    require_active(from)?;
    require_active(to)?;
    if to.person.is_some() {
        return Err(CoreError::MoveTargetBound);
    }
    if attestation.issued_at > now || attestation.expires_at.is_some_and(|end| end <= now) {
        return Err(CoreError::AttestationNotValid);
    }
    let bound = from
        .person
        .is_some_and(|p| p == attestation.person || attestation.replaces == Some(p));
    if !bound {
        return Err(CoreError::NotAccountOfPerson);
    }
    let rest = from_balance.positive_part();
    if from.loan != LoanState::default()
        || to.loan != LoanState::default()
        || Balance::ZERO.checked_credit(rest)? != from_balance
    {
        return Err(CoreError::MoveWithDebt);
    }
    let params = in_force_at(list, now)?;
    require_settled(from, params, now)?;
    require_settled(to, params, now)?;

    // Everything accrued so far, on either side, under the old conditions.
    carry_forward(&mut old, list, now)?;
    carry_forward(&mut new, list, now)?;
    if !old.issuance.carried.is_zero() {
        new.issuance.carried_ripe_at = if new.issuance.carried.is_zero() {
            old.issuance.carried_ripe_at
        } else {
            max(new.issuance.carried_ripe_at, old.issuance.carried_ripe_at)
        };
        new.issuance.carried = new.issuance.carried.checked_add(old.issuance.carried)?;
        old.issuance.carried = Amount::ZERO;
    }
    new.start_grant_reserved = new
        .start_grant_reserved
        .checked_add(old.start_grant_reserved)?;
    old.start_grant_reserved = Amount::ZERO;

    let mut ops = Vec::new();
    let mut events = Vec::new();
    if !rest.is_zero() {
        ops.push(LedgerOp::Transfer {
            id: next_op(&mut old)?,
            from: from.id,
            to: to.id,
            amount: rest,
            purpose: TransferPurpose::AccountMoved,
        });
        events.push(Event::Transferred {
            from: from.id,
            to: to.id,
            amount: rest,
            purpose: TransferPurpose::AccountMoved,
            at: now,
        });
    }

    new.person = Some(attestation.person);
    new.level = attestation.level;
    new.level_until = attestation.expires_at;
    new.residence = attestation.residence.or(from.residence);
    new.opened_at = from.opened_at;
    new.cap_ramp = from.cap_ramp;
    new.tier = from.tier;
    new.tier_since = from.tier_since;

    old.person = None;
    old.level = VerificationLevel(0);
    old.level_until = None;
    old.status = AccountStatus::Closed;

    events.extend(vec![
        Event::AccountMoved {
            from: from.id,
            to: to.id,
            at: now,
        },
        Event::StatusChanged {
            account: from.id,
            from: from.status,
            to: AccountStatus::Closed,
            at: now,
        },
    ]);
    Ok(MovePlan {
        from: old,
        to: new,
        ops,
        events,
    })
}
