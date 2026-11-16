// Capita Money – capita-core
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Issuance by age (ADR 0026): the calendar, the amount at the first
//! verification, the birthdays.

#![allow(
    missing_docs,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use capita_core::account::{AccountState, AccountStatus};
use capita_core::attributes::Attestation;
use capita_core::errors::CoreError;
use capita_core::issuance::{birthdays_due, plan_claim};
use capita_core::lifecycle::{plan_attest, plan_set_status};
use capita_core::params::{AgeParams, CountryAge, Params};
use capita_core::plan::Plan;
use capita_core::ports::{Event, LedgerOp};
use capita_core::recovery::plan_move;
use capita_core::types::{Amount, Balance, CountryCode, Date, Duration, Timestamp};
use common::{attestation, cents, id, open, params, secs, successor, DAY};
use proptest::prelude::*;

const YEAR: u64 = 365 * DAY;

fn date(year: u16, month: u8, day: u8) -> Date {
    Date::new(year, month, day).unwrap()
}

/// 00:00 UTC of a day.
fn at(year: u16, month: u8, day: u8) -> Timestamp {
    date(year, month, day).start().unwrap()
}

fn plus(t: Timestamp, s: u64) -> Timestamp {
    t.checked_add(secs(s)).unwrap()
}

/// Rules by age only: 2.00 per year of age on each birthday, 1.00 per year
/// of age at the first verification, no ladder amount, no start grant.
fn rules() -> Params {
    let mut p = params();
    p.start_grant = Amount::ZERO;
    for step in &mut p.ladder {
        step.rate = capita_core::types::Rate::new(Amount::ZERO, Duration::MONTH).unwrap();
    }
    p.age = AgeParams {
        per_year: cents(200),
        at_verification: cents(100),
        ..AgeParams::OFF
    };
    p
}

fn born(mut att: Attestation, day: Date) -> Attestation {
    att.born = Some(day);
    att
}

/// What a plan mints.
fn minted(plan: &Plan) -> u64 {
    plan.ops
        .iter()
        .map(|op| match op {
            LedgerOp::Mint { amount, .. } => amount.cents(),
            _ => 0,
        })
        .sum()
}

/// An account opened and verified at `when` for a person born on `day`,
/// the verification holding for `valid` seconds; with what the claim right
/// after the verification mints.
fn verified(p: &Params, day: Date, when: Timestamp, valid: u64) -> (AccountState, u64) {
    let list = [p.clone()];
    let state = open(1, p, when);
    let att = born(attestation(1, 1, None, when, Some(plus(when, valid))), day);
    let state = plan_attest(&state, &list, Some(&att), None, when)
        .unwrap()
        .state;
    let plan = plan_claim(&state, &list, when).unwrap();
    let paid = minted(&plan);
    (plan.state, paid)
}

fn claim(state: &AccountState, p: &Params, now: Timestamp) -> (AccountState, u64) {
    let plan = plan_claim(state, std::slice::from_ref(p), now).unwrap();
    let paid = minted(&plan);
    (plan.state, paid)
}

// ------------------------------------------------------------- the calendar

#[test]
fn a_date_knows_its_first_second() {
    assert_eq!(date(1970, 1, 1).start(), Some(Timestamp::from_secs(0)));
    assert_eq!(
        date(2000, 2, 29).start(),
        Some(Timestamp::from_secs(951_782_400))
    );
    assert_eq!(
        date(2026, 11, 15).start(),
        Some(Timestamp::from_secs(1_794_700_800))
    );
    assert_eq!(
        date(9999, 12, 31).start(),
        Some(Timestamp::from_secs(253_402_214_400))
    );
    // Before 1970 no timestamp exists.
    assert_eq!(date(1969, 12, 31).start(), None);
    assert_eq!(date(1, 1, 1).start(), None);
}

