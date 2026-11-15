// Capita Money – pallet-capita-keys
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

use capita_keys::encoding::account_of;
use capita_keys::types::{DeviceKey, Proof};
use codec::Decode;
use frame_support::pallet_prelude::{InvalidTransaction, TransactionSource, ValidateUnsigned};
use frame_support::{assert_noop, assert_ok};
use sp_core::Pair;
use sp_runtime::traits::Dispatchable;
use sp_runtime::AccountId32;

use crate::mock::*;
use crate::{Error, Event, HomeClaim, KeyOwner, Keys as KeyList, Nonce};

fn keys_of(account: &AccountId32) -> Vec<DeviceKey> {
    KeyList::<Test>::get(account).into_inner()
}

fn validate(call: &RuntimeCall) -> Result<(), InvalidTransaction> {
    let RuntimeCall::Keys(inner) = call else {
        panic!("a call of the keys pallet");
    };
    <Keys as ValidateUnsigned>::validate_unsigned(TransactionSource::External, inner)
        .map(|_| ())
        .map_err(|e| match e {
            sp_runtime::transaction_validity::TransactionValidityError::Invalid(i) => i,
            other => panic!("unexpected {other:?}"),
        })
}

#[test]
fn a_first_key_opens_the_account_derived_from_it() {
    new_test_ext().execute_with(|| {
        let account = open_ed(1);
        assert_eq!(account, AccountId32::from(account_of(&ed_key(1))));
        assert_eq!(keys_of(&account), vec![ed_key(1)]);
        assert_eq!(KeyOwner::<Test>::get(ed_key(1)), Some(account.clone()));
        assert_eq!(Nonce::<Test>::get(&account), 1);
        assert_eq!(last_result(), Some(Ok(())));
        System::assert_has_event(
            Event::Opened {
                account,
                key: ed_key(1),
            }
            .into(),
        );
    });
}

#[test]
fn a_key_opens_once() {
    new_test_ext().execute_with(|| {
        let account = open_ed(1);
        let call = remark(0);
        let proof = sign_ed(1, &account, 0, &call);
        assert_noop!(
            Keys::open(RuntimeOrigin::none(), ed_key(1), Box::new(call), proof),
            Error::<Test>::KeyTaken
        );
    });
}

#[test]
fn an_action_runs_once() {
    new_test_ext().execute_with(|| {
        let account = open_ed(1);
        let call = remark(7);
        let proof = sign_ed(1, &account, 1, &call);
        let action = RuntimeCall::Keys(crate::Call::act {
            account: account.clone(),
            key: ed_key(1),
            call: Box::new(call),
            nonce: 1,
            proof,
        });
        assert_eq!(validate(&action), Ok(()));
        assert_ok!(action.clone().dispatch(RuntimeOrigin::none()));
        assert_eq!(Nonce::<Test>::get(&account), 2);
        assert_eq!(validate(&action), Err(InvalidTransaction::Stale));
        assert!(action.dispatch(RuntimeOrigin::none()).is_err());

        let later = remark(8);
        let proof = sign_ed(1, &account, 5, &later);
        let early = RuntimeCall::Keys(crate::Call::act {
            account,
            key: ed_key(1),
            call: Box::new(later),
            nonce: 5,
            proof,
        });
        assert_eq!(validate(&early), Err(InvalidTransaction::Future));
    });
}

#[test]
fn only_a_listed_key_with_its_own_signature_acts() {
    new_test_ext().execute_with(|| {
        let account = open_ed(1);
        let call = remark(1);
        let proof = sign_ed(2, &account, 1, &call);
        assert_noop!(
            Keys::act(
                RuntimeOrigin::none(),
                account.clone(),
                ed_key(2),
                Box::new(call.clone()),
                1,
                proof.clone()
            ),
            Error::<Test>::UnknownKey
        );
        assert_noop!(
            Keys::act(
                RuntimeOrigin::none(),
                account,
                ed_key(1),
                Box::new(call),
                1,
                proof
            ),
            Error::<Test>::BadProof
        );
    });
}

#[test]
fn a_failing_call_spends_the_nonce() {
    new_test_ext().execute_with(|| {
        let account = open_ed(1);
        let call = RuntimeCall::Keys(crate::Call::claim_home {});
        assert_ok!(act_ed(1, &account, call));
        assert_eq!(last_result(), Some(Err(Error::<Test>::AlreadyHome.into())));
        assert_eq!(Nonce::<Test>::get(&account), 2);
    });
}

#[test]
fn an_action_cannot_contain_an_action() {
    new_test_ext().execute_with(|| {
        let account = open_ed(1);
        let inner = remark(1);
        let proof = sign_ed(1, &account, 2, &inner);
        let nested = RuntimeCall::Keys(crate::Call::act {
            account: account.clone(),
            key: ed_key(1),
            call: Box::new(inner),
            nonce: 2,
            proof,
        });
        assert_noop!(act_ed(1, &account, nested), Error::<Test>::NestedCall);
    });
}

