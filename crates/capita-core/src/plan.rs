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

//! Plans: what a rule decides, before anything is stored or moved.
//!
//! Every rule function is pure: it takes the state of an account, the
//! parameters in force and the time, and returns a [`Plan`]. The host
//! applies the plan as a whole or not at all (see `docs/architecture.md`,
//! "Plan, then apply").

use alloc::vec::Vec;

use crate::account::{AccountState, AccountStatus};
use crate::errors::{ArithmeticError, CoreError};
use crate::params::Params;
use crate::ports::{Event, LedgerOp};
use crate::types::{OpId, Timestamp};

/// The outcome of one rule decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// New state of the account, to be saved with compare-and-swap against
    /// the revision the plan started from.
    pub state: AccountState,
    /// Ledger operations, in order. May be empty.
    pub ops: Vec<LedgerOp>,
    /// Events for the public record, in order. May be empty.
    pub events: Vec<Event>,
}

impl Plan {
    /// A plan that changes only the account state.
    pub(crate) const fn of(state: AccountState) -> Self {
        Self {
            state,
            ops: Vec::new(),
            events: Vec::new(),
        }
    }
}

/// Start of every plan: refuses a clock that went backwards and returns a
/// copy of the state with the time noted and the revision raised.
pub(crate) fn begin(state: &AccountState, now: Timestamp) -> Result<AccountState, CoreError> {
    if now < state.last_seen {
        return Err(CoreError::ClockWentBackwards {
            last: state.last_seen,
            now,
        });
    }
    let mut next = state.clone();
    next.last_seen = now;
    next.revision = state
        .revision
        .checked_add(1)
        .ok_or(ArithmeticError::Overflow)?;
    Ok(next)
}

/// Refuses frozen and closed accounts.
pub(crate) const fn require_active(state: &AccountState) -> Result<(), CoreError> {
    match state.status {
        AccountStatus::Active => Ok(()),
        other => Err(CoreError::AccountNotActive(other)),
    }
}

/// With demurrage switched on, money may move only on an account settled up
/// to `now` – otherwise the charge would be computed on the wrong balance.
pub(crate) fn require_settled(
    state: &AccountState,
    params: &Params,
    now: Timestamp,
) -> Result<(), CoreError> {
    if !params.demurrage.rate.is_zero() && state.demurrage_settled_at != now {
        return Err(CoreError::DemurrageNotSettled);
    }
    Ok(())
}

/// Takes the next operation identifier of the account.
pub(crate) fn next_op(state: &mut AccountState) -> Result<OpId, CoreError> {
    let id = OpId {
        account: state.id,
        seq: state.next_op_seq,
    };
    state.next_op_seq = state
        .next_op_seq
        .checked_add(1)
        .ok_or(ArithmeticError::Overflow)?;
    Ok(id)
}