#[test]
fn a_moment_knows_its_day() {
    let day = |secs| Date::of(Timestamp::from_secs(secs)).unwrap();
    assert_eq!(day(0), date(1970, 1, 1));
    assert_eq!(day(86_399), date(1970, 1, 1));
    assert_eq!(day(86_400), date(1970, 1, 2));
    assert_eq!(day(951_782_400), date(2000, 2, 29));
    assert_eq!(day(951_782_400 + 86_400), date(2000, 3, 1));
    assert_eq!(day(1_800_000_000), date(2027, 1, 15));
    assert_eq!(day(253_402_300_799), date(9999, 12, 31));
    assert!(Date::of(Timestamp::from_secs(253_402_300_800)).is_err());
    assert!(Date::of(Timestamp::from_secs(u64::MAX)).is_err());
}

#[test]
fn only_days_of_the_calendar_exist() {
    for (y, m, d) in [
        (0, 1, 1),
        (10_000, 1, 1),
        (2026, 0, 1),
        (2026, 13, 1),
        (2026, 1, 0),
        (2026, 4, 31),
        (2026, 2, 29),
        (1900, 2, 29),
    ] {
        assert!(Date::new(y, m, d).is_err(), "{y}-{m}-{d}");
    }
    for (y, m, d) in [(2028, 2, 29), (2000, 2, 29), (1, 1, 1), (9999, 12, 31)] {
        assert!(Date::new(y, m, d).is_ok(), "{y}-{m}-{d}");
    }
}

#[test]
fn age_counts_completed_years() {
    let b = date(1990, 5, 17);
    assert_eq!(b.age_on(date(1990, 5, 17)), 0);
    assert_eq!(b.age_on(date(1991, 5, 16)), 0);
    assert_eq!(b.age_on(date(1991, 5, 17)), 1);
    assert_eq!(b.age_on(date(2026, 11, 15)), 36);
    assert_eq!(b.age_on(date(2027, 5, 16)), 36);
    assert_eq!(b.age_on(date(2027, 5, 17)), 37);
    // Earlier than the birth: zero, not negative.
    assert_eq!(b.age_on(date(1980, 1, 1)), 0);
}

#[test]
fn born_on_29_february_the_birthday_is_1_march_in_other_years() {
    let b = date(1996, 2, 29);
    assert_eq!(b.birthday(4), Some(date(2000, 2, 29)));
    assert_eq!(b.birthday(31), Some(date(2027, 3, 1)));
    assert_eq!(b.birthday(32), Some(date(2028, 2, 29)));
    assert_eq!(b.age_on(date(2027, 2, 28)), 30);
    assert_eq!(b.age_on(date(2027, 3, 1)), 31);
    assert_eq!(b.age_on(date(2028, 2, 28)), 31);
    assert_eq!(b.age_on(date(2028, 2, 29)), 32);
    // No birthday after the year 9999.
    assert_eq!(date(9990, 1, 1).birthday(10), None);
}

