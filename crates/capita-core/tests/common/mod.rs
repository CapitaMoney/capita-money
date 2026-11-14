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

//! Shared helpers for the tests: sample parameters, sample accounts and a
//! small in-memory ledger that enforces the contract of the `Ledger` port.

#![allow(
    dead_code,
    missing_docs,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use std::collections::{BTreeMap, BTreeSet};

use capita_core::account::AccountState;
use capita_core::attributes::{Attestation, Residence, VerificationLevel};
use capita_core::lifecycle::{plan_attest, plan_open};
use capita_core::params::{
    CapParams, ClosedBalance, DemurrageParams, LadderStep, LoanParams, Params,
};
use capita_core::plan::Plan;
use capita_core::ports::{Applied, Ledger, LedgerOp};
use capita_core::totals::Totals;
use capita_core::types::{
    AccountId, Amount, Balance, CountryCode, Duration, OpId, ParamsVersion, PersonId, Rate, Share,
    TierId, Timestamp,
};

pub const T0: u64 = 1_800_000_000;
pub const MINUTE: u64 = 60;
pub const HOUR: u64 = 3_600;
pub const DAY: u64 = 86_400;
pub const MONTH: u64 = 2_629_800;

/// `T0` plus `offset` seconds.
pub fn ts(offset: u64) -> Timestamp {
    Timestamp::from_secs(T0 + offset)
}

pub fn id(n: u8) -> AccountId {
    AccountId([n; 32])
}

pub fn cents(n: u64) -> Amount {
    Amount::from_cents(n)
}

pub fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

pub fn ppm(n: u32) -> Share {
    Share::from_ppm(n).unwrap()
}

pub fn step(id: u16, level: u8, amount: u64, per: u64) -> LadderStep {
    LadderStep {
        id: TierId(id),
        min_level: VerificationLevel(level),
        rate: Rate::new(cents(amount), secs(per)).unwrap(),
    }
}

/// Version 1, in force since the epoch. Ladder: step 0 = 1 cent per hour
/// (unverified), step 1 = 1 cent per minute (level 1), step 2 = 3 cents per
/// minute (level 2). Caps far above the rates, 14-day claim window, no
/// ripening, no demurrage, repayment share 50 %.
pub fn params() -> Params {
    Params {
        version: ParamsVersion(1),
        valid_from: Timestamp::from_secs(0),
        start_grant: cents(100),
        ladder: vec![
            step(0, 0, 1, HOUR),
            step(1, 1, 1, MINUTE),
            step(2, 2, 3, MINUTE),
        ],
        claim_window: secs(14 * DAY),
        caps: CapParams {
            period: Duration::MONTH,
            global_cap: cents(100_000_000),
            by_country: vec![],
            ramp: Duration::ZERO,
        },
        loans: LoanParams {
            max_credit_line: cents(50_000),
            max_outstanding: cents(100_000),
            repayment_share: ppm(500_000),
        },
        demurrage: DemurrageParams {
            rate: Share::ZERO,
            period: Duration::MONTH,
            free_allowance: Amount::ZERO,
        },
        ripening: Duration::ZERO,
        sponsor_keys: vec![],
        closed_balance: ClosedBalance::Keep,
    }
}

/// A successor of `prev`, starting at `from`.
pub fn successor(prev: &Params, from: Timestamp) -> Params {
    let mut next = prev.clone();
    next.version = ParamsVersion(prev.version.0 + 1);
    next.valid_from = from;
    next
}

pub fn attestation(
    person: u8,
    level: u8,
    country: Option<&[u8; 2]>,
    issued_at: Timestamp,
    expires_at: Option<Timestamp>,
) -> Attestation {
    Attestation {
        replaces: None,
        person: PersonId([person; 32]),
        level: VerificationLevel(level),
        residence: country.map(|c| Residence {
            country: CountryCode(*c),
            proven_at: issued_at,
        }),
        attributes: vec![],
        issued_at,
        expires_at,
    }
}

/// A new account `n`, opened at `at`.
pub fn open(n: u8, params: &Params, at: Timestamp) -> AccountState {
    plan_open(id(n), params, at).unwrap().state
}

/// A new account `n`, opened and verified at `at` with `level` (no expiry),
/// standing on the ladder step with the same number. Its start grant has
/// been spent on that verification.
pub fn open_verified(n: u8, level: u8, params: &Params, at: Timestamp) -> AccountState {
    let state = open(n, params, at);
    let att = attestation(n, level, None, at, None);
    let list = [params.clone()];
    let mut state = plan_attest(&state, &list, Some(&att), None, at)
        .unwrap()
        .state;
    state.tier = TierId(u16::from(level));
    state.tier_since = at;
    state.start_grant_reserved = Amount::ZERO;
    state.issuance.rate = capita_core::issuance::effective_rate(params, &state, at).unwrap();
    state
}

