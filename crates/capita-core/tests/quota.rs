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

//! Worked examples for the free quota of transactions (rules diary 32).

#![allow(
    missing_docs,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use capita_core::quota::{day_of, Limits, Sender, Usage};
use capita_core::types::Timestamp;
use proptest::prelude::*;

const LIMITS: Limits = Limits {
    person_per_day: 100,
    new_key_per_day: 5,
    new_keys_per_block: 10,
};

#[test]
fn a_day_is_a_day_of_utc() {
    assert_eq!(day_of(Timestamp::from_secs(0)), 0);
    assert_eq!(day_of(Timestamp::from_secs(86_399)), 0);
    assert_eq!(day_of(Timestamp::from_secs(86_400)), 1);
    // 2026-11-16 00:00:00 UTC
    assert_eq!(day_of(Timestamp::from_secs(1_794_787_200)), 20_773);
    assert_eq!(day_of(Timestamp::from_secs(u64::MAX)), u32::MAX);
}

#[test]
fn a_person_has_more_than_a_key_without_one_and_a_service_is_not_counted() {
    assert_eq!(LIMITS.per_day(Sender::Person), Some(100));
    assert_eq!(LIMITS.per_day(Sender::NewKey), Some(5));
    assert_eq!(LIMITS.per_day(Sender::Service), None);
}

#[test]
fn the_quota_of_a_day_is_used_up_and_back_the_next_day() {
    let mut usage = Usage::default();
    for sent in 1..=5 {
        usage = usage.spend(20_773, 5).unwrap();
        assert_eq!(
            usage,
            Usage {
                day: 20_773,
                used: sent
            }
        );
        assert_eq!(usage.left(20_773, 5), 5 - sent);
    }
    assert_eq!(usage.spend(20_773, 5), None);
    assert_eq!(usage.left(20_773, 5), 0);

    // the next day: all of it again
    assert_eq!(usage.left(20_774, 5), 5);
    assert_eq!(
        usage.spend(20_774, 5),
        Some(Usage {
            day: 20_774,
            used: 1
        })
    );
}

#[test]
fn a_clock_that_goes_backwards_hands_out_nothing_twice() {
    let usage = Usage {
        day: 20_773,
        used: 5,
    };
    assert_eq!(usage.left(20_772, 5), 0);
    assert_eq!(usage.spend(20_772, 5), None);
    let some_left = Usage {
        day: 20_773,
        used: 4,
    };
    assert_eq!(
        some_left.spend(20_772, 5),
        Some(Usage {
            day: 20_773,
            used: 5
        })
    );
}

#[test]
fn a_limit_of_zero_lets_nothing_through() {
    assert_eq!(Usage::default().spend(20_773, 0), None);
    assert_eq!(Usage::default().left(20_773, 0), 0);
}

#[test]
fn a_lower_limit_takes_effect_at_once() {
    // 80 sent under a limit of 100; the limit drops to 50
    let usage = Usage {
        day: 20_773,
        used: 80,
    };
    assert_eq!(usage.left(20_773, 50), 0);
    assert_eq!(usage.spend(20_773, 50), None);
    assert_eq!(usage.left(20_774, 50), 50);
}

proptest! {
    /// Whatever the order of days: never more than `limit` on one day.
    #[test]
    fn never_more_than_the_limit_on_one_day(
        limit in 0u32..20,
        days in proptest::collection::vec(0u32..4, 0..200),
    ) {
        let mut usage = Usage::default();
        let mut sent = [0u32; 4];
        for day in days {
            if let Some(next) = usage.spend(day, limit) {
                // counted on the day the record holds, never an earlier one
                prop_assert!(next.day >= day);
                sent[next.day as usize] += 1;
                usage = next;
            }
        }
        for count in sent {
            prop_assert!(count <= limit);
        }
    }

    /// What is left and what can be spent agree.
    #[test]
    fn left_says_whether_spending_works(
        day in 0u32..5, used in 0u32..30, today in 0u32..5, limit in 0u32..30,
    ) {
        let usage = Usage { day, used };
        prop_assert_eq!(usage.left(today, limit) > 0, usage.spend(today, limit).is_some());
    }
}
