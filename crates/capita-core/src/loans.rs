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

//! Interest-free lending and payments (ADR 0006).
//!
//! Two forms of credit, neither with interest:
//! - a **credit line**: the balance of the account may go below zero down
//!   to the line; issuance minted on the account fills it up again;
//! - a **recorded loan**: a lender transfers money, the debt is recorded in
//!   the state of the borrower and repaid from a share of each claim (see
//!   `plan_claim`) or by voluntary repayments.
//!
//! Offene Frage: C1 who lends (person or pool – both are accounts here) and
//! who grants credit lines; C2 fees (none exist); C3 default (debt simply
//! stays until repaid from issuance); C4 maximum amount and term (two
//! parameters for the amounts, no term). One lender per borrower at a time.
//! Repayments also reach a lender whose account has been frozen or closed
//! since – a debt is not cancelled by the state of the lender (B5).

use alloc::vec;

use crate::account::{AccountState, AccountStatus};
use crate::errors::CoreError;
use crate::params::Params;
use crate::plan::{begin, next_op, require_active, require_settled, Plan};
use crate::ports::{Event, LedgerOp, TransferPurpose};
use crate::types::{Amount, Balance, Timestamp};

/// Plans a payment from `payer` to `payee`.
///
/// The balance of the payer may not fall below minus the credit line. With
/// demurrage switched on, both accounts must be settled up to `now` first.
pub fn plan_payment(
    payer: &AccountState,
    payer_balance: Balance,
    payee: &AccountState,
    amount: Amount,
    params: &Params,
    now: Timestamp,
) -> Result<Plan, CoreError> {
    let mut next = begin(payer, now)?;
    transfer_checks(payer, payer_balance, payee, amount, params, now)?;
    let id = next_op(&mut next)?;
    let mut plan = Plan::of(next);
    plan.ops.push(LedgerOp::Transfer {
        id,
        from: payer.id,
        to: payee.id,
        amount,
        purpose: TransferPurpose::Payment,
    });
    plan.events.push(Event::Transferred {
        from: payer.id,
        to: payee.id,
        amount,
        purpose: TransferPurpose::Payment,
        at: now,
    });
    Ok(plan)
}

/// Plans the payment of the start grant for the verification of the
/// account itself, to the account of the verification service. Once only.
pub fn plan_spend_start_grant(
    state: &AccountState,
    balance: Balance,
    service: &AccountState,
    params: &Params,
    now: Timestamp,
) -> Result<Plan, CoreError> {
    if state.start_grant_spent {
        return Err(CoreError::StartGrantAlreadySpent);
    }
    let mut plan = plan_payment(state, balance, service, params.start_grant, params, now)?;
    plan.state.start_grant_spent = true;
    Ok(plan)
}

/// Plans a new credit line for the account (granting, raising, lowering).
///
/// The line may not exceed `max_credit_line`, and it may not be lowered
/// below what is already drawn.
pub fn plan_set_credit_line(
    state: &AccountState,
    balance: Balance,
    limit: Amount,
    params: &Params,
    now: Timestamp,
) -> Result<Plan, CoreError> {
    let mut next = begin(state, now)?;
    require_active(state)?;
    if limit > params.loans.max_credit_line {
        return Err(CoreError::CreditLineTooHigh);
    }
    balance.check_debit(Amount::ZERO, limit)?;
    if limit == state.loan.credit_line {
        return Ok(Plan::of(next));
    }
    let ops = vec![LedgerOp::SetCreditLine {
        id: next_op(&mut next)?,
        account: state.id,
        limit,
    }];
    let events = vec![Event::CreditLineChanged {
        account: state.id,
        from: state.loan.credit_line,
        to: limit,
        at: now,
    }];
    next.loan.credit_line = limit;
    Ok(Plan {
        state: next,
        ops,
        events,
    })
}

/// Plans the hand-out of a recorded loan from `lender` to `borrower`.
///
/// The plan belongs to the borrower (state and operation identifier); the
/// host has to make sure the lender agreed. A borrower owes one lender at a
/// time; a further loan from the same lender adds to the debt.
pub fn plan_lend(
    borrower: &AccountState,
    lender: &AccountState,
    lender_balance: Balance,
    amount: Amount,
    params: &Params,
    now: Timestamp,
) -> Result<Plan, CoreError> {
    let mut next = begin(borrower, now)?;
    transfer_checks(lender, lender_balance, borrower, amount, params, now)?;
    if borrower.loan.lender.is_some_and(|l| l != lender.id) {
        return Err(CoreError::OtherLenderOutstanding);
    }
    let debt = borrower.loan.outstanding.checked_add(amount)?;
    if debt > params.loans.max_outstanding {
        return Err(CoreError::DebtLimitExceeded);
    }
    next.loan.outstanding = debt;
    next.loan.lender = Some(lender.id);
    let ops = vec![LedgerOp::Transfer {
        id: next_op(&mut next)?,
        from: lender.id,
        to: borrower.id,
        amount,
        purpose: TransferPurpose::LoanDisbursement,
    }];
    let events = vec![Event::Transferred {
        from: lender.id,
        to: borrower.id,
        amount,
        purpose: TransferPurpose::LoanDisbursement,
        at: now,
    }];
    Ok(Plan {
        state: next,
        ops,
        events,
    })
}

/// Plans a voluntary repayment of recorded debt, on top of what claims
/// repay by themselves.
pub fn plan_repay(
    borrower: &AccountState,
    balance: Balance,
    amount: Amount,
    params: &Params,
    now: Timestamp,
) -> Result<Plan, CoreError> {
    let mut next = begin(borrower, now)?;
    if borrower.status == AccountStatus::Closed {
        return Err(CoreError::AccountNotActive(borrower.status));
    }
    require_settled(borrower, params, now)?;
    if amount.is_zero() {
        return Err(CoreError::ZeroAmount);
    }
    if amount > borrower.loan.outstanding {
        return Err(CoreError::RepaymentExceedsDebt);
    }
    let lender = borrower
        .loan
        .lender
        .ok_or(CoreError::InvalidState("debt without a lender"))?;
    balance.check_debit(amount, borrower.loan.credit_line)?;
    next.loan.outstanding = borrower.loan.outstanding.checked_sub(amount)?;
    if next.loan.outstanding.is_zero() {
        next.loan.lender = None;
    }
    let ops = vec![LedgerOp::Transfer {
        id: next_op(&mut next)?,
        from: borrower.id,
        to: lender,
        amount,
        purpose: TransferPurpose::LoanRepayment,
    }];
    let events = vec![Event::Transferred {
        from: borrower.id,
        to: lender,
        amount,
        purpose: TransferPurpose::LoanRepayment,
        at: now,
    }];
    Ok(Plan {
        state: next,
        ops,
        events,
    })
}

/// Checks shared by every transfer between two member accounts.
fn transfer_checks(
    from: &AccountState,
    from_balance: Balance,
    to: &AccountState,
    amount: Amount,
    params: &Params,
    now: Timestamp,
) -> Result<(), CoreError> {
    require_active(from)?;
    require_active(to)?;
    if from.id == to.id {
        return Err(CoreError::SameAccount);
    }
    if amount.is_zero() {
        return Err(CoreError::ZeroAmount);
    }
    require_settled(from, params, now)?;
    require_settled(to, params, now)?;
    from_balance.check_debit(amount, from.loan.credit_line)
}
