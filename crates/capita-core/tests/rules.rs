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

//! Worked examples of every rule, with concrete numbers.

#![allow(
    missing_docs,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use capita_core::account::AccountStatus;
use capita_core::attributes::{AttrKey, AttrValue, Attribute, AttributeFilter, Criterion};
use capita_core::demurrage::{charge, plan_demurrage};
use capita_core::errors::CoreError;
use capita_core::issuance::{cap_at, plan_claim, plan_purchase};
use capita_core::lifecycle::{plan_attest, plan_open, plan_set_status, plan_set_tier};
use capita_core::loans::{
    plan_lend, plan_payment, plan_repay, plan_set_credit_line, plan_spend_start_grant,
};
use capita_core::params::{check_sequence, in_force_at, ClosedBalance, CountryCap, Params};
use capita_core::ports::{
    Applied, BurnReason, Event, Ledger, LedgerOp, MintKind, NoGate, TierGate, TransferPurpose,
};
use capita_core::types::{
    Amount, Balance, CountryCode, Duration, ParamsVersion, Rate, Share, TierId, Timestamp,
};

use common::*;

fn issued(plan: &capita_core::plan::Plan) -> u64 {
    plan.events
        .iter()
        .map(|e| match e {
            Event::Claimed { issued, .. } => issued.cents(),
            _ => 0,
        })
        .sum()
}

// ---------------------------------------------------------------- issuance

#[test]
fn one_cent_per_minute_rounds_down_and_keeps_the_remainder() {
    let p = [params()];
    let a = open_verified(1, 1, &p[0], ts(0));
    let first = plan_claim(&a, &p, ts(90)).unwrap();
    assert_eq!(issued(&first), 1);
    // The 30 seconds left over are not lost: at 120 s the second cent is due.
    let second = plan_claim(&first.state, &p, ts(120)).unwrap();
    assert_eq!(issued(&second), 1);
    let third = plan_claim(&second.state, &p, ts(130)).unwrap();
    assert_eq!(issued(&third), 0);
    assert!(third.ops.is_empty() && third.events.is_empty());
}

#[test]
fn claim_mints_issued_money_with_the_next_operation_id() {
    let p = [params()];
    let a = open_verified(1, 1, &p[0], ts(0));
    let plan = plan_claim(&a, &p, ts(HOUR)).unwrap();
    assert_eq!(plan.ops.len(), 1);
    match plan.ops[0] {
        LedgerOp::Mint {
            id: op,
            to,
            amount,
            kind,
        } => {
            assert_eq!(op.account, id(1));
            assert_eq!(op.seq, a.next_op_seq);
            assert_eq!(to, id(1));
            assert_eq!(amount, cents(60));
            assert_eq!(kind, MintKind::Issued);
        }
        ref other => panic!("unexpected {other:?}"),
    }
    assert_eq!(plan.state.next_op_seq, a.next_op_seq + 1);
    assert_eq!(plan.state.revision, a.revision + 1);
    assert_eq!(plan.state.issuance.last_claim, ts(HOUR));
}