/// In-memory ledger. Enforces the contract of the port: each operation at
/// most once, no balance below minus its credit line.
#[derive(Default)]
pub struct Bank {
    pub balances: BTreeMap<AccountId, i64>,
    pub lines: BTreeMap<AccountId, u64>,
    pub applied: BTreeSet<OpId>,
    pub minted: u128,
    pub burnt: u128,
}

impl Bank {
    pub fn balance_of(&self, account: &AccountId) -> Balance {
        Balance::from_cents(self.balances.get(account).copied().unwrap_or(0))
    }

    fn line(&self, account: &AccountId) -> i64 {
        self.lines.get(account).copied().unwrap_or(0) as i64
    }

    fn debit(&mut self, account: &AccountId, amount: Amount) -> Result<(), &'static str> {
        let after = self.balance_of(account).cents() - amount.cents() as i64;
        if after < -self.line(account) {
            return Err("below credit line");
        }
        self.balances.insert(*account, after);
        Ok(())
    }

    fn credit(&mut self, account: &AccountId, amount: Amount) {
        let after = self.balance_of(account).cents() + amount.cents() as i64;
        self.balances.insert(*account, after);
    }

    /// Applies every operation of a plan; panics if the ledger refuses one
    /// (the core must never plan what the ledger refuses).
    pub fn apply_plan(&mut self, plan: &Plan) {
        for op in &plan.ops {
            assert_eq!(self.apply(op), Ok(Applied::Done), "refused: {op:?}");
        }
    }

    /// Every balance respects its credit line.
    pub fn lines_respected(&self) -> bool {
        self.balances.iter().all(|(a, b)| *b >= -self.line(a))
    }

    pub fn sum_of_balances(&self) -> i128 {
        self.balances.values().map(|b| i128::from(*b)).sum()
    }
}

impl Ledger for Bank {
    type Error = &'static str;

    fn balance(&self, account: &AccountId) -> Result<Balance, Self::Error> {
        Ok(self.balance_of(account))
    }

    fn apply(&mut self, op: &LedgerOp) -> Result<Applied, Self::Error> {
        let op_id = match op {
            LedgerOp::Mint { id, .. }
            | LedgerOp::Burn { id, .. }
            | LedgerOp::Transfer { id, .. }
            | LedgerOp::SetCreditLine { id, .. } => *id,
        };
        if self.applied.contains(&op_id) {
            return Ok(Applied::AlreadyApplied);
        }
        match op {
            LedgerOp::Mint { to, amount, .. } => {
                self.credit(to, *amount);
                self.minted += u128::from(amount.cents());
            }
            LedgerOp::Burn { from, amount, .. } => {
                self.debit(from, *amount)?;
                self.burnt += u128::from(amount.cents());
            }
            LedgerOp::Transfer {
                from, to, amount, ..
            } => {
                self.debit(from, *amount)?;
                self.credit(to, *amount);
            }
            LedgerOp::SetCreditLine { account, limit, .. } => {
                if self.balance_of(account).cents() < -(limit.cents() as i64) {
                    return Err("line below drawn credit");
                }
                self.lines.insert(*account, limit.cents());
            }
        }
        self.applied.insert(op_id);
        Ok(Applied::Done)
    }

    fn total_supply(&self) -> Result<u128, Self::Error> {
        Ok(self.minted - self.burnt)
    }
}

/// Bank, account states and totals together: applies plans the way a host
/// would.
#[derive(Default)]
pub struct World {
    pub bank: Bank,
    pub accounts: BTreeMap<AccountId, AccountState>,
    pub totals: Totals,
}

impl World {
    pub fn apply(&mut self, plan: Plan) {
        let before = self.accounts.get(&plan.state.id).map_or(0, |s| s.revision);
        assert_eq!(plan.state.revision, before + 1, "revision must rise by one");
        self.bank.apply_plan(&plan);
        for event in &plan.events {
            self.totals.record(event).unwrap();
        }
        self.accounts.insert(plan.state.id, plan.state);
    }

    pub fn state(&self, n: u8) -> &AccountState {
        &self.accounts[&id(n)]
    }

    pub fn balance(&self, n: u8) -> Balance {
        self.bank.balance_of(&id(n))
    }
}
