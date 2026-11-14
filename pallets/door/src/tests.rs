// Capita Money – pallet-capita-door
// SPDX-License-Identifier: AGPL-3.0-only

//! The euro door on chain: vouchers mint once, false ones never.

use capita_core::types::Amount;
use capita_door::types::{Door as DoorEntry, DoorId, DoorStatus, PaymentRef};
use codec::Encode;
use frame_support::pallet_prelude::{InvalidTransaction, TransactionSource, ValidateUnsigned};
use frame_support::{assert_noop, assert_ok};
use frame_system::RawOrigin;
use sp_runtime::DispatchError::BadOrigin;

use crate::mock::*;
use crate::{Call, Doors, Error, Event, Pallet, Redeemed};

fn redeem(
    v: capita_door::types::Voucher,
    s: capita_door::types::DoorSignature,
) -> sp_runtime::DispatchResult {
    CapitaDoor::redeem(RawOrigin::None.into(), v, s)
}

#[test]
fn a_voucher_mints_once() {
    new_test_ext().execute_with(|| {
        open(5);
        let v = voucher(0, 1, 5, 500);
        let s = sign(1, &v);
        assert_ok!(redeem(v, s));
        assert_eq!(bought(5), 500);
        assert_eq!(
            Redeemed::<Test>::get(PaymentRef([1; 32])),
            Some((DoorId(0), acct(5)))
        );
        System::assert_last_event(
            Event::Redeemed {
                door: DoorId(0),
                payment: PaymentRef([1; 32]),
                account: acct(5),
                amount: Amount::from_cents(500),
            }
            .into(),
        );
        assert_noop!(redeem(v, s), Error::<Test>::AlreadyRedeemed);
        assert_eq!(bought(5), 500);
    });
}

#[test]
fn false_vouchers_mint_nothing() {
    new_test_ext().execute_with(|| {
        open(5);
        let v = voucher(0, 1, 5, 500);
        // signed by a key that is not the door's
        assert_noop!(redeem(v, sign(2, &v)), Error::<Test>::BadSignature);
        // changed after signing
        let s = sign(1, &v);
        let more = capita_door::types::Voucher {
            amount: Amount::from_cents(501),
            ..v
        };
        assert_noop!(redeem(more, s), Error::<Test>::BadSignature);
        // a door that does not exist
        let v = voucher(7, 1, 5, 500);
        assert_noop!(redeem(v, sign(1, &v)), Error::<Test>::UnknownDoor);
        // above the limit of the door, or for nothing
        let v = voucher(0, 1, 5, 10_001);
        assert_noop!(redeem(v, sign(1, &v)), Error::<Test>::AmountTooHigh);
        let v = voucher(0, 1, 5, 0);
        assert_noop!(redeem(v, sign(1, &v)), Error::<Test>::ZeroAmount);
        // signed: only as a transaction without signer
        let v = voucher(0, 1, 5, 500);
        assert_noop!(
            CapitaDoor::redeem(RawOrigin::Signed(acct(5)).into(), v, sign(1, &v)),
            BadOrigin
        );
        assert_eq!(bought(5), 0);
    });
}

#[test]
fn a_voucher_for_an_account_not_open_waits() {
    new_test_ext().execute_with(|| {
        let v = voucher(0, 1, 5, 500);
        let s = sign(1, &v);
        assert_noop!(redeem(v, s), Error::<Test>::CannotReceive);
        open(5);
        assert_ok!(redeem(v, s));
        assert_eq!(bought(5), 500);
    });
}

#[test]
fn the_list_of_doors_is_kept_by_its_origin() {
    new_test_ext().execute_with(|| {
        open(5);
        assert_noop!(
            CapitaDoor::register_door(RawOrigin::Signed(acct(5)).into(), door(2, 100)),
            BadOrigin
        );
        assert_ok!(CapitaDoor::register_door(
            RawOrigin::Root.into(),
            door(2, 100)
        ));
        assert_eq!(Doors::<Test>::get(DoorId(1)), Some(door(2, 100)));
        let v = voucher(1, 3, 5, 100);
        let s = sign(2, &v);
        let suspended = DoorEntry {
            status: DoorStatus::Suspended,
            ..door(2, 100)
        };
        assert_ok!(CapitaDoor::update_door(
            RawOrigin::Root.into(),
            DoorId(1),
            suspended
        ));
        assert_noop!(redeem(v, s), Error::<Test>::DoorSuspended);
        assert_ok!(CapitaDoor::update_door(
            RawOrigin::Root.into(),
            DoorId(1),
            door(2, 100)
        ));
        assert_ok!(redeem(v, s));
        assert_noop!(
            CapitaDoor::update_door(RawOrigin::Root.into(), DoorId(9), door(2, 100)),
            Error::<Test>::UnknownDoor
        );
    });
}

#[test]
fn the_pool_takes_only_vouchers_that_would_mint() {
    new_test_ext().execute_with(|| {
        let v = voucher(0, 1, 5, 500);
        let good = Call::<Test>::redeem {
            voucher: v,
            signature: sign(1, &v),
        };
        let validate = |call: &Call<Test>| {
            Pallet::<Test>::validate_unsigned(TransactionSource::External, call)
        };
        assert_eq!(validate(&good), Err(InvalidTransaction::Custom(1).into()));
        open(5);
        let valid = validate(&good).unwrap();
        assert_eq!(
            valid.provides,
            vec![("CapitaDoor", PaymentRef([1; 32])).encode()]
        );
        let forged = Call::<Test>::redeem {
            voucher: v,
            signature: sign(2, &v),
        };
        assert_eq!(validate(&forged), Err(InvalidTransaction::BadProof.into()));
        assert_ok!(redeem(v, sign(1, &v)));
        assert_eq!(validate(&good), Err(InvalidTransaction::Stale.into()));
    });
}
