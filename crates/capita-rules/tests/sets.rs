// Capita Money – capita-rules
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

//! Every rule set of the repository runs through the core: whatever the
//! numbers of a rules file are, the rules hold. No number is expected here,
//! so a change of rules never breaks these tests – only rules that do not
//! work do.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use capita_core::attributes::{Attestation, VerificationLevel};
use capita_core::issuance::plan_claim;
use capita_core::lifecycle::{plan_attest, plan_open, plan_set_tier};
use capita_core::params::Params;
use capita_core::plan::Plan;
use capita_core::ports::{LedgerOp, NoGate};
use capita_core::types::{AccountId, Duration, PersonId, Timestamp};

const T0: u64 = 1_800_000_000;

fn sets() -> Vec<(&'static str, Params)> {
    capita_rules::SETS
        .iter()
        .map(|name| {
            (
                *name,
                capita_rules::params_of(name).expect("checked by the build"),
            )
        })
        .collect()
}

fn minted(plan: &Plan) -> u64 {
    plan.ops
        .iter()
        .map(|op| match op {
            LedgerOp::Mint { amount, .. } => amount.cents(),
            _ => 0,
        })
        .sum()
}

#[test]
fn the_sets_are_those_of_the_rules_folder() {
    assert_eq!(capita_rules::SETS, ["alpha", "dev-fast"]);
    assert_eq!(capita_rules::alpha::NAME, "alpha");
    assert_eq!(capita_rules::dev_fast::NAME, "dev-fast");
    assert!(capita_rules::params_of("no such set").is_none());
    assert_eq!(
        capita_rules::alpha::params(),
        capita_rules::params_of("alpha")
    );
}

#[test]
fn every_set_passes_the_validation_of_the_core() {
    for (name, params) in sets() {
        params.validate().unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(params.valid_from, Timestamp::from_secs(0), "{name}");
    }
    assert!(!capita_rules::alpha::MEMBERSHIP_PERIOD.is_zero());
    assert!(!capita_rules::dev_fast::MEMBERSHIP_PERIOD.is_zero());
}

/// A person opens an account, is verified at the highest level of the
/// ladder, moves up step by step and claims after each cap period.
#[test]
fn a_person_goes_from_opening_to_claims_under_every_set() {
    for (name, params) in sets() {
        let list = [params.clone()];
        let t0 = Timestamp::from_secs(T0);
        let opened = plan_open(AccountId([1; 32]), &params, t0).unwrap();
        // the start grant, and nothing else, exists from the first moment
        assert_eq!(minted(&opened), params.start_grant.cents(), "{name}");

        let top = params.ladder.iter().map(|s| s.min_level).max().unwrap();
        let attestation = Attestation {
            person: PersonId([1; 32]),
            level: top,
            residence: None,
            attributes: vec![],
            issued_at: t0,
            expires_at: None,
        };
        let mut state = plan_attest(&opened.state, &list, Some(&attestation), None, t0)
            .unwrap()
            .state;

        let period = params.caps.period;
        let mut now = t0;
        for step in &params.ladder {
            state = plan_set_tier(&state, &list, &NoGate, step.id, now)
                .unwrap_or_else(|e| panic!("{name}, step {}: {e}", step.id.0))
                .state;
            now = now.checked_add(period).unwrap();
            let claim = plan_claim(&state, &list, now).unwrap();
            let issued = minted(&claim);
            // never more than the rate of the step over the time that
            // counts, and never more than the cap of the period
            let counted = Duration::from_secs(period.secs().min(params.claim_window.secs()));
            let at_most = step.rate.accrued(counted).unwrap().cents();
            assert!(
                issued <= at_most,
                "{name}, step {}: {issued} > {at_most}",
                step.id.0
            );
            assert!(issued <= params.caps.global_cap.cents(), "{name}");
            // a rate of nothing issues nothing; a rate of something that
            // amounts to a cent or more within the time issues something
            assert_eq!(issued == 0, at_most == 0, "{name}, step {}", step.id.0);
            // claiming again at the same moment gives nothing
            let again = plan_claim(&claim.state, &list, now).unwrap();
            assert_eq!(minted(&again), 0, "{name}");
            state = claim.state;
        }
    }
}

/// Without a verification an account stands on the first step, whatever
/// that step issues, and cannot move up.
#[test]
fn an_unverified_account_stays_on_the_first_step_under_every_set() {
    for (name, params) in sets() {
        let list = [params.clone()];
        let t0 = Timestamp::from_secs(T0);
        let state = plan_open(AccountId([2; 32]), &params, t0).unwrap().state;
        let first = params.ladder.first().unwrap();
        assert_eq!(state.tier, first.id, "{name}");
        assert_eq!(first.min_level, VerificationLevel(0), "{name}");
        for step in params
            .ladder
            .iter()
            .filter(|s| s.min_level > VerificationLevel(0))
        {
            assert!(
                plan_set_tier(&state, &list, &NoGate, step.id, t0).is_err(),
                "{name}: step {} without verification",
                step.id.0
            );
        }
    }
}