#[test]
fn entitlement_older_than_the_claim_window_lapses() {
    let p = [params()];
    let a = open_verified(1, 1, &p[0], ts(0));
    let plan = plan_claim(&a, &p, ts(20 * DAY)).unwrap();
    assert_eq!(issued(&plan), 14 * DAY / MINUTE);
    match plan.events[0] {
        Event::Claimed { from, to, .. } => {
            assert_eq!(from, ts(6 * DAY));
            assert_eq!(to, ts(20 * DAY));
        }
        ref other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn a_change_of_rules_splits_the_claim_and_never_reaches_back() {
    let v1 = params();
    let mut v2 = successor(&v1, ts(DAY));
    v2.ladder[1] = step(1, 1, 2, MINUTE);
    let list = [v1, v2];
    let a = open_verified(1, 1, &list[0], ts(0));
    let plan = plan_claim(&a, &list, ts(2 * DAY)).unwrap();
    assert_eq!(issued(&plan), DAY / MINUTE + 2 * DAY / MINUTE);
    assert_eq!(plan.state.issuance.anchor, ts(DAY));
}

#[test]
fn the_global_cap_limits_the_rate() {
    let mut p = params();
    p.caps.global_cap = cents(1_000); // 10 Capita per month
    p.claim_window = secs(3 * MONTH);
    let a = open_verified(1, 1, &p, ts(0));
    // 1 cent per minute would be 43 830 cents in a month.
    let plan = plan_claim(&a, &[p], ts(MONTH)).unwrap();
    assert_eq!(issued(&plan), 1_000);
}

fn with_country_caps(mut p: Params) -> Params {
    p.caps.global_cap = cents(1_000);
    p.claim_window = secs(12 * MONTH);
    // 12 000 Capita per person and year, 10 % of it = 100 Capita per month.
    p.caps.by_country = vec![
        CountryCap {
            country: CountryCode(*b"DE"),
            gni_per_capita: cents(1_200_000),
            pct: ppm(100_000),
        },
        CountryCap {
            country: CountryCode(*b"BI"),
            gni_per_capita: cents(1_000),
            pct: ppm(100_000),
        },
        CountryCap {
            country: CountryCode(*b"FR"),
            gni_per_capita: cents(2_400_000),
            pct: ppm(100_000),
        },
    ];
    p
}

fn resident(
    n: u8,
    country: &[u8; 2],
    p: &Params,
    at: Timestamp,
) -> capita_core::account::AccountState {
    let mut a = open_verified(n, 1, p, at);
    let att = attestation(n, 1, Some(country), at, None);
    a = plan_attest(&a, core::slice::from_ref(p), Some(&att), None, at)
        .unwrap()
        .state;
    a
}

#[test]
fn the_country_cap_follows_residence_and_never_falls_below_the_global_cap() {
    let p = with_country_caps(params());
    let german = resident(1, b"DE", &p, ts(0));
    let burundian = resident(2, b"BI", &p, ts(0));
    let unknown = resident(3, b"XX", &p, ts(0));
    assert_eq!(cap_at(&p, &german, ts(0)).unwrap(), cents(10_000));
    assert_eq!(cap_at(&p, &burundian, ts(0)).unwrap(), cents(1_000));
    assert_eq!(cap_at(&p, &unknown, ts(0)).unwrap(), cents(1_000));
    let plan = plan_claim(&german, &[p], ts(MONTH)).unwrap();
    assert_eq!(issued(&plan), 10_000);
}

#[test]
fn the_seniority_ramp_raises_the_cap_step_by_step() {
    let mut p = with_country_caps(params());
    p.caps.ramp = secs(2 * MONTH);
    let a = resident(1, b"DE", &p, ts(0));
    assert_eq!(cap_at(&p, &a, ts(0)).unwrap(), cents(1_000));
    assert_eq!(cap_at(&p, &a, ts(MONTH - 1)).unwrap(), cents(1_000));
    assert_eq!(cap_at(&p, &a, ts(MONTH)).unwrap(), cents(5_500));
    assert_eq!(cap_at(&p, &a, ts(2 * MONTH)).unwrap(), cents(10_000));
    assert_eq!(cap_at(&p, &a, ts(50 * MONTH)).unwrap(), cents(10_000));
    let plan = plan_claim(&a, &[p], ts(3 * MONTH)).unwrap();
    assert_eq!(issued(&plan), 1_000 + 5_500 + 10_000);
}

#[test]
fn moving_to_a_poorer_country_applies_its_cap_at_once() {
    let mut p = with_country_caps(params());
    p.caps.ramp = secs(2 * MONTH);
    let list = [p.clone()];
    let a = resident(1, b"DE", &p, ts(0));
    let att = attestation(1, 1, Some(b"BI"), ts(5 * MONTH), None);
    let moved = plan_attest(&a, &list, Some(&att), None, ts(5 * MONTH))
        .unwrap()
        .state;
    assert_eq!(cap_at(&p, &moved, ts(5 * MONTH)).unwrap(), cents(1_000));
    // What accrued in Germany stays owed, under the German cap.
    assert_eq!(moved.issuance.carried, cents(1_000 + 5_500 + 3 * 10_000));
}

#[test]
fn moving_to_a_richer_country_ramps_up_the_difference_from_the_move() {
    let mut p = with_country_caps(params());
    p.caps.ramp = secs(2 * MONTH);
    let list = [p.clone()];
    let a = resident(1, b"DE", &p, ts(0));
    let at = ts(5 * MONTH);
    let att = attestation(1, 1, Some(b"FR"), at, None);
    let moved = plan_attest(&a, &list, Some(&att), None, at).unwrap().state;
    assert_eq!(cap_at(&p, &moved, at).unwrap(), cents(10_000));
    assert_eq!(cap_at(&p, &moved, ts(6 * MONTH)).unwrap(), cents(15_000));
    assert_eq!(cap_at(&p, &moved, ts(7 * MONTH)).unwrap(), cents(20_000));
}

#[test]
fn an_expired_verification_falls_back_to_the_step_still_reached() {
    let p = [params()];
    let a = open_verified(1, 1, &p[0], ts(0));
    let att = attestation(1, 1, None, ts(0), Some(ts(HOUR)));
    let a = plan_attest(&a, &p, Some(&att), None, ts(0)).unwrap().state;
    // One hour at 1 cent per minute, then one hour at 1 cent per hour.
    let plan = plan_claim(&a, &p, ts(2 * HOUR)).unwrap();
    assert_eq!(issued(&plan), 60 + 1);
    // Renewed without a gap: nothing is lost, nothing is paid twice.
    let renewed = attestation(1, 1, None, ts(30 * MINUTE), Some(ts(3 * HOUR)));
    let b = plan_attest(&a, &p, Some(&renewed), None, ts(30 * MINUTE))
        .unwrap()
        .state;
    assert_eq!(issued(&plan_claim(&b, &p, ts(2 * HOUR)).unwrap()), 120);
}

#[test]
fn a_revoked_verification_counts_from_the_moment_of_revocation() {
    let p = [params()];
    let a = open_verified(1, 1, &p[0], ts(0));
    let revoked = plan_attest(&a, &p, None, None, ts(HOUR)).unwrap().state;
    assert_eq!(revoked.issuance.carried, cents(60));
    let plan = plan_claim(&revoked, &p, ts(3 * HOUR)).unwrap();
    assert_eq!(issued(&plan), 60 + 2);
}

#[test]
fn ripening_delays_the_mint_but_not_purchases() {
    let mut p = params();
    p.ripening = secs(7 * DAY);
    p.claim_window = secs(30 * DAY);
    let list = [p.clone()];
    let a = open_verified(1, 1, &p, ts(0));
    assert_eq!(issued(&plan_claim(&a, &list, ts(6 * DAY)).unwrap()), 0);
    let plan = plan_claim(&a, &list, ts(10 * DAY)).unwrap();
    assert_eq!(issued(&plan), 3 * DAY / MINUTE);
    let bought = plan_purchase(&plan.state, &p, cents(500), ts(10 * DAY)).unwrap();
    assert!(matches!(
        bought.ops[0],
        LedgerOp::Mint {
            kind: MintKind::Purchased,
            ..
        }
    ));
    // Money set aside at a change of step ripens one period after the change.
    let changed = plan_set_tier(&plan.state, &list, &NoGate, TierId(0), ts(10 * DAY))
        .unwrap()
        .state;
    assert_eq!(changed.issuance.carried, cents(7 * DAY / MINUTE));
    assert_eq!(changed.issuance.carried_ripe_at, ts(17 * DAY));
    assert_eq!(
        issued(&plan_claim(&changed, &list, ts(16 * DAY)).unwrap()),
        0
    );
    // At day 17: the carried amount is ripe; nothing new has ripened yet.
    let later = plan_claim(&changed, &list, ts(17 * DAY)).unwrap();
    assert_eq!(issued(&later), 7 * DAY / MINUTE);
}

#[test]
fn time_going_backwards_is_refused_everywhere() {
    let p = params();
    let list = [p.clone()];
    let a = open_verified(1, 1, &p, ts(100));
    let b = open_verified(2, 1, &p, ts(0));
    let back = ts(99);
    let zero = Balance::ZERO;
    let err = |r: Result<capita_core::plan::Plan, CoreError>| {
        assert_eq!(
            r.unwrap_err(),
            CoreError::ClockWentBackwards {
                last: ts(100),
                now: back
            }
        );
    };
    err(plan_claim(&a, &list, back));
    err(plan_purchase(&a, &p, cents(1), back));
    err(plan_attest(&a, &list, None, None, back));
    err(plan_set_tier(&a, &list, &NoGate, TierId(0), back));
    err(plan_set_status(
        &a,
        zero,
        &list,
        AccountStatus::Frozen,
        back,
    ));
    err(plan_payment(&a, zero, &b, cents(1), &p, back));
    err(plan_set_credit_line(&a, zero, cents(1), &p, back));
    err(plan_lend(&a, &b, zero, cents(1), &p, back));
    err(plan_repay(&a, zero, cents(1), &p, back));
    err(plan_demurrage(&a, zero, &list, back));
}

// --------------------------------------------------------------- lifecycle

#[test]
fn a_new_account_gets_the_start_grant_once_and_spends_it_on_verification() {
    let p = params();
    let mut world = World::default();
    world.apply(plan_open(id(1), &p, ts(0)).unwrap());
    world.apply(plan_open(id(9), &p, ts(0)).unwrap());
    assert_eq!(world.balance(1), Balance::from_cents(100));
    assert_eq!(world.state(1).tier, TierId(0));
    assert_eq!(world.totals.accounts_opened, 2);
    assert_eq!(world.totals.start_grants, 200);

    let service = world.state(9).clone();
    let plan =
        plan_spend_start_grant(world.state(1), world.balance(1), &service, &p, ts(10)).unwrap();
    world.apply(plan);
    assert_eq!(world.balance(1), Balance::ZERO);
    assert_eq!(world.balance(9), Balance::from_cents(200));
    assert!(world.state(1).start_grant_reserved.is_zero());
    assert_eq!(
        plan_spend_start_grant(world.state(1), world.balance(1), &service, &p, ts(20)),
        Err(CoreError::StartGrantAlreadySpent)
    );
}

#[test]
fn the_start_grant_can_be_spent_on_nothing_but_verification() {
    let mut p = with_demurrage(params());
    p.demurrage.free_allowance = Amount::ZERO;
    let list = [p.clone()];
    let mut world = World::default();
    for n in [1, 2, 9] {
        world.apply(plan_open(id(n), &p, ts(0)).unwrap());
    }
    let (other, service) = (world.state(2).clone(), world.state(9).clone());
    let (a, grant) = (world.state(1).clone(), world.balance(1));
    assert_eq!(a.start_grant_reserved, cents(100));
    // Not as a payment, not as a loan, not even one cent of it.
    assert_eq!(
        plan_payment(&a, grant, &other, cents(1), &p, ts(0)),
        Err(CoreError::InsufficientFunds)
    );
    assert_eq!(
        plan_lend(&other, &a, grant, cents(1), &p, ts(0)),
        Err(CoreError::InsufficientFunds)
    );
    // Money on top of the grant is free; a credit line does not unlock it.
    let more = Balance::from_cents(150);
    assert!(plan_payment(&a, more, &other, cents(50), &p, ts(0)).is_ok());
    assert_eq!(
        plan_payment(&a, more, &other, cents(51), &p, ts(0)),
        Err(CoreError::InsufficientFunds)
    );
    let mut with_line = a.clone();
    with_line.loan.credit_line = cents(30);
    assert!(plan_payment(&with_line, grant, &other, cents(30), &p, ts(0)).is_ok());
    assert_eq!(
        plan_payment(&with_line, grant, &other, cents(31), &p, ts(0)),
        Err(CoreError::InsufficientFunds)
    );
    // Demurrage leaves it alone.
    let later = ts(12 * MONTH);
    let year = plan_demurrage(&a, grant, &list, later).unwrap();
    assert!(year.ops.is_empty());
    // It still buys the verification, in full.
    let service = plan_demurrage(&service, world.balance(9), &list, later)
        .unwrap()
        .state;
    let spent = plan_spend_start_grant(&year.state, grant, &service, &p, later).unwrap();
    assert!(matches!(spent.ops[0], LedgerOp::Transfer { amount, .. } if amount == cents(100)));
    // A later change of the grant in the rules does not change what was
    // granted.
    let mut dearer = p.clone();
    dearer.start_grant = cents(500);
    let spent = plan_spend_start_grant(&year.state, grant, &service, &dearer, later).unwrap();
    assert!(matches!(spent.ops[0], LedgerOp::Transfer { amount, .. } if amount == cents(100)));
}

#[test]
fn time_may_not_go_backwards_for_the_other_side_of_a_transfer() {
    let p = params();
    let a = open_verified(1, 1, &p, ts(0));
    let late = open_verified(2, 1, &p, ts(500));
    let rich = Balance::from_cents(1_000);
    let refused = CoreError::ClockWentBackwards {
        last: ts(500),
        now: ts(100),
    };
    assert_eq!(
        plan_payment(&a, rich, &late, cents(1), &p, ts(100)),
        Err(refused)
    );
    assert_eq!(
        plan_lend(&a, &late, rich, cents(1), &p, ts(100)),
        Err(refused)
    );
}

#[test]
fn one_person_one_account() {
    let p = [params()];
    let a = open(1, &p[0], ts(0));
    let att = attestation(7, 1, None, ts(0), None);
    assert_eq!(
        plan_attest(&a, &p, Some(&att), Some(id(2)), ts(1)),
        Err(CoreError::DuplicatePerson)
    );
    let bound = plan_attest(&a, &p, Some(&att), Some(id(1)), ts(1))
        .unwrap()
        .state;
    let other = attestation(8, 1, None, ts(1), None);
    assert_eq!(
        plan_attest(&bound, &p, Some(&other), None, ts(2)),
        Err(CoreError::PersonMismatch)
    );
    let stale = attestation(7, 1, None, ts(0), Some(ts(2)));
    assert_eq!(
        plan_attest(&bound, &p, Some(&stale), None, ts(2)),
        Err(CoreError::AttestationNotValid)
    );
    let early = attestation(7, 1, None, ts(5), None);
    assert_eq!(
        plan_attest(&bound, &p, Some(&early), None, ts(2)),
        Err(CoreError::AttestationNotValid)
    );
}

struct Locked(TierId);
impl TierGate for Locked {
    fn unlocked(&self, tier: TierId, _now: Timestamp) -> bool {
        tier != self.0
    }
}

#[test]
fn a_step_needs_its_level_and_an_open_gate_and_changes_nothing_in_the_past() {
    let p = [params()];
    let a = open_verified(1, 1, &p[0], ts(0));
    assert_eq!(
        plan_set_tier(&a, &p, &NoGate, TierId(2), ts(HOUR)),
        Err(CoreError::LevelTooLow)
    );
    assert_eq!(
        plan_set_tier(&a, &p, &NoGate, TierId(5), ts(HOUR)),
        Err(CoreError::UnknownTier)
    );
    let att = attestation(1, 2, None, ts(HOUR), None);
    let a = plan_attest(&a, &p, Some(&att), None, ts(HOUR))
        .unwrap()
        .state;
    assert_eq!(
        plan_set_tier(&a, &p, &Locked(TierId(2)), TierId(2), ts(HOUR)),
        Err(CoreError::TierGated)
    );
    let plan = plan_set_tier(&a, &p, &NoGate, TierId(2), ts(2 * HOUR)).unwrap();
    assert!(matches!(plan.events[0], Event::TierChanged { .. }));
    // Two hours on step 1, then one hour on step 2.
    let claim = plan_claim(&plan.state, &p, ts(3 * HOUR)).unwrap();
    assert_eq!(issued(&claim), 120 + 180);
}

#[test]
fn frozen_and_closed_accounts_receive_nothing() {
    let p = params();
    let list = [p.clone()];
    let a = open_verified(1, 1, &p, ts(0));
    let zero = Balance::ZERO;
    let frozen = plan_set_status(&a, zero, &list, AccountStatus::Frozen, ts(HOUR))
        .unwrap()
        .state;
    assert_eq!(
        plan_claim(&frozen, &list, ts(2 * HOUR)),
        Err(CoreError::AccountNotActive(AccountStatus::Frozen))
    );
    assert_eq!(
        plan_purchase(&frozen, &p, cents(1), ts(2 * HOUR)),
        Err(CoreError::AccountNotActive(AccountStatus::Frozen))
    );
    // A change of level while frozen accrues nothing either.
    let att = attestation(1, 2, None, ts(2 * HOUR), None);
    let frozen = plan_attest(&frozen, &list, Some(&att), None, ts(2 * HOUR))
        .unwrap()
        .state;
    // Reopened after five hours: the hour before the freeze is still owed,
    // the frozen hours are not.
    let open_again = plan_set_status(&frozen, zero, &list, AccountStatus::Active, ts(6 * HOUR))
        .unwrap()
        .state;
    let claim = plan_claim(&open_again, &list, ts(7 * HOUR)).unwrap();
    assert_eq!(issued(&claim), 60 + 60);

    let closed = plan_set_status(&a, zero, &list, AccountStatus::Closed, ts(HOUR))
        .unwrap()
        .state;
    assert_eq!(closed.issuance.carried, Amount::ZERO);
    assert_eq!(
        plan_claim(&closed, &list, ts(2 * HOUR)),
        Err(CoreError::AccountNotActive(AccountStatus::Closed))
    );
    assert_eq!(
        plan_set_status(&closed, zero, &list, AccountStatus::Active, ts(2 * HOUR)),
        Err(CoreError::AccountNotActive(AccountStatus::Closed))
    );
    let other = open_verified(2, 1, &p, ts(0));
    assert_eq!(
        plan_payment(
            &other,
            Balance::from_cents(10),
            &closed,
            cents(1),
            &p,
            ts(2 * HOUR)
        ),
        Err(CoreError::AccountNotActive(AccountStatus::Closed))
    );
}

#[test]
fn closing_burns_the_balance_only_if_the_rules_say_so() {
    let mut p = params();
    let a = open_verified(1, 1, &p, ts(0));
    let balance = Balance::from_cents(250);
    let keep = plan_set_status(&a, balance, &[p.clone()], AccountStatus::Closed, ts(1)).unwrap();
    assert!(keep.ops.is_empty());
    p.closed_balance = ClosedBalance::Burn;
    let burn = plan_set_status(&a, balance, &[p], AccountStatus::Closed, ts(1)).unwrap();
    assert!(matches!(
        burn.ops[0],
        LedgerOp::Burn {
            reason: BurnReason::AccountClosed,
            ..
        }
    ));
    let mut totals = capita_core::totals::Totals::default();
    for e in &burn.events {
        totals.record(e).unwrap();
    }
    assert_eq!(totals.closed, 250);
}

// ------------------------------------------------------------------- loans

#[test]
fn a_payment_may_draw_the_credit_line_but_not_more() {
    let p = params();
    let mut world = World::default();
    world.apply(plan_open(id(1), &p, ts(0)).unwrap());
    world.apply(plan_open(id(2), &p, ts(0)).unwrap());
    let payee = world.state(2).clone();
    // Both have spent their start grant on verification; account 1 then
    // bought 100 cents.
    for n in [1, 2] {
        world.bank.balances.insert(id(n), 0);
        world.accounts.get_mut(&id(n)).unwrap().start_grant_reserved = Amount::ZERO;
    }
    world.bank.minted = 0;
    world.totals.start_grants = 0;
    world.apply(plan_purchase(world.state(1), &p, cents(100), ts(1)).unwrap());
    assert_eq!(
        plan_payment(
            world.state(1),
            world.balance(1),
            &payee,
            cents(101),
            &p,
            ts(1)
        ),
        Err(CoreError::InsufficientFunds)
    );
    assert_eq!(
        plan_set_credit_line(world.state(1), world.balance(1), cents(50_001), &p, ts(1)),
        Err(CoreError::CreditLineTooHigh)
    );
    world.apply(
        plan_set_credit_line(world.state(1), world.balance(1), cents(400), &p, ts(1)).unwrap(),
    );
    world.apply(
        plan_payment(
            world.state(1),
            world.balance(1),
            &payee,
            cents(500),
            &p,
            ts(2),
        )
        .unwrap(),
    );
    assert_eq!(world.balance(1), Balance::from_cents(-400));
    assert_eq!(
        plan_payment(
            world.state(1),
            world.balance(1),
            &payee,
            cents(1),
            &p,
            ts(3)
        ),
        Err(CoreError::InsufficientFunds)
    );
    // The line cannot be cut below what is drawn.
    assert_eq!(
        plan_set_credit_line(world.state(1), world.balance(1), cents(399), &p, ts(3)),
        Err(CoreError::InsufficientFunds)
    );
    assert_eq!(
        plan_payment(
            world.state(1),
            world.balance(1),
            world.state(1),
            cents(1),
            &p,
            ts(3)
        ),
        Err(CoreError::SameAccount)
    );
    assert_eq!(
        plan_payment(
            world.state(2),
            world.balance(2),
            world.state(1),
            Amount::ZERO,
            &p,
            ts(3)
        ),
        Err(CoreError::ZeroAmount)
    );
    assert!(world.bank.lines_respected());
    assert_eq!(world.totals.paid, 500);
}

#[test]
fn a_loan_is_repaid_from_a_share_of_each_claim_without_interest() {
    let p = params();
    let list = [p.clone()];
    let mut world = World::default();
    world.accounts.insert(id(1), open_verified(1, 1, &p, ts(0)));
    world.accounts.insert(id(2), open_verified(2, 1, &p, ts(0)));
    world.accounts.insert(id(3), open_verified(3, 1, &p, ts(0)));
    world.bank.balances.insert(id(2), 1_000);
    world.bank.minted = 1_000;
    world.totals.purchased = 1_000;

    let lender = world.state(2).clone();
    let third = world.state(3).clone();
    world.apply(
        plan_lend(
            world.state(1),
            &lender,
            world.balance(2),
            cents(100),
            &p,
            ts(0),
        )
        .unwrap(),
    );
    assert_eq!(world.balance(1), Balance::from_cents(100));
    assert_eq!(world.state(1).loan.outstanding, cents(100));
    assert_eq!(world.state(1).loan.lender, Some(id(2)));
    assert_eq!(
        plan_lend(
            world.state(1),
            &third,
            Balance::from_cents(500),
            cents(1),
            &p,
            ts(0)
        ),
        Err(CoreError::OtherLenderOutstanding)
    );
    assert_eq!(
        plan_lend(
            world.state(1),
            &lender,
            world.balance(2),
            cents(901),
            &p,
            ts(0)
        ),
        Err(CoreError::InsufficientFunds)
    );

    // One hour: 60 cents issued, half of it goes to the lender.
    let claim = plan_claim(world.state(1), &list, ts(HOUR)).unwrap();
    assert!(matches!(
        claim.ops[1],
        LedgerOp::Transfer {
            purpose: TransferPurpose::LoanRepayment,
            amount,
            ..
        } if amount == cents(30)
    ));
    world.apply(claim);
    assert_eq!(world.state(1).loan.outstanding, cents(70));
    assert_eq!(world.balance(1), Balance::from_cents(130));

    // A voluntary repayment on top.
    assert_eq!(
        plan_repay(world.state(1), world.balance(1), cents(71), &p, ts(HOUR)),
        Err(CoreError::RepaymentExceedsDebt)
    );
    world.apply(plan_repay(world.state(1), world.balance(1), cents(60), &p, ts(HOUR)).unwrap());
    // The last 10 cents come out of the next claim – never more than owed.
    world.apply(plan_claim(world.state(1), &list, ts(2 * HOUR)).unwrap());
    assert_eq!(world.state(1).loan.outstanding, Amount::ZERO);
    assert_eq!(world.state(1).loan.lender, None);
    // Lent 100, repaid exactly 100: no interest, no fee.
    assert_eq!(world.balance(2), Balance::from_cents(1_000));
    assert_eq!(world.totals.lent, 100);
    assert_eq!(world.totals.repaid, 100);
    assert_eq!(world.totals.outstanding().unwrap(), 0);
    assert_eq!(
        world.bank.total_supply().unwrap(),
        world.totals.supply().unwrap()
    );
}

#[test]
fn recorded_debt_has_a_maximum() {
    let mut p = params();
    p.loans.max_outstanding = cents(150);
    let a = open_verified(1, 1, &p, ts(0));
    let lender = open_verified(2, 1, &p, ts(0));
    let rich = Balance::from_cents(10_000);
    let first = plan_lend(&a, &lender, rich, cents(100), &p, ts(0)).unwrap();
    assert_eq!(
        plan_lend(&first.state, &lender, rich, cents(51), &p, ts(0)),
        Err(CoreError::DebtLimitExceeded)
    );
    let second = plan_lend(&first.state, &lender, rich, cents(50), &p, ts(0)).unwrap();
    assert_eq!(second.state.loan.outstanding, cents(150));
}

// --------------------------------------------------------------- demurrage

fn with_demurrage(mut p: Params) -> Params {
    p.demurrage.rate = ppm(10_000); // 1 % per month
    p.demurrage.free_allowance = cents(1_000);
    p
}

#[test]
fn demurrage_is_off_by_default() {
    let p = [params()];
    let a = open_verified(1, 1, &p[0], ts(0));
    let plan = plan_demurrage(&a, Balance::from_cents(1_000_000), &p, ts(12 * MONTH)).unwrap();
    assert!(plan.ops.is_empty());
    assert_eq!(plan.state.demurrage_settled_at, ts(12 * MONTH));
}

#[test]
fn demurrage_charges_the_balance_above_the_free_allowance() {
    let p = [with_demurrage(params())];
    let a = open_verified(1, 1, &p[0], ts(0));
    let plan = plan_demurrage(&a, Balance::from_cents(101_000), &p, ts(MONTH)).unwrap();
    assert!(matches!(
        plan.ops[0],
        LedgerOp::Burn {
            reason: BurnReason::Demurrage,
            amount,
            ..
        } if amount == cents(1_000)
    ));
    for balance in [-500, 0, 1_000] {
        let none = plan_demurrage(&a, Balance::from_cents(balance), &p, ts(MONTH)).unwrap();
        assert!(none.ops.is_empty());
    }
}

#[test]
fn frequent_settling_does_not_round_demurrage_away() {
    let p = [with_demurrage(params())];
    let mut a = open_verified(1, 1, &p[0], ts(0));
    let mut balance: i64 = 101_000;
    for hour in 1..=(MONTH / HOUR) {
        let plan = plan_demurrage(&a, Balance::from_cents(balance), &p, ts(hour * HOUR)).unwrap();
        for op in &plan.ops {
            if let LedgerOp::Burn { amount, .. } = op {
                balance -= i64::try_from(amount.cents()).unwrap();
            }
        }
        a = plan.state;
    }
    // Settled 730 times instead of once: slightly less than 1 000 (the
    // charge shrinks with the balance), but nowhere near zero.
    let charged = 101_000 - balance;
    assert!((990..=1_000).contains(&charged), "charged {charged}");
}

#[test]
fn demurrage_never_takes_more_than_the_chargeable_balance() {
    let rules = with_demurrage(params()).demurrage;
    let (all, carry) = charge(&rules, cents(5_000), secs(1_000 * MONTH), 7).unwrap();
    assert_eq!((all, carry), (cents(4_000), 0));
    let huge = charge(
        &rules,
        Amount::from_cents(u64::MAX),
        secs(u64::MAX),
        u64::MAX,
    )
    .unwrap();
    assert_eq!(huge.0, cents(u64::MAX - 1_000));
}

#[test]
fn switching_demurrage_on_never_reaches_back() {
    let v1 = params();
    let v2 = with_demurrage(successor(&v1, ts(10 * MONTH)));
    let list = [v1, v2];
    let a = open_verified(1, 1, &list[0], ts(0));
    let plan = plan_demurrage(&a, Balance::from_cents(101_000), &list, ts(11 * MONTH)).unwrap();
    assert!(matches!(plan.ops[0], LedgerOp::Burn { amount, .. } if amount == cents(1_000)));
}

#[test]
fn with_demurrage_on_money_moves_only_on_settled_accounts() {
    let p = with_demurrage(params());
    let list = [p.clone()];
    let a = open_verified(1, 1, &p, ts(0));
    let b = open_verified(2, 1, &p, ts(0));
    let now = ts(HOUR);
    let rich = Balance::from_cents(5_000);
    assert_eq!(
        plan_claim(&a, &list, now),
        Err(CoreError::DemurrageNotSettled)
    );
    assert_eq!(
        plan_payment(&a, rich, &b, cents(1), &p, now),
        Err(CoreError::DemurrageNotSettled)
    );
    let a = plan_demurrage(&a, rich, &list, now).unwrap().state;
    assert_eq!(
        plan_payment(&a, rich, &b, cents(1), &p, now),
        Err(CoreError::DemurrageNotSettled)
    );
    let b = plan_demurrage(&b, Balance::ZERO, &list, now).unwrap().state;
    assert!(plan_payment(&a, rich, &b, cents(1), &p, now).is_ok());
    assert!(plan_claim(&a, &list, now).is_ok());
}

// ------------------------------------------------------------------ params

#[test]
fn the_sample_parameters_are_valid() {
    assert_eq!(params().validate(), Ok(()));
    assert_eq!(
        with_demurrage(with_country_caps(params())).validate(),
        Ok(())
    );
}

#[test]
fn structurally_broken_parameters_are_refused() {
    let broken: [fn(&mut Params); 9] = [
        |p| p.ladder.clear(),
        |p| p.ladder[1].id = TierId(0),
        |p| p.ladder.swap(0, 2),
        |p| p.claim_window = Duration::ZERO,
        |p| p.caps.period = Duration::ZERO,
        |p| {
            let cap = CountryCap {
                country: CountryCode(*b"DE"),
                gni_per_capita: cents(1),
                pct: Share::ONE,
            };
            p.caps.by_country = vec![cap, cap];
        },
        |p| {
            p.caps.by_country = vec![CountryCap {
                country: CountryCode(*b"de"),
                gni_per_capita: cents(1),
                pct: Share::ONE,
            }];
        },
        |p| {
            p.demurrage.rate = Share::ONE;
            p.demurrage.period = Duration::ZERO;
        },
        |p| {
            p.caps.ramp = secs(MONTH);
            p.caps.period = secs(1);
        },
    ];
    for (i, breaker) in broken.iter().enumerate() {
        let mut p = params();
        breaker(&mut p);
        assert!(
            matches!(p.validate(), Err(CoreError::InvalidParams(_))),
            "case {i}"
        );
    }
    let mut twice = params();
    twice.sponsor_keys = vec![AttrKey(1), AttrKey(1)];
    assert!(twice.validate().is_err());
}

#[test]
fn new_rules_need_a_higher_version_and_a_start_that_is_not_in_the_past() {
    let v1 = params();
    let now = ts(0);
    let good = successor(&v1, ts(10));
    assert_eq!(good.validate_successor(&v1, now), Ok(()));
    assert_eq!(successor(&v1, now).validate_successor(&v1, now), Ok(()));
    let mut same_version = good.clone();
    same_version.version = ParamsVersion(1);
    assert!(same_version.validate_successor(&v1, now).is_err());
    assert!(good.validate_successor(&v1, ts(11)).is_err()); // retroactive
    let mut invalid = good.clone();
    invalid.ladder.clear();
    assert!(invalid.validate_successor(&v1, now).is_err());

    let list = [v1.clone(), good.clone()];
    assert_eq!(check_sequence(&list), Ok(()));
    assert_eq!(in_force_at(&list, ts(9)).unwrap().version, ParamsVersion(1));
    assert_eq!(
        in_force_at(&list, ts(10)).unwrap().version,
        ParamsVersion(2)
    );
    assert_eq!(check_sequence(&[]), Err(CoreError::NoParamsInForce));
    assert!(check_sequence(&[good.clone(), v1]).is_err());
    assert_eq!(
        in_force_at(&[good], ts(9)).unwrap_err(),
        CoreError::NoParamsInForce
    );
}

#[test]
fn sponsors_may_target_published_attributes_only() {
    let mut p = params();
    p.sponsor_keys = vec![AttrKey(1), AttrKey(2)];
    let filter = AttributeFilter {
        all_of: vec![
            Criterion::Equals(AttrKey(1), AttrValue(49)),
            Criterion::Between(AttrKey(2), AttrValue(18), AttrValue(25)),
        ],
    };
    assert_eq!(filter.validate(&p), Ok(()));
    let hidden = AttributeFilter {
        all_of: vec![Criterion::Equals(AttrKey(3), AttrValue(1))],
    };
    assert_eq!(hidden.validate(&p), Err(CoreError::UnknownSponsorKey));
    let empty = AttributeFilter {
        all_of: vec![Criterion::OneOf(AttrKey(1), vec![])],
    };
    assert!(empty.validate(&p).is_err());
    let upside_down = AttributeFilter {
        all_of: vec![Criterion::Between(AttrKey(2), AttrValue(9), AttrValue(1))],
    };
    assert!(upside_down.validate(&p).is_err());

    let attr = |k, v| Attribute {
        key: AttrKey(k),
        value: AttrValue(v),
    };
    assert!(filter.matches(&[attr(1, 49), attr(2, 20)]));
    assert!(!filter.matches(&[attr(1, 49), attr(2, 30)]));
    assert!(!filter.matches(&[attr(1, 49)]));
    assert!(AttributeFilter::default().matches(&[]));
    let one_of = Criterion::OneOf(AttrKey(1), vec![AttrValue(1), AttrValue(2)]);
    assert!(one_of.matches(&[attr(1, 2)]));
}

// ------------------------------------------------------------------- types

#[test]
fn rates_compare_by_value_without_floats() {
    let per_minute = Rate::new(cents(1), Duration::MINUTE).unwrap();
    let per_hour = Rate::new(cents(60), Duration::HOUR).unwrap();
    assert!(per_minute.same_value(per_hour));
    assert!(Rate::ZERO.is_zero());
    assert_eq!(Rate::ZERO.min_value(per_hour), Rate::ZERO);
    assert!(Rate::new(cents(1), Duration::ZERO).is_err());
    let max = Rate::new(Amount::from_cents(u64::MAX), secs(1)).unwrap();
    assert!(max.accrued(secs(2)).is_err());
    assert_eq!(max.accrued(secs(1)).unwrap().cents(), u64::MAX);
    assert_eq!(ppm(500_000).of(cents(3)).unwrap(), cents(1));
}

#[test]
fn a_replayed_operation_changes_nothing() {
    let p = [params()];
    let a = open_verified(1, 1, &p[0], ts(0));
    let plan = plan_claim(&a, &p, ts(HOUR)).unwrap();
    let mut bank = Bank::default();
    assert_eq!(bank.apply(&plan.ops[0]), Ok(Applied::Done));
    assert_eq!(bank.apply(&plan.ops[0]), Ok(Applied::AlreadyApplied));
    assert_eq!(bank.balance_of(&id(1)), Balance::from_cents(60));
}

#[cfg(feature = "serde")]
#[test]
fn parameters_and_account_state_survive_json() {
    let p = with_demurrage(with_country_caps(params()));
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(serde_json::from_str::<Params>(&json).unwrap(), p);
    let a = resident(1, b"DE", &p, ts(0));
    let json = serde_json::to_string(&a).unwrap();
    assert_eq!(
        serde_json::from_str::<capita_core::account::AccountState>(&json).unwrap(),
        a
    );
}
