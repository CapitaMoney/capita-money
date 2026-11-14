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

//! Worked examples for rebinding a person and for moving an account to a
//! new key.

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
use capita_core::attributes::VerificationLevel;
use capita_core::errors::CoreError;
use capita_core::issuance::plan_claim;
use capita_core::lifecycle::{plan_attest, plan_open, plan_set_status};
use capita_core::loans::plan_set_credit_line;
use capita_core::ports::{Event, Ledger};
use capita_core::recovery::{plan_move, MovePlan};
use capita_core::types::{Balance, PersonId, TierId};

use common::*;

fn apply_move(world: &mut World, plan: MovePlan) {
    for op in &plan.ops {
        world.bank.apply(op).unwrap();
    }
    for event in &plan.events {
        world.totals.record(event).unwrap();
    }
    world.accounts.insert(plan.from.id, plan.from);
    world.accounts.insert(plan.to.id, plan.to);
}

/// Person 1 on account 1: verified at level 1, on step 1 (1 cent a minute),
/// start grant spent, 500 cents bought. Account 2 opened later with a new
/// key.
fn world() -> (World, [capita_core::params::Params; 1]) {
    let p = params();
    let mut world = World::default();
    world.accounts.insert(id(1), open_verified(1, 1, &p, ts(0)));
    let buy = capita_core::issuance::plan_purchase(world.state(1), &p, cents(500), ts(0)).unwrap();
    world.apply(buy);
    world.apply(plan_open(id(2), &p, ts(10 * DAY)).unwrap());
    (world, [p])
}

#[test]
fn a_person_known_under_a_new_identifier_keeps_the_account() {
    let (world, list) = world();
    let mut att = attestation(7, 1, None, ts(DAY), None);
    // Another person: refused as before.
    assert_eq!(
        plan_attest(world.state(1), &list, Some(&att), None, ts(DAY)),
        Err(CoreError::PersonMismatch)
    );
    // Stated to replace another identifier than the one the account has.
    att.replaces = Some(PersonId([3; 32]));
    assert_eq!(
        plan_attest(world.state(1), &list, Some(&att), None, ts(DAY)),
        Err(CoreError::PersonMismatch)
    );
    // Stated to replace the identifier of the account: rebound.
    att.replaces = Some(PersonId([1; 32]));
    let plan = plan_attest(world.state(1), &list, Some(&att), None, ts(DAY)).unwrap();
    assert_eq!(plan.state.person, Some(PersonId([7; 32])));
    // The new identifier already holds another account: refused.
    assert_eq!(
        plan_attest(world.state(1), &list, Some(&att), Some(id(5)), ts(DAY)),
        Err(CoreError::DuplicatePerson)
    );
}

