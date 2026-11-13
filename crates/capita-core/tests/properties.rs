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

//! Property tests: the invariants of the rules, checked on random inputs.

#![allow(
    missing_docs,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

mod common;

use capita_core::account::{AccountState, AccountStatus};
use capita_core::demurrage::{charge, plan_demurrage};
use capita_core::errors::CoreError;
use capita_core::issuance::{cap_at, effective_rate, plan_claim, plan_purchase};
use capita_core::lifecycle::{plan_attest, plan_set_status, plan_set_tier};
use capita_core::loans::{
    plan_lend, plan_payment, plan_repay, plan_set_credit_line, plan_spend_start_grant,
};
use capita_core::params::{CountryCap, DemurrageParams, Params};
use capita_core::plan::Plan;
use capita_core::ports::{Event, Ledger, LedgerOp, NoGate, TransferPurpose};
use capita_core::types::{Amount, Balance, CountryCode, Duration, Rate, TierId, Timestamp};
use proptest::prelude::*;

use common::*;

fn issued(plan: &Plan) -> u64 {
    plan.events
        .iter()
        .map(|e| match e {
            Event::Claimed { issued, .. } => issued.cents(),
            _ => 0,
        })
        .sum()
}

/// Parameters with a random rate on step 1 and a random global cap.
fn random_rules(amount: u64, per: u64, cap: u64) -> Params {
    let mut p = params();
    p.ladder[1] = step(1, 1, amount, per);
    p.caps.global_cap = cents(cap);
    p
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// claim ≤ rate × elapsed, claim ≤ cap, and nothing beyond the window.
    #[test]
    fn a_claim_never_exceeds_rate_times_time_nor_the_cap(
        amount in 0u64..10_000,
        per in 1u64..100_000,
        cap in 0u64..10_000_000,
        elapsed in 0u64..60 * DAY,
    ) {
        let p = random_rules(amount, per, cap);
        let a = open_verified(1, 1, &p, ts(0));
        let plan = plan_claim(&a, core::slice::from_ref(&p), ts(elapsed)).unwrap();
        let counted = secs(elapsed.min(14 * DAY));
        let by_rate = p.ladder[1].rate.accrued(counted).unwrap();
        let by_cap = Rate::new(cents(cap), Duration::MONTH).unwrap().accrued(counted).unwrap();
        prop_assert!(issued(&plan) <= by_rate.cents());
        prop_assert!(issued(&plan) <= by_cap.cents());
        prop_assert_eq!(issued(&plan), by_rate.min(by_cap).cents());
    }

    /// Claiming often pays exactly what claiming once pays – also across a
    /// change of rules, an expiring verification and the seniority ramp.
    #[test]
    fn partial_claims_add_up_to_one_claim(
        amount in 0u64..10_000,
        per in 1u64..100_000,
        new_amount in 0u64..10_000,
        change_at in 1u64..14 * DAY,
        expires_at in proptest::option::of(1u64..14 * DAY),
        ramp_days in 0u64..10,
        mut times in proptest::collection::vec(0u64..=14 * DAY, 1..8),
    ) {
        times.sort_unstable();
        let mut v1 = random_rules(amount, per, 2_000);
        v1.caps.period = secs(DAY);
        v1.caps.ramp = secs(ramp_days * DAY);
        v1.caps.by_country = vec![CountryCap {
            country: CountryCode(*b"DE"),
            gni_per_capita: cents(100_000_000),
            pct: ppm(100_000),
        }];
        let mut v2 = successor(&v1, ts(change_at));
        v2.ladder[1] = step(1, 1, new_amount, per);
        let list = [v1, v2];
        let a = open_verified(1, 1, &list[0], ts(0));
        let att = attestation(1, 1, Some(b"DE"), ts(0), expires_at.map(ts));
        let a = plan_attest(&a, &list, Some(&att), None, ts(0)).unwrap().state;

        let end = *times.last().unwrap();
        let once = issued(&plan_claim(&a, &list, ts(end)).unwrap());
        let mut state = a;
        let mut sum = 0;
        for t in times {
            let plan = plan_claim(&state, &list, ts(t)).unwrap();
            sum += issued(&plan);
            state = plan.state;
        }
        prop_assert_eq!(sum, once);
    }

    /// The effective rate is never above the step rate nor above the cap.
    #[test]
    fn the_effective_rate_is_the_lower_of_step_and_cap(
        amount in 0u64..u64::MAX,
        per in 1u64..u64::MAX,
        cap in 0u64..u64::MAX,
    ) {
        let p = random_rules(amount, per, cap);
        let a = open_verified(1, 1, &p, ts(0));
        let rate = effective_rate(&p, &a, ts(0)).unwrap();
        let cap_rate = Rate::new(cap_at(&p, &a, ts(0)).unwrap(), p.caps.period).unwrap();
        prop_assert!(rate.cmp_value(p.ladder[1].rate) != core::cmp::Ordering::Greater);
        prop_assert!(rate.cmp_value(cap_rate) != core::cmp::Ordering::Greater);
    }

    /// Extreme values give a result or an error – never a panic, never a
    /// wrapped number.
    #[test]
    fn extreme_values_never_panic(
        amount in any::<u64>(),
        per in 1u64..=u64::MAX,
        cap in any::<u64>(),
        gni in any::<u64>(),
        ramp in any::<u64>(),
        window in 1u64..=u64::MAX,
        ripening in any::<u64>(),
        opened in any::<u64>(),
        now in any::<u64>(),
        balance in any::<i64>(),
        line in any::<u64>(),
        pay in any::<u64>(),
        dem_rate in 0u32..=1_000_000,
        dem_period in 1u64..=u64::MAX,
        allowance in any::<u64>(),
        outstanding in any::<u64>(),
    ) {
        let mut p = random_rules(amount, per, cap);
        p.claim_window = secs(window);
        p.ripening = secs(ripening);
        p.caps.ramp = secs(ramp);
        p.caps.by_country = vec![CountryCap {
            country: CountryCode(*b"DE"),
            gni_per_capita: cents(gni),
            pct: ppm(1_000_000),
        }];
        p.demurrage = DemurrageParams {
            rate: ppm(dem_rate),
            period: secs(dem_period),
            free_allowance: cents(allowance),
        };
        p.loans.max_credit_line = cents(u64::MAX);
        p.loans.max_outstanding = cents(u64::MAX);
        let opened = Timestamp::from_secs(opened);
        let now = Timestamp::from_secs(now);
        let list = [p.clone()];
        let mut a = open_verified(1, 1, &p, opened);
        let att = attestation(1, 1, Some(b"DE"), opened, None);
        a = plan_attest(&a, &list, Some(&att), None, opened).unwrap().state;
        a.loan.credit_line = cents(line);
        a.loan.outstanding = cents(outstanding);
        a.loan.lender = Some(id(2));
        let b = open_verified(2, 1, &p, opened);
        let balance = Balance::from_cents(balance);

        let claim = plan_claim(&a, &list, now);
        if let Ok(plan) = &claim {
            for e in &plan.events {
                if let Event::Claimed { issued, repaid, .. } = e {
                    prop_assert!(repaid <= issued);
                    prop_assert!(*repaid <= cents(outstanding));
                }
            }
        }
        let _ = plan_demurrage(&a, balance, &list, now);
        let _ = plan_payment(&a, balance, &b, cents(pay), &p, now);
        let _ = plan_lend(&b, &a, balance, cents(pay), &p, now);
        let _ = plan_repay(&a, balance, cents(pay), &p, now);
        let _ = plan_set_credit_line(&a, balance, cents(pay), &p, now);
        let _ = plan_purchase(&a, &p, cents(pay), now);
        let _ = plan_set_tier(&a, &list, &NoGate, TierId(0), now);
        let _ = plan_set_status(&a, balance, &list, AccountStatus::Closed, now);
        let _ = plan_attest(&a, &list, None, None, now);
        let _ = cap_at(&p, &a, now);
        let _ = p.validate();
    }

    /// Time may never go backwards.
    #[test]
    fn an_earlier_time_is_always_refused(seen in 1u64..u64::MAX, back in 1u64..1_000_000) {
        let p = params();
        let mut a = open_verified(1, 1, &p, Timestamp::from_secs(0));
        a.last_seen = Timestamp::from_secs(seen);
        let now = Timestamp::from_secs(seen.saturating_sub(back));
        prop_assume!(now < a.last_seen);
        let refused = matches!(
            plan_claim(&a, &[p], now),
            Err(CoreError::ClockWentBackwards { .. })
        );
        prop_assert!(refused);
    }

    /// Demurrage never charges more than the balance above the allowance,
    /// and settling in several steps never charges more than settling once.
    #[test]
    fn demurrage_is_bounded(
        balance in 0u64..=u64::MAX / 2,
        allowance in 0u64..1_000_000,
        rate in 1u32..=1_000_000,
        period in 1u64..10 * MONTH,
        spans in proptest::collection::vec(0u64..3 * MONTH, 1..6),
    ) {
        let rules = DemurrageParams {
            rate: ppm(rate),
            period: secs(period),
            free_allowance: cents(allowance),
        };
        let chargeable = balance.saturating_sub(allowance);
        let whole: u64 = spans.iter().sum();
        let (once, _) = charge(&rules, cents(balance), secs(whole), 0).unwrap();
        prop_assert!(once.cents() <= chargeable);
        let mut left = balance;
        let mut carry = 0;
        for span in spans {
            let (part, rest) = charge(&rules, cents(left), secs(span), carry).unwrap();
            prop_assert!(part.cents() <= left.saturating_sub(allowance));
            left -= part.cents();
            carry = rest;
        }
        prop_assert!(balance - left <= once.cents());
    }
}