#[cfg(feature = "serde")]
#[test]
fn json_refuses_a_day_the_calendar_does_not_have() {
    let day: Date = serde_json::from_str(r#"{"year":1990,"month":5,"day":17}"#).unwrap();
    assert_eq!(day, date(1990, 5, 17));
    assert_eq!(
        serde_json::to_string(&day).unwrap(),
        r#"{"year":1990,"month":5,"day":17}"#
    );
    assert!(serde_json::from_str::<Date>(r#"{"year":1990,"month":2,"day":30}"#).is_err());
}

proptest! {
    /// Day and moment agree, for every day a timestamp can name: the day
    /// begins at its start, ends one second before the next, and days
    /// follow each other without a gap.
    #[test]
    fn day_and_moment_agree(days in 0u64..2_932_896) {
        let start = Timestamp::from_secs(days * DAY);
        let day = Date::of(start).unwrap();
        prop_assert_eq!(day.start(), Some(start));
        prop_assert_eq!(Date::of(plus(start, DAY - 1)).unwrap(), day);
        let next = Date::of(plus(start, DAY)).unwrap();
        prop_assert!(next > day);
        prop_assert_eq!(next.start(), Some(plus(start, DAY)));
    }

    /// A birthday is the first day of a new age.
    #[test]
    fn a_birthday_is_the_first_day_of_an_age(
        days in 0u64..40_000, age in 1u16..120,
    ) {
        let b = Date::of(Timestamp::from_secs(days * DAY)).unwrap();
        let birthday = b.birthday(age).unwrap();
        prop_assert_eq!(b.age_on(birthday), age);
        let eve = Date::of(Timestamp::from_secs(birthday.start().unwrap().secs() - 1)).unwrap();
        prop_assert_eq!(b.age_on(eve), age - 1);
    }
}

// ------------------------------------------------ the first verification

#[test]
fn the_first_verification_pays_the_age_once() {
    let p = rules();
    let when = at(2026, 11, 15);
    // Verified at 36: 36 × 1.00, at once.
    let (state, paid) = verified(&p, date(1990, 5, 17), when, YEAR);
    assert_eq!(paid, 3_600);
    assert!(state.first_payout_made);
    assert_eq!(state.born, Some(date(1990, 5, 17)));

    // Verified again a month later, by a fresh statement: nothing more.
    let later = plus(when, 30 * DAY);
    let att = born(
        attestation(1, 1, None, later, Some(plus(later, YEAR))),
        date(1990, 5, 17),
    );
    let list = [p.clone()];
    let state = plan_attest(&state, &list, Some(&att), None, later)
        .unwrap()
        .state;
    assert_eq!(claim(&state, &p, later).1, 0);
}

#[test]
fn the_amount_of_the_first_verification_waits_for_a_claim() {
    // The core mints only in a claim: the statement sets the amount aside.
    let p = rules();
    let when = at(2026, 11, 15);
    let state = open(1, &p, when);
    let att = born(attestation(1, 1, None, when, None), date(2000, 1, 1));
    let plan = plan_attest(&state, std::slice::from_ref(&p), Some(&att), None, when).unwrap();
    assert!(plan.ops.is_empty());
    assert_eq!(plan.state.issuance.carried, cents(2_600));
    // Not lost if the claim comes long after the claim window.
    let (_, paid) = claim(&plan.state, &p, plus(when, 100 * DAY));
    assert_eq!(paid, 2_600);
}

#[test]
fn a_baby_gets_nothing_at_first_and_its_age_from_the_first_birthday() {
    let p = rules();
    let when = at(2026, 11, 15);
    let (state, paid) = verified(&p, date(2026, 6, 1), when, 3 * YEAR);
    assert_eq!(paid, 0);
    // The amount of the first verification is used up, although it was zero.
    assert!(state.first_payout_made);
    let (state, paid) = claim(&state, &p, at(2027, 6, 1));
    assert_eq!(paid, 200);
    let (_, paid) = claim(&state, &p, at(2028, 6, 1));
    assert_eq!(paid, 400);
}

#[test]
fn without_a_date_of_birth_nothing_comes_by_age() {
    let p = rules();
    let when = at(2026, 11, 15);
    let state = open(1, &p, when);
    let att = attestation(1, 1, None, when, None);
    let list = [p.clone()];
    let state = plan_attest(&state, &list, Some(&att), None, when)
        .unwrap()
        .state;
    assert!(!state.first_payout_made);
    assert_eq!(claim(&state, &p, plus(when, 10 * DAY)).1, 0);

    // The date of birth arrives with a later statement: the amount of the
    // first verification is paid then.
    let later = plus(when, 20 * DAY);
    let att = born(attestation(1, 1, None, later, None), date(1990, 5, 17));
    let state = plan_attest(&state, &list, Some(&att), None, later)
        .unwrap()
        .state;
    assert_eq!(claim(&state, &p, later).1, 3_600);
}

#[test]
fn a_date_of_birth_in_the_future_is_refused() {
    let p = rules();
    let when = at(2026, 11, 15);
    let state = open(1, &p, when);
    let att = born(attestation(1, 1, None, when, None), date(2026, 11, 16));
    assert_eq!(
        plan_attest(&state, std::slice::from_ref(&p), Some(&att), None, when),
        Err(CoreError::AttestationNotValid)
    );
    // Born today: fine.
    let att = born(attestation(1, 1, None, when, None), date(2026, 11, 15));
    assert!(plan_attest(&state, &[p], Some(&att), None, when).is_ok());
}

#[test]
fn a_level_below_the_rule_gets_no_first_amount() {
    let mut p = rules();
    p.age.min_level = capita_core::attributes::VerificationLevel(2);
    let when = at(2026, 11, 15);
    let (state, paid) = verified(&p, date(1990, 5, 17), when, YEAR);
    assert_eq!(paid, 0);
    assert!(!state.first_payout_made);
}

#[test]
fn rules_without_age_change_nothing() {
    let mut p = rules();
    p.age = AgeParams::OFF;
    let when = at(2026, 11, 15);
    let (state, paid) = verified(&p, date(1990, 5, 17), when, 3 * YEAR);
    assert_eq!(paid, 0);
    assert!(!state.first_payout_made);
    assert_eq!(claim(&state, &p, at(2027, 5, 17)).1, 0);
}

// ---------------------------------------------------------- the birthdays

#[test]
fn a_birthday_pays_twice_the_age_at_once() {
    let p = rules();
    let (state, _) = verified(&p, date(1990, 5, 17), at(2026, 11, 15), YEAR);
    // The day before: nothing yet.
    let eve = plus(at(2027, 5, 16), DAY - 1);
    let (state, paid) = claim(&state, &p, eve);
    assert_eq!(paid, 0);
    // The first second of the birthday: 37 × 2.00.
    let (state, paid) = claim(&state, &p, at(2027, 5, 17));
    assert_eq!(paid, 7_400);
    // Never twice.
    assert_eq!(claim(&state, &p, plus(at(2027, 5, 17), 3_600)).1, 0);
    assert_eq!(claim(&state, &p, plus(at(2027, 5, 17), 5 * DAY)).1, 0);
}

#[test]
fn the_claim_says_what_it_paid() {
    let p = rules();
    let (state, _) = verified(&p, date(1990, 5, 17), at(2026, 11, 15), YEAR);
    let now = plus(at(2027, 5, 17), 3_600);
    let plan = plan_claim(&state, std::slice::from_ref(&p), now).unwrap();
    assert_eq!(
        plan.events,
        vec![Event::Claimed {
            account: id(1),
            issued: cents(7_400),
            repaid: Amount::ZERO,
            // The claim reaches back one claim window, not further.
            from: now.checked_sub(p.claim_window).unwrap(),
            to: now,
        }]
    );
}

#[test]
fn a_birthday_older_than_the_claim_window_lapses() {
    let p = rules();
    let (state, _) = verified(&p, date(1990, 5, 17), at(2026, 11, 15), YEAR);
    // 14 days after the birthday, to the second: still paid.
    let last = plus(at(2027, 5, 17), 14 * DAY - 1);
    assert_eq!(claim(&state, &p, last).1, 7_400);
    // One second later the birthday is no longer inside the window.
    let late = plus(at(2027, 5, 17), 14 * DAY);
    let (state, paid) = claim(&state, &p, late);
    assert_eq!(paid, 0);
    // And it stays lapsed.
    assert_eq!(claim(&state, &p, plus(late, DAY)).1, 0);
}

#[test]
fn a_long_claim_window_pays_every_birthday_inside_it() {
    let mut p = rules();
    p.claim_window = secs(3 * YEAR);
    let (state, _) = verified(&p, date(1990, 5, 17), at(2026, 11, 15), 5 * YEAR);
    // Two birthdays later, one claim: 37 × 2 + 38 × 2.
    let (_, paid) = claim(&state, &p, at(2028, 6, 1));
    assert_eq!(paid, 7_400 + 7_600);
}

#[test]
fn a_birthday_on_which_the_verification_had_run_out_pays_nothing() {
    let p = rules();
    // Verified for 100 days only; the birthday comes after that.
    let (state, _) = verified(&p, date(1990, 5, 17), at(2026, 11, 15), 100 * DAY);
    assert_eq!(claim(&state, &p, at(2027, 5, 17)).1, 0);

    // Verified again the day after the birthday: that birthday stays lost,
    // the next one pays.
    let again = at(2027, 5, 18);
    let att = born(
        attestation(1, 1, None, again, Some(plus(again, 2 * YEAR))),
        date(1990, 5, 17),
    );
    let list = [p.clone()];
    let state = plan_attest(&state, &list, Some(&att), None, again)
        .unwrap()
        .state;
    assert_eq!(claim(&state, &p, again).1, 0);
    assert_eq!(claim(&state, &p, at(2028, 5, 17)).1, 7_600);
}

#[test]
fn a_verification_renewed_in_time_keeps_the_birthday() {
    let p = rules();
    let (state, _) = verified(&p, date(1990, 5, 17), at(2026, 11, 15), 100 * DAY);
    // Renewed before it ran out, with the same level.
    let renewed = at(2027, 1, 10);
    let att = born(
        attestation(1, 1, None, renewed, Some(plus(renewed, YEAR))),
        date(1990, 5, 17),
    );
    let list = [p.clone()];
    let state = plan_attest(&state, &list, Some(&att), None, renewed)
        .unwrap()
        .state;
    assert_eq!(claim(&state, &p, at(2027, 5, 17)).1, 7_400);
}

#[test]
fn a_verification_that_ends_takes_the_birthdays_before_it_along() {
    let mut p = rules();
    p.claim_window = secs(YEAR);
    let (state, _) = verified(&p, date(1990, 5, 17), at(2026, 11, 15), 2 * YEAR);
    // Revoked a week after the birthday, unclaimed so far: the birthday
    // was earned while verified and is set aside.
    let revoked = at(2027, 5, 24);
    let list = [p.clone()];
    let state = plan_attest(&state, &list, None, None, revoked)
        .unwrap()
        .state;
    assert_eq!(state.issuance.carried, cents(7_400));
    assert_eq!(claim(&state, &p, plus(revoked, DAY)).1, 7_400);
}

#[test]
fn a_locked_account_keeps_its_birthday() {
    let p = rules();
    let list = [p.clone()];
    let (state, _) = verified(&p, date(1990, 5, 17), at(2026, 11, 15), 3 * YEAR);
    // Locked by the holder before the birthday, in dispute over it, open
    // again after: nobody could claim meanwhile, and nothing is lost.
    let mut state = state;
    for (status, when) in [
        (AccountStatus::HolderFrozen, at(2027, 5, 1)),
        (AccountStatus::Disputed, at(2027, 5, 10)),
    ] {
        state = plan_set_status(&state, Balance::ZERO, &list, status, when)
            .unwrap()
            .state;
        assert_eq!(
            plan_claim(&state, &list, plus(when, DAY)).map(|_| ()),
            Err(CoreError::AccountNotActive(status))
        );
    }
    let reopened = plan_set_status(
        &state,
        Balance::ZERO,
        &list,
        AccountStatus::Active,
        at(2027, 5, 20),
    )
    .unwrap()
    .state;
    assert_eq!(claim(&reopened, &p, at(2027, 5, 21)).1, 7_400);
    // Frozen by the authority out of a lock: what accrued until then is
    // set aside, nothing after.
    let frozen = plan_set_status(
        &state,
        Balance::ZERO,
        &list,
        AccountStatus::Frozen,
        at(2027, 5, 20),
    )
    .unwrap()
    .state;
    assert_eq!(frozen.issuance.carried, cents(7_400));
}

#[test]
fn a_frozen_account_earns_no_birthday() {
    let p = rules();
    let list = [p.clone()];
    let (state, _) = verified(&p, date(1990, 5, 17), at(2026, 11, 15), 3 * YEAR);
    let frozen = plan_set_status(
        &state,
        Balance::ZERO,
        &list,
        AccountStatus::Frozen,
        at(2027, 5, 1),
    )
    .unwrap()
    .state;
    // Reopened after the birthday: that birthday is gone.
    let reopened = plan_set_status(
        &frozen,
        Balance::ZERO,
        &list,
        AccountStatus::Active,
        at(2027, 5, 20),
    )
    .unwrap()
    .state;
    assert_eq!(claim(&reopened, &p, at(2027, 5, 21)).1, 0);
    assert_eq!(claim(&reopened, &p, at(2028, 5, 17)).1, 7_600);
}

#[test]
fn born_on_29_february_the_money_comes_on_1_march() {
    let p = rules();
    let (state, _) = verified(&p, date(1996, 2, 29), at(2026, 11, 15), 3 * YEAR);
    let (state, paid) = claim(&state, &p, plus(at(2027, 2, 28), DAY - 1));
    assert_eq!(paid, 0);
    let (state, paid) = claim(&state, &p, at(2027, 3, 1));
    assert_eq!(paid, 31 * 200);
    // The leap year: on the day itself.
    let (state, paid) = claim(&state, &p, plus(at(2028, 2, 28), DAY - 1));
    assert_eq!(paid, 0);
    let (_, paid) = claim(&state, &p, at(2028, 2, 29));
    assert_eq!(paid, 32 * 200);
}

#[test]
fn verified_on_the_birthday_itself_the_day_is_not_paid_twice() {
    let p = rules();
    // Verified at noon of the 37th birthday: 37 × 1.00 for the
    // verification; the birthday began before it and does not count.
    let noon = plus(at(2027, 5, 17), 12 * 3_600);
    let (state, paid) = verified(&p, date(1990, 5, 17), noon, 2 * YEAR);
    assert_eq!(paid, 3_700);
    assert_eq!(claim(&state, &p, plus(noon, DAY)).1, 0);
    assert_eq!(claim(&state, &p, at(2028, 5, 17)).1, 7_600);
}

#[test]
fn a_new_amount_counts_from_the_day_it_starts() {
    let v1 = rules();
    let mut v2 = successor(&v1, at(2028, 1, 1));
    v2.age.per_year = cents(300);
    let mut list = vec![v1.clone()];
    let (state, _) = verified(&v1, date(1990, 5, 17), at(2026, 11, 15), 5 * YEAR);
    // Birthday 2027: old amount.
    let plan = plan_claim(&state, &list, at(2027, 5, 17)).unwrap();
    assert_eq!(minted(&plan), 37 * 200);
    // Birthday 2028: the new amount is in force.
    list.push(v2);
    let plan = plan_claim(&plan.state, &list, at(2028, 5, 17)).unwrap();
    assert_eq!(minted(&plan), 38 * 300);
}

#[test]
fn a_country_can_have_its_own_amount() {
    let mut p = rules();
    p.age.by_country = vec![CountryAge {
        country: CountryCode(*b"IN"),
        per_year: cents(400),
    }];
    p.validate().unwrap();
    let when = at(2026, 11, 15);
    let list = [p.clone()];
    let in_india = |country: Option<&[u8; 2]>| {
        let state = open(1, &p, when);
        let att = born(
            attestation(1, 1, country, when, Some(plus(when, YEAR))),
            date(1990, 5, 17),
        );
        let state = plan_attest(&state, &list, Some(&att), None, when)
            .unwrap()
            .state;
        let (state, first) = claim(&state, &p, when);
        (first, claim(&state, &p, at(2027, 5, 17)).1)
    };
    // The amount of the first verification is the same everywhere.
    assert_eq!(in_india(Some(b"IN")), (3_600, 37 * 400));
    assert_eq!(in_india(Some(b"DE")), (3_600, 37 * 200));
    assert_eq!(in_india(None), (3_600, 37 * 200));

    // A country may be listed once, in capital letters.
    let mut twice = p.clone();
    twice.age.by_country.push(twice.age.by_country[0]);
    assert!(twice.validate().is_err());
    let mut small = p;
    small.age.by_country[0].country = CountryCode(*b"in");
    assert!(small.validate().is_err());
}

#[test]
fn a_corrected_date_of_birth_counts_from_the_correction() {
    let mut p = rules();
    p.claim_window = secs(YEAR);
    let list = [p.clone()];
    let (state, _) = verified(&p, date(1990, 5, 17), at(2026, 11, 15), 5 * YEAR);
    // After the birthday of the old date, the service corrects the date.
    let fixed = at(2027, 6, 1);
    let att = born(
        attestation(1, 1, None, fixed, Some(plus(fixed, 5 * YEAR))),
        date(1985, 9, 1),
    );
    let state = plan_attest(&state, &list, Some(&att), None, fixed)
        .unwrap()
        .state;
    // The birthday that passed under the old date is paid as it was then;
    // no second amount for the verification.
    assert_eq!(state.issuance.carried, cents(7_400));
    let (state, paid) = claim(&state, &p, fixed);
    assert_eq!(paid, 7_400);
    // From now on the new date counts: 42 on 1 September 2027.
    assert_eq!(claim(&state, &p, at(2027, 9, 1)).1, 42 * 200);
}

#[test]
fn the_age_follows_the_person_to_a_new_key() {
    let mut p = rules();
    p.claim_window = secs(YEAR);
    let list = [p.clone()];
    let (old, _) = verified(&p, date(1990, 5, 17), at(2026, 11, 15), 5 * YEAR);
    // The key is lost; a new account, a new statement, after a birthday
    // nobody could claim.
    let now = at(2027, 6, 1);
    let new = open(2, &p, now);
    let att = born(
        attestation(1, 1, None, now, Some(plus(now, YEAR))),
        date(1990, 5, 17),
    );
    let plan = plan_move(&old, Balance::from_cents(3_600), &new, &list, &att, now).unwrap();
    assert_eq!(plan.to.born, Some(date(1990, 5, 17)));
    assert!(plan.to.first_payout_made);
    assert_eq!(plan.from.born, None);
    // The birthday under the old key comes along; no second amount for a
    // verification.
    assert_eq!(plan.to.issuance.carried, cents(7_400));
    let (state, paid) = claim(&plan.to, &p, now);
    assert_eq!(paid, 7_400);
    assert_eq!(claim(&state, &p, at(2028, 5, 17)).1, 7_600);
}

#[test]
fn ripening_delays_the_money_by_age_too() {
    let mut p = rules();
    p.ripening = secs(7 * DAY);
    let when = at(2026, 11, 15);
    let (state, paid) = verified(&p, date(1990, 5, 17), when, 2 * YEAR);
    assert_eq!(paid, 0);
    let (state, paid) = claim(&state, &p, plus(when, 7 * DAY));
    assert_eq!(paid, 3_600);
    // A birthday becomes money seven days later.
    let (state, paid) = claim(&state, &p, plus(at(2027, 5, 17), 6 * DAY));
    assert_eq!(paid, 0);
    assert_eq!(claim(&state, &p, plus(at(2027, 5, 17), 7 * DAY)).1, 7_400);
}

#[test]
fn an_absurd_age_is_an_error_not_a_wrong_number() {
    let mut p = rules();
    p.age.at_verification = cents(u64::MAX / 2);
    let when = at(2026, 11, 15);
    let state = open(1, &p, when);
    let att = born(attestation(1, 1, None, when, None), date(1, 1, 1));
    assert!(matches!(
        plan_attest(&state, &[p], Some(&att), None, when),
        Err(CoreError::Arithmetic(_))
    ));
}

proptest! {
    /// However the claims are spread over the years, every birthday pays
    /// exactly once – if each claim comes within the claim window of the
    /// birthdays before it.
    #[test]
    fn every_birthday_pays_exactly_once(
        born_day in 0u64..20_000,
        gaps in proptest::collection::vec(1u64..400 * DAY, 1..12),
    ) {
        let mut p = rules();
        p.claim_window = secs(2 * YEAR);
        let b = Date::of(Timestamp::from_secs(born_day * DAY)).unwrap();
        let start = at(2026, 11, 15);
        let (mut state, first) = verified(&p, b, start, 100 * YEAR);
        prop_assert_eq!(first, u64::from(b.age_on(date(2026, 11, 15))) * 100);
        let mut now = start;
        let mut total = 0;
        for gap in gaps {
            now = plus(now, gap);
            let (next, paid) = claim(&state, &p, now);
            state = next;
            total += paid;
        }
        // Expected: every birthday after the verification, up to now.
        let from = b.age_on(date(2026, 11, 15));
        let upto = b.age_on(Date::of(now).unwrap());
        let expected: u64 = (from + 1..=upto).map(|age| u64::from(age) * 200).sum();
        prop_assert_eq!(total, expected);
        prop_assert_eq!(
            birthdays_due(&state, &[p], now, now).unwrap(),
            Amount::ZERO
        );
    }
}