#[test]
fn up_to_three_devices_each_proving_its_key() {
    new_test_ext().execute_with(|| {
        let account = open_ed(1);
        assert_ok!(act_ed(1, &account, add_ed(2, &account)));
        assert_eq!(last_result(), Some(Ok(())));
        assert_ok!(act_ed(2, &account, add_ed(3, &account)));
        assert_eq!(last_result(), Some(Ok(())));
        assert_eq!(keys_of(&account), vec![ed_key(1), ed_key(2), ed_key(3)]);
        assert_ok!(act_ed(1, &account, add_ed(4, &account)));
        assert_eq!(last_result(), Some(Err(Error::<Test>::TooManyKeys.into())));

        // a possession proof by another key than the one linked
        let raw: [u8; 32] = account.clone().into();
        let c = capita_keys::encoding::add_challenge(&Keys::audience(), &raw, &ed_key(5));
        let wrong = RuntimeCall::Keys(crate::Call::add_key {
            key: ed_key(5),
            possession: Proof::Ed25519(ed(6).sign(&c).0),
        });
        assert_ok!(act_ed(1, &account, wrong));
        assert_eq!(last_result(), Some(Err(Error::<Test>::BadProof.into())));
    });
}

#[test]
fn a_key_acts_for_one_account() {
    new_test_ext().execute_with(|| {
        let a = open_ed(1);
        let _b = open_ed(2);
        assert_ok!(act_ed(1, &a, add_ed(2, &a)));
        assert_eq!(last_result(), Some(Err(Error::<Test>::KeyTaken.into())));
    });
}

#[test]
fn key_changes_go_through_an_action() {
    new_test_ext().execute_with(|| {
        let account = open_ed(1);
        let RuntimeCall::Keys(crate::Call::add_key { key, possession }) = add_ed(2, &account)
        else {
            panic!("add_key");
        };
        assert_noop!(
            Keys::add_key(RuntimeOrigin::signed(account), key, possession),
            Error::<Test>::NotByKey
        );
    });
}

#[test]
fn the_home_key_removes_others_a_linked_key_only_itself() {
    new_test_ext().execute_with(|| {
        let account = open_ed(1);
        assert_ok!(act_ed(1, &account, add_ed(2, &account)));
        assert_ok!(act_ed(1, &account, add_ed(3, &account)));
        let remove = |k: u8| RuntimeCall::Keys(crate::Call::remove_key { key: ed_key(k) });

        assert_ok!(act_ed(2, &account, remove(1)));
        assert_eq!(last_result(), Some(Err(Error::<Test>::NotHome.into())));
        assert_ok!(act_ed(2, &account, remove(3)));
        assert_eq!(last_result(), Some(Err(Error::<Test>::NotHome.into())));

        assert_ok!(act_ed(3, &account, remove(3)));
        assert_eq!(last_result(), Some(Ok(())));
        assert_eq!(KeyOwner::<Test>::get(ed_key(3)), None);

        assert_ok!(act_ed(1, &account, remove(2)));
        assert_eq!(last_result(), Some(Ok(())));
        assert_eq!(keys_of(&account), vec![ed_key(1)]);

        assert_ok!(act_ed(1, &account, remove(1)));
        assert_eq!(last_result(), Some(Err(Error::<Test>::LastKey.into())));
    });
}

#[test]
fn a_linked_key_becomes_home_after_the_delay() {
    new_test_ext().execute_with(|| {
        let account = open_ed(1);
        assert_ok!(act_ed(1, &account, add_ed(2, &account)));
        let claim = RuntimeCall::Keys(crate::Call::claim_home {});
        let take = RuntimeCall::Keys(crate::Call::take_home {});

        assert_ok!(act_ed(2, &account, claim));
        assert_eq!(last_result(), Some(Ok(())));
        assert!(HomeClaim::<Test>::get(&account).is_some());

        advance(WEEK - 1);
        assert_ok!(act_ed(2, &account, take.clone()));
        assert_eq!(last_result(), Some(Err(Error::<Test>::TooEarly.into())));

        advance(1);
        assert_ok!(act_ed(2, &account, take));
        assert_eq!(last_result(), Some(Ok(())));
        assert_eq!(keys_of(&account), vec![ed_key(2), ed_key(1)]);
        assert!(HomeClaim::<Test>::get(&account).is_none());
    });
}

#[test]
fn the_home_key_acting_withdraws_a_claim() {
    new_test_ext().execute_with(|| {
        let account = open_ed(1);
        assert_ok!(act_ed(1, &account, add_ed(2, &account)));
        assert_ok!(act_ed(
            2,
            &account,
            RuntimeCall::Keys(crate::Call::claim_home {})
        ));
        assert_ok!(act_ed(
            1,
            &account,
            RuntimeCall::Keys(crate::Call::refuse_home {})
        ));
        assert_eq!(last_result(), Some(Ok(())));
        System::assert_has_event(
            Event::HomeRefused {
                account: account.clone(),
            }
            .into(),
        );
        advance(WEEK);
        assert_ok!(act_ed(
            2,
            &account,
            RuntimeCall::Keys(crate::Call::take_home {})
        ));
        assert_eq!(last_result(), Some(Err(Error::<Test>::NoClaim.into())));
        assert_eq!(keys_of(&account), vec![ed_key(1), ed_key(2)]);
    });
}