// ------------------------------------------------- random histories

#[derive(Clone, Debug)]
enum Action {
    Claim(u8),
    Pay(u8, u8, u64),
    Lend(u8, u8, u64),
    Repay(u8, u64),
    Line(u8, u64),
    Purchase(u8, u64),
    Tier(u8, u16),
    Status(u8, u8),
    Verify(u8, u8),
    SpendGrant(u8, u8),
}

fn action() -> impl Strategy<Value = Action> {
    let who = 1u8..=3;
    prop_oneof![
        4 => who.clone().prop_map(Action::Claim),
        3 => (who.clone(), 1u8..=3, 0u64..3_000).prop_map(|(a, b, c)| Action::Pay(a, b, c)),
        2 => (who.clone(), 1u8..=3, 0u64..3_000).prop_map(|(a, b, c)| Action::Lend(a, b, c)),
        2 => (who.clone(), 0u64..2_000).prop_map(|(a, c)| Action::Repay(a, c)),
        2 => (who.clone(), 0u64..3_000).prop_map(|(a, c)| Action::Line(a, c)),
        1 => (who.clone(), 1u64..5_000).prop_map(|(a, c)| Action::Purchase(a, c)),
        1 => (who.clone(), 0u16..4).prop_map(|(a, t)| Action::Tier(a, t)),
        1 => (who.clone(), 0u8..3).prop_map(|(a, s)| Action::Status(a, s)),
        1 => (who.clone(), 0u8..3).prop_map(|(a, l)| Action::Verify(a, l)),
        1 => (who, 1u8..=3).prop_map(|(a, b)| Action::SpendGrant(a, b)),
    ]
}