#[test]
fn a_lost_key_moves_person_money_and_entitlement_to_the_new_account() {
    let (mut world, list) = world();
    let supply = world.bank.total_supply().unwrap();
    let now = ts(10 * DAY + HOUR);
    let att = attestation(1, 1, None, now, Some(ts(375 * DAY)));
    let plan = plan_move(
        world.state(1),
        world.balance(1),
        world.state(2),
        &list,
        &att,
        now,
    );
    let plan = plan.unwrap();
    assert!(plan
        .events
        .iter()
        .any(|e| matches!(e, Event::AccountMoved { .. })));
    apply_move(&mut world, plan);

    // The old account: closed, empty, nobody's.
    assert_eq!(world.state(1).status, AccountStatus::Closed);
    assert_eq!(world.state(1).person, None);
    assert_eq!(world.state(1).level, VerificationLevel(0));
    assert_eq!(world.balance(1), Balance::ZERO);
    assert!(world.state(1).issuance.carried.is_zero());
    // The new one: the person, the money (500 bought + its own start
    // grant), the step and the seniority.
    assert_eq!(world.state(2).person, Some(PersonId([1; 32])));
    assert_eq!(world.state(2).level, VerificationLevel(1));
    assert_eq!(world.state(2).level_until, Some(ts(375 * DAY)));
    assert_eq!(world.state(2).tier, TierId(1));
    assert_eq!(world.state(2).opened_at, ts(0));
    assert_eq!(world.balance(2), Balance::from_cents(600));
    assert_eq!(world.state(2).start_grant_reserved, cents(100));
    // No money was made or lost, and a move is no payment.
    assert_eq!(world.bank.total_supply().unwrap(), supply);
    assert_eq!(world.totals.paid, 0);
    // What accrued under the old key (10 days and an hour at 1 cent a
    // minute, never claimed) is claimed with the new one – together with
    // the one cent the new account accrued in its hour on step 0 – and
    // accrual goes on from there.
    let claim = plan_claim(world.state(2), &list, now).unwrap();
    world.apply(claim);
    let accrued = (10 * DAY + HOUR) / MINUTE + 1;
    assert_eq!(
        world.balance(2),
        Balance::from_cents(600 + i64::try_from(accrued).unwrap())
    );
    let later = plan_claim(world.state(2), &list, ts(10 * DAY + 2 * HOUR)).unwrap();
    world.apply(later);
    assert_eq!(
        world.balance(2),
        Balance::from_cents(600 + i64::try_from(accrued).unwrap() + 60)
    );
    // The closed account stays closed.
    assert_eq!(
        plan_claim(world.state(1), &list, ts(11 * DAY)),
        Err(CoreError::AccountNotActive(AccountStatus::Closed))
    );
}

#[test]
fn a_move_is_refused_where_it_would_take_or_hide_something() {
    let (mut world, list) = world();
    let now = ts(10 * DAY + HOUR);
    let att = attestation(1, 1, None, now, None);
    let go = |w: &World, a: u8, b: u8, att| {
        plan_move(w.state(a), w.balance(a), w.state(b), &list, att, now).map(|_| ())
    };
    // Another person than the one the old account belongs to.
    let other = attestation(4, 1, None, now, None);
    assert_eq!(go(&world, 1, 2, &other), Err(CoreError::NotAccountOfPerson));
    // An account onto itself; from an account that belongs to nobody.
    assert_eq!(go(&world, 1, 1, &att), Err(CoreError::SameAccount));
    assert_eq!(go(&world, 2, 1, &att), Err(CoreError::MoveTargetBound));
    // A statement that is not valid (yet, any more).
    let early = attestation(1, 1, None, ts(11 * DAY), None);
    assert_eq!(
        go(&world, 1, 2, &early),
        Err(CoreError::AttestationNotValid)
    );
    let late = attestation(1, 1, None, ts(0), Some(now));
    assert_eq!(go(&world, 1, 2, &late), Err(CoreError::AttestationNotValid));
    // Onto the account of somebody else.
    world
        .accounts
        .insert(id(3), open_verified(3, 1, &list[0], ts(0)));
    assert_eq!(go(&world, 1, 3, &att), Err(CoreError::MoveTargetBound));
    // With a credit line.
    let line = plan_set_credit_line(
        world.state(1),
        world.balance(1),
        cents(1_000),
        &list[0],
        now,
    )
    .unwrap();
    let mut with_line = World {
        accounts: world.accounts.clone(),
        ..World::default()
    };
    with_line.apply(line);
    assert_eq!(go(&with_line, 1, 2, &att), Err(CoreError::MoveWithDebt));
    // A frozen account does not move; nor onto a closed one.
    let frozen = plan_set_status(
        world.state(1),
        world.balance(1),
        &list,
        AccountStatus::Frozen,
        now,
    );
    let mut with_frozen = World {
        accounts: world.accounts.clone(),
        ..World::default()
    };
    with_frozen.apply(frozen.unwrap());
    assert_eq!(
        go(&with_frozen, 1, 2, &att),
        Err(CoreError::AccountNotActive(AccountStatus::Frozen))
    );
    // The old name of the person, replaced by a new one, moves as well.
    let mut renamed = attestation(8, 1, None, now, None);
    renamed.replaces = Some(PersonId([1; 32]));
    assert_eq!(go(&world, 1, 2, &renamed), Ok(()));
}
