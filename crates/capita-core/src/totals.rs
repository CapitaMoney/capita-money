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

//! Public totals: sums over the event record (transparency, E3).
//!
//! Anybody can recompute them from the published events and compare the
//! supply with the ledger – a mismatch is an alarm.

use crate::errors::ArithmeticError;
use crate::ports::{BurnReason, Event, TransferPurpose};
use crate::types::Amount;

/// Running sums over all events. Start from [`Totals::default`] and
/// [`Totals::record`] every event in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Totals {
    /// Accounts ever opened.
    pub accounts_opened: u64,
    /// Start grants issued, in cents.
    pub start_grants: u128,
    /// Per-capita money claimed, in cents.
    pub issued: u128,
    /// Money bought with ordinary money, in cents.
    pub purchased: u128,
    /// Money burnt as demurrage, in cents.
    pub demurrage: u128,
    /// Money burnt from closed accounts, in cents.
    pub closed: u128,
    /// Payments between accounts, in cents.
    pub paid: u128,
    /// Loans handed out, in cents.
    pub lent: u128,
    /// Loans repaid (from claims and voluntarily), in cents.
    pub repaid: u128,
}

impl Totals {
    /// Adds one event to the sums.
    pub fn record(&mut self, event: &Event) -> Result<(), ArithmeticError> {
        match *event {
            Event::AccountOpened { .. } => {
                self.accounts_opened = self
                    .accounts_opened
                    .checked_add(1)
                    .ok_or(ArithmeticError::Overflow)?;
            }
            Event::StartGranted { amount, .. } => add(&mut self.start_grants, amount)?,
            Event::Claimed { issued, repaid, .. } => {
                add(&mut self.issued, issued)?;
                add(&mut self.repaid, repaid)?;
            }
            Event::Purchased { amount, .. } => add(&mut self.purchased, amount)?,
            Event::Transferred {
                amount, purpose, ..
            } => match purpose {
                TransferPurpose::Payment => add(&mut self.paid, amount)?,
                TransferPurpose::LoanDisbursement => add(&mut self.lent, amount)?,
                TransferPurpose::LoanRepayment => add(&mut self.repaid, amount)?,
            },
            Event::Burnt { amount, reason, .. } => match reason {
                BurnReason::Demurrage => add(&mut self.demurrage, amount)?,
                BurnReason::AccountClosed => add(&mut self.closed, amount)?,
            },
            Event::StatusChanged { .. }
            | Event::TierChanged { .. }
            | Event::CreditLineChanged { .. }
            | Event::ParamsEnacted { .. } => {}
        }
        Ok(())
    }

    /// Everything ever minted, in cents.
    pub fn minted(&self) -> Result<u128, ArithmeticError> {
        self.start_grants
            .checked_add(self.issued)
            .and_then(|sum| sum.checked_add(self.purchased))
            .ok_or(ArithmeticError::Overflow)
    }

    /// Everything ever burnt, in cents.
    pub fn burnt(&self) -> Result<u128, ArithmeticError> {
        self.demurrage
            .checked_add(self.closed)
            .ok_or(ArithmeticError::Overflow)
    }

    /// Money in circulation: minted minus burnt. Must equal
    /// `Ledger::total_supply`.
    pub fn supply(&self) -> Result<u128, ArithmeticError> {
        self.minted()?
            .checked_sub(self.burnt()?)
            .ok_or(ArithmeticError::Underflow)
    }

    /// Recorded debt still outstanding: lent minus repaid.
    pub fn outstanding(&self) -> Result<u128, ArithmeticError> {
        self.lent
            .checked_sub(self.repaid)
            .ok_or(ArithmeticError::Underflow)
    }
}

fn add(sum: &mut u128, amount: Amount) -> Result<(), ArithmeticError> {
    *sum = sum
        .checked_add(u128::from(amount.cents()))
        .ok_or(ArithmeticError::Overflow)?;
    Ok(())
}