/// Settles demurrage the way a host does before money moves.
fn settle(world: &mut World, n: u8, list: &[Params], now: Timestamp) {
    if world.state(n).status == AccountStatus::Closed {
        return;
    }
    let plan = plan_demurrage(world.state(n), world.balance(n), list, now).unwrap();
    world.apply(plan);
}

/// Applies the plan if there is one. Returns what each account received as
/// loan repayment.
fn try_apply(world: &mut World, plan: Result<Plan, CoreError>) -> Result<[i64; 3], TestCaseError> {
    let mut repaid_to = [0i64; 3];
    if let Ok(plan) = plan {
        for e in &plan.events {
            if let Event::Claimed { issued, repaid, .. } = e {
                prop_assert!(repaid <= issued, "repayment above issuance");
            }
        }
        for op in &plan.ops {
            if let LedgerOp::Transfer {
                to,
                amount,
                purpose: TransferPurpose::LoanRepayment,
                ..
            } = op
            {
                repaid_to[usize::from(to.0[0] - 1)] += i64::try_from(amount.cents()).unwrap();
            }
        }
        world.apply(plan);
    }
    Ok(repaid_to)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Whatever people do, in whatever order:
    /// - no balance falls below minus its credit line (the ledger never has
    ///   to refuse what the core planned);
    /// - the supply equals minted minus burnt, on the ledger and in the
    ///   public totals;
    /// - recorded debt equals lent minus repaid;
    /// - frozen and closed accounts receive no issuance and no payments;
    /// - a start grant not yet spent on verification is never touched.
    #[test]
    fn random_histories_keep_every_invariant(
        demurrage in proptest::option::of(1u32..200_000),
        share in 0u32..=1_000_000,
        ripening in 0u64..3 * DAY,
        steps in proptest::collection::vec((action(), 0u64..3 * DAY), 1..60),
    ) {
        let mut p = params();
        p.loans.repayment_share = ppm(share);
        p.loans.max_credit_line = cents(2_000);
        p.loans.max_outstanding = cents(5_000);
        p.ripening = secs(ripening);
        if let Some(rate) = demurrage {
            p.demurrage.rate = ppm(rate);
            p.demurrage.free_allowance = cents(100);
        }
        let list = [p.clone()];
        let mut world = World::default();
        for n in 1..=3u8 {
            world.apply(capita_core::lifecycle::plan_open(id(n), &p, ts(0)).unwrap());
        }
        let mut clock = 0;
        for (action, wait) in steps {
            clock += wait;
            let now = ts(clock);
            for n in 1..=3 {
                settle(&mut world, n, &list, now);
            }
            let before: Vec<i64> = (1..=3).map(|n| world.balance(n).cents()).collect();
            let frozen: Vec<bool> = (1..=3)
                .map(|n| world.state(n).status != AccountStatus::Active)
                .collect();
            let w = &world;
            let plan = match action {
                Action::Claim(a) => plan_claim(w.state(a), &list, now),
                Action::Pay(a, b, c) => {
                    plan_payment(w.state(a), w.balance(a), w.state(b), cents(c), &p, now)
                }
                Action::Lend(a, b, c) => {
                    plan_lend(w.state(a), w.state(b), w.balance(b), cents(c), &p, now)
                }
                Action::Repay(a, c) => plan_repay(w.state(a), w.balance(a), cents(c), &p, now),
                Action::Line(a, c) => {
                    plan_set_credit_line(w.state(a), w.balance(a), cents(c), &p, now)
                }
                Action::Purchase(a, c) => plan_purchase(w.state(a), &p, cents(c), now),
                Action::Tier(a, t) => plan_set_tier(w.state(a), &list, &NoGate, TierId(t), now),
                Action::Status(a, s) => {
                    let to = [
                        AccountStatus::Active,
                        AccountStatus::Frozen,
                        AccountStatus::Closed,
                    ][usize::from(s)];
                    plan_set_status(w.state(a), w.balance(a), &list, to, now)
                }
                Action::SpendGrant(a, b) => {
                    plan_spend_start_grant(w.state(a), w.balance(a), w.state(b), &p, now)
                }
                Action::Verify(a, l) => {
                    let att = attestation(a, l, None, now, None);
                    plan_attest(w.state(a), &list, Some(&att), None, now)
                }
            };
            let repaid_to = try_apply(&mut world, plan)?;

            prop_assert!(world.bank.lines_respected());
            let supply = world.bank.total_supply().unwrap();
            prop_assert_eq!(world.totals.supply().unwrap(), supply);
            prop_assert_eq!(world.bank.sum_of_balances(), i128::try_from(supply).unwrap());
            let debt: u128 = world
                .accounts
                .values()
                .map(|s| u128::from(s.loan.outstanding.cents()))
                .sum();
            prop_assert_eq!(world.totals.outstanding().unwrap(), debt);
            for s in world.accounts.values() {
                prop_assert_eq!(s.loan.outstanding.is_zero(), s.loan.lender.is_none());
                prop_assert_eq!(
                    world.bank.lines.get(&s.id).copied().unwrap_or(0),
                    s.loan.credit_line.cents()
                );
                // An unspent start grant is still there.
                let free = i128::from(world.bank.balance_of(&s.id).cents())
                    + i128::from(s.loan.credit_line.cents());
                prop_assert!(free >= i128::from(s.start_grant_reserved.cents()));
            }
            for n in 1..=3u8 {
                let i = usize::from(n - 1);
                if frozen[i] {
                    // Only the repayment of a loan it gave still reaches it.
                    let gained = world.balance(n).cents() - before[i];
                    prop_assert!(gained <= repaid_to[i], "frozen account gained");
                }
            }
        }
    }
}

/// A state as a host might hand it in: must be the type the functions take.
#[test]
fn account_state_is_plain_data() {
    fn assert_plain<T: Clone + PartialEq + core::fmt::Debug + Send + Sync>() {}
    assert_plain::<AccountState>();
    assert_plain::<Params>();
    assert_plain::<Plan>();
    let _ = Amount::ZERO;
}
