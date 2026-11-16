// Capita Money – pallet-capita-quota
// SPDX-License-Identifier: AGPL-3.0-only

//! The free quota at work: four a day for a person, two for a key without
//! one, three such keys per block (the numbers of the mock).

use capita_core::quota::Usage;
use sp_runtime::transaction_validity::InvalidTransaction;

use crate::mock::*;
use crate::{NOT_FOR_NEW_KEYS, PERSON_PRIORITY, SERVICE_PRIORITY, USED_UP};

const USED: InvalidTransaction = InvalidTransaction::Custom(USED_UP);
const NOT_OPEN: InvalidTransaction = InvalidTransaction::Custom(NOT_FOR_NEW_KEYS);

#[test]
fn a_person_sends_the_number_of_the_day_and_no_more() {
    new_test_ext().execute_with(|| {
        let who = person(1);
        assert_eq!(Quota::left(&who), Some(4));
        for sent in 1..=4 {
            assert_eq!(Quota::spend(&who, &other()), Ok(()));
            assert_eq!(Quota::left(&who), Some(4 - sent));
        }
        // used up: not into the pool, not into a block
        assert_eq!(Quota::check(&who, &other(), false), Err(USED));
        assert_eq!(Quota::check(&who, &other(), true), Err(USED));
        assert_eq!(Quota::spend(&who, &other()), Err(USED));
        assert_eq!(Quota::left(&who), Some(0));

        // somebody else is not touched
        assert_eq!(Quota::left(&person(2)), Some(4));

        // the next day of UTC: all of it again
        advance(12 * 3600);
        assert_eq!(Quota::left(&who), Some(4));
        assert_eq!(Quota::spend(&who, &other()), Ok(()));
        assert_eq!(
            crate::Used::<Test>::get(&who),
            Usage {
                day: Quota::today(),
                used: 1
            }
        );
    });
}

#[test]
fn the_day_ends_at_midnight_of_utc() {
    new_test_ext().execute_with(|| {
        let who = person(1);
        for _ in 0..4 {
            assert_eq!(Quota::spend(&who, &other()), Ok(()));
        }
        // one second before midnight
        advance(12 * 3600 - 1);
        assert_eq!(Quota::left(&who), Some(0));
        advance(1);
        assert_eq!(Quota::left(&who), Some(4));
    });
}

#[test]
fn a_key_without_a_person_only_opens_and_presents() {
    new_test_ext().execute_with(|| {
        let who = account(7);
        assert_eq!(Quota::check(&who, &other(), false), Err(NOT_OPEN));
        assert_eq!(Quota::spend(&who, &other()), Err(NOT_OPEN));
        // refused: nothing counted
        assert_eq!(Quota::left(&who), Some(2));
        assert_eq!(Quota::new_keys_in_block(), 0);

        assert_eq!(Quota::spend(&who, &opening()), Ok(()));
        assert_eq!(Quota::spend(&who, &opening()), Ok(()));
        assert_eq!(Quota::spend(&who, &opening()), Err(USED));
        assert_eq!(Quota::new_keys_in_block(), 2);

        advance(DAY);
        assert_eq!(Quota::left(&who), Some(2));
    });
}

#[test]
fn a_block_takes_only_so_many_keys_without_a_person() {
    new_test_ext().execute_with(|| {
        for n in 10..13 {
            assert_eq!(Quota::spend(&account(n), &opening()), Ok(()));
        }
        assert_eq!(Quota::new_keys_in_block(), 3);
        let late = account(13);
        // the block is full for it …
        assert_eq!(
            Quota::check(&late, &opening(), true),
            Err(InvalidTransaction::ExhaustsResources)
        );
        assert_eq!(
            Quota::spend(&late, &opening()),
            Err(InvalidTransaction::ExhaustsResources)
        );
        // … the pool is not, and nothing of its day is gone
        assert!(Quota::check(&late, &opening(), false).is_ok());
        assert_eq!(Quota::left(&late), Some(2));

        // persons and services are not held up by it
        assert_eq!(Quota::spend(&person(1), &other()), Ok(()));
        assert_eq!(Quota::spend(&service(2), &other()), Ok(()));

        // the next block takes it
        System::set_block_number(2);
        assert_eq!(Quota::new_keys_in_block(), 0);
        assert_eq!(Quota::spend(&late, &opening()), Ok(()));
        assert_eq!(Quota::new_keys_in_block(), 1);
    });
}

#[test]
fn a_service_is_not_counted() {
    new_test_ext().execute_with(|| {
        let who = service(9);
        assert_eq!(Quota::left(&who), None);
        for _ in 0..50 {
            assert_eq!(Quota::spend(&who, &other()), Ok(()));
        }
        assert_eq!(Quota::check(&who, &other(), true), Ok(SERVICE_PRIORITY));
        assert_eq!(crate::Used::<Test>::get(&who), Usage::default());
        assert_eq!(Quota::new_keys_in_block(), 0);
    });
}

#[test]
fn persons_come_before_new_keys_and_who_sent_less_comes_first() {
    new_test_ext().execute_with(|| {
        let (fresh, busy, newcomer) = (person(1), person(2), account(3));
        assert_eq!(Quota::spend(&busy, &other()), Ok(()));
        assert_eq!(Quota::spend(&busy, &other()), Ok(()));

        let fresh_place = Quota::check(&fresh, &other(), false).unwrap();
        let busy_place = Quota::check(&busy, &other(), false).unwrap();
        let newcomer_place = Quota::check(&newcomer, &opening(), false).unwrap();
        assert_eq!(fresh_place, PERSON_PRIORITY + 4);
        assert_eq!(busy_place, PERSON_PRIORITY + 2);
        assert_eq!(newcomer_place, 2);
        assert!(SERVICE_PRIORITY > fresh_place);
        assert!(fresh_place > busy_place);
        assert!(busy_place > newcomer_place);
    });
}

#[test]
fn a_key_that_becomes_a_person_has_the_quota_of_a_person() {
    new_test_ext().execute_with(|| {
        let who = account(5);
        assert_eq!(Quota::spend(&who, &opening()), Ok(()));
        assert_eq!(Quota::spend(&who, &opening()), Ok(()));
        assert_eq!(Quota::left(&who), Some(0));

        // verified: what it sent today counts on
        person(5);
        assert_eq!(Quota::left(&who), Some(2));
        assert_eq!(Quota::spend(&who, &other()), Ok(()));
        assert_eq!(Quota::spend(&who, &other()), Ok(()));
        assert_eq!(Quota::spend(&who, &other()), Err(USED));
        // a person is not counted against the block's number of new keys
        assert_eq!(Quota::new_keys_in_block(), 2);
    });
}

#[test]
fn checking_changes_nothing() {
    new_test_ext().execute_with(|| {
        let who = person(1);
        for _ in 0..20 {
            assert!(Quota::check(&who, &other(), false).is_ok());
            assert!(Quota::check(&who, &other(), true).is_ok());
        }
        assert_eq!(Quota::left(&who), Some(4));
        assert_eq!(crate::Used::<Test>::get(&who), Usage::default());
    });
}