#[test]
fn a_passkey_opens_and_acts() {
    new_test_ext().execute_with(|| {
        let p = phone(3);
        let key = passkey(&p);
        let account = AccountId32::from(account_of(&key));
        let first = remark(0);
        let proof = assertion(&p, &challenge(&account, 0, &first), ORIGIN);
        assert_ok!(Keys::open(
            RuntimeOrigin::none(),
            key,
            Box::new(first),
            proof
        ));
        assert_eq!(keys_of(&account), vec![key]);

        let call = remark(1);
        let phished = assertion(&p, &challenge(&account, 1, &call), "https://evil.example");
        assert_noop!(
            Keys::act(
                RuntimeOrigin::none(),
                account.clone(),
                key,
                Box::new(call.clone()),
                1,
                phished
            ),
            Error::<Test>::BadProof
        );
        let proof = assertion(&p, &challenge(&account, 1, &call), ORIGIN);
        assert_ok!(Keys::act(
            RuntimeOrigin::none(),
            account,
            key,
            Box::new(call),
            1,
            proof
        ));
        assert_eq!(last_result(), Some(Ok(())));
    });
}

#[test]
fn the_list_of_sites_is_kept_by_root() {
    new_test_ext().execute_with(|| {
        let account = open_ed(1);
        assert!(Keys::set_policy(RuntimeOrigin::signed(account), vec![], vec![]).is_err());
        assert_ok!(Keys::set_policy(
            RuntimeOrigin::root(),
            vec![b"localhost".to_vec()],
            vec![b"http://localhost:8787".to_vec()]
        ));
        assert_noop!(
            Keys::set_policy(RuntimeOrigin::root(), vec![vec![0u8]; 5], vec![]),
            Error::<Test>::PolicyTooLong
        );
    });
}

// ------------------------------------------------- the app's own bytes

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn vectors() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../crates/capita-keys/tests/vectors.json"
    ))
    .unwrap()
}

fn app_call(v: &serde_json::Value, kind: &str, name: &str) -> RuntimeCall {
    let bytes = hex(v[kind][name].as_str().unwrap());
    RuntimeCall::decode(&mut bytes.as_slice()).unwrap()
}

#[test]
fn the_test_chain_has_the_audience_of_the_vectors() {
    new_test_ext().execute_with(|| {
        assert_eq!(
            hex(vectors()["audience"].as_str().unwrap()),
            Keys::audience().0.to_vec()
        );
    });
}

#[test]
fn transactions_the_app_made_run() {
    new_test_ext().execute_with(|| {
        let v = vectors();
        for kind in ["ed25519", "passkey", "hardware"] {
            for name in ["open_call", "act_call"] {
                let call = app_call(&v, kind, name);
                assert_eq!(validate(&call), Ok(()), "{kind} {name}");
                assert_ok!(call.dispatch(RuntimeOrigin::none()));
                assert_eq!(last_result(), Some(Ok(())), "{kind} {name}");
            }
            let account = AccountId32::new(
                hex(v[kind]["account"].as_str().unwrap())
                    .try_into()
                    .unwrap(),
            );
            assert_eq!(Nonce::<Test>::get(&account), 2, "{kind}");
        }
        let add = app_call(&v, "ed25519", "add_act_call");
        assert_eq!(validate(&add), Ok(()));
        assert_ok!(add.dispatch(RuntimeOrigin::none()));
        assert_eq!(last_result(), Some(Ok(())));
        let added = DeviceKey::Ed25519(
            hex(v["ed25519"]["add_public"].as_str().unwrap())
                .try_into()
                .unwrap(),
        );
        let account = AccountId32::new(
            hex(v["ed25519"]["account"].as_str().unwrap())
                .try_into()
                .unwrap(),
        );
        assert_eq!(KeyOwner::<Test>::get(added), Some(account));
    });
}

#[test]
fn a_phone_hardware_key_links_a_laptop_passkey() {
    new_test_ext().execute_with(|| {
        let v = vectors();
        for name in ["open_call", "act_call", "add_act_call"] {
            let call = app_call(&v, "hardware", name);
            assert_eq!(validate(&call), Ok(()), "{name}");
            assert_ok!(call.dispatch(RuntimeOrigin::none()));
            assert_eq!(last_result(), Some(Ok(())), "{name}");
        }
        let account = AccountId32::new(
            hex(v["hardware"]["account"].as_str().unwrap())
                .try_into()
                .unwrap(),
        );
        let phone = DeviceKey::Hardware(
            hex(v["hardware"]["public"].as_str().unwrap())
                .try_into()
                .unwrap(),
        );
        let laptop = DeviceKey::Passkey(
            hex(v["hardware"]["add_passkey"].as_str().unwrap())
                .try_into()
                .unwrap(),
        );
        // The phone's key came first: it is the home key, the laptop linked.
        assert_eq!(keys_of(&account), vec![phone, laptop]);
        assert_eq!(KeyOwner::<Test>::get(laptop), Some(account.clone()));
        assert_eq!(Nonce::<Test>::get(&account), 3);
    });
}
