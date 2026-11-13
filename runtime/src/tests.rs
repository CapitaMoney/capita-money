// Capita Money – runtime of the test chain
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

//! The runtime as a whole, natively: genesis presets, real signed
//! transactions through every check, and the two Duniter pallets in step.

use crate::{
    configs::NoWebOfTrust, genesis_config_presets, nonce, AccountId, BlockNumber, Capita,
    Executive, Header, Identity, Membership, Nonce, Runtime, RuntimeCall, RuntimeGenesisConfig,
    RuntimeOrigin, Signature, SignedPayload, System, TxExtension, UncheckedExtrinsic,
    SLOT_DURATION,
};
use capita_core::attributes::{Attestation, VerificationLevel};
use capita_core::types::{AccountId as CoreId, PersonId, TierId};
use codec::Encode;
use frame_support::{assert_noop, assert_ok};
use pallet_identity::{IdtyName, IdtyStatus};
use serde_json::Value;
use sp_consensus_aura::{Slot, AURA_ENGINE_ID};
use sp_keyring::Sr25519Keyring::{self, Alice, Bob, Charlie, Dave};
use sp_runtime::traits::Dispatchable as _;
use sp_runtime::{
    generic::Era,
    traits::Header as _,
    transaction_validity::{InvalidTransaction, TransactionValidityError},
    Digest, DigestItem, DispatchError, MultiAddress,
};

/// Start of the test clock, seconds since the Unix epoch.
const T0: u64 = 1_800_000_000;

/// Lays `patch` over `base`, the way the node lays a preset over the default.
fn merge(base: &mut Value, patch: Value) {
    match (base, patch) {
        (Value::Object(base), Value::Object(patch)) => {
            for (key, value) in patch {
                merge(base.entry(key).or_insert(Value::Null), value);
            }
        }
        (base, patch) => *base = patch,
    }
}

/// The state a chain starts with under `preset`.
fn new_ext(preset: &str) -> sp_io::TestExternalities {
    let patch = genesis_config_presets::get_preset(&preset.into()).expect("preset exists");
    let mut config = serde_json::to_value(RuntimeGenesisConfig::default()).unwrap();
    merge(&mut config, serde_json::from_slice(&patch).unwrap());
    let mut ext = sp_io::TestExternalities::default();
    ext.execute_with(|| {
        frame_support::genesis_builder_helper::build_state::<RuntimeGenesisConfig>(
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
    });
    ext
}

fn dev() -> sp_io::TestExternalities {
    new_ext(sp_genesis_builder::DEV_RUNTIME_PRESET)
}

/// Opens block `number` at `secs` after `T0`, the way a block author does:
/// header with the Aura slot, then the time as first transaction.
fn start_block(number: BlockNumber, secs: u64) {
    let now = (T0 + secs) * 1_000;
    let slot = Slot::from(now / SLOT_DURATION);
    let digest = Digest {
        logs: vec![DigestItem::PreRuntime(AURA_ENGINE_ID, slot.encode())],
    };
    let header = Header::new(
        number,
        Default::default(),
        Default::default(),
        System::parent_hash(),
        digest,
    );
    Executive::initialize_block(&header);
    let set_time = RuntimeCall::Timestamp(pallet_timestamp::Call::set { now });
    assert_ok!(Executive::apply_extrinsic(UncheckedExtrinsic::new_bare(set_time)).unwrap());
}

fn end_block() {
    Executive::finalize_block();
    check();
}

/// The invariants of the rules hold on the whole state.
fn check() {
    #[cfg(feature = "try-runtime")]
    assert_eq!(Capita::do_try_state(), Ok(()));
}

/// `call`, signed by `who` as that account's transaction number `nonce`.
fn signed(who: Sr25519Keyring, nonce: Nonce, call: RuntimeCall) -> UncheckedExtrinsic {
    let extension: TxExtension = (
        frame_system::AuthorizeCall::<Runtime>::new(),
        frame_system::CheckNonZeroSender::<Runtime>::new(),
        frame_system::CheckSpecVersion::<Runtime>::new(),
        frame_system::CheckTxVersion::<Runtime>::new(),
        frame_system::CheckGenesis::<Runtime>::new(),
        frame_system::CheckEra::<Runtime>::from(Era::Immortal),
        nonce::CheckNonce::<Runtime>(nonce),
        frame_system::CheckWeight::<Runtime>::new(),
        frame_system::WeightReclaim::<Runtime>::new(),
    );
    let payload = SignedPayload::new(call.clone(), extension.clone()).unwrap();
    let signature = payload.using_encoded(|bytes| who.sign(bytes));
    UncheckedExtrinsic::new_signed(
        call,
        MultiAddress::Id(who.to_account_id()),
        Signature::Sr25519(signature),
        extension,
    )
}

/// Runs a signed transaction inside the open block; fails the test unless
/// the chain accepts it.
fn run(
    who: Sr25519Keyring,
    nonce: Nonce,
    call: impl Into<RuntimeCall>,
) -> sp_runtime::DispatchResult {
    Executive::apply_extrinsic(signed(who, nonce, call.into()))
        .expect("the chain accepts the transaction")
}

/// A call with the authority of root, sent by the holder of the sudo key.
fn sudo(nonce: Nonce, call: impl Into<RuntimeCall>) {
    let call = pallet_sudo::Call::sudo {
        call: Box::new(call.into()),
    };
    assert_ok!(run(Alice, nonce, call));
}

fn core_id(who: Sr25519Keyring) -> CoreId {
    Capita::id(who.to_account_id())
}

fn balance(who: Sr25519Keyring) -> i64 {
    pallet_capita::Balances::<Runtime>::get(core_id(who)).cents()
}

fn status(idty_index: u32) -> IdtyStatus {
    Identity::identity(idty_index)
        .expect("identity exists")
        .status
}

#[test]
fn every_preset_builds_a_consistent_state() {
    for (preset, members) in [
        (sp_genesis_builder::DEV_RUNTIME_PRESET, vec![Alice]),
        (
            sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET,
            vec![Alice, Bob],
        ),
    ] {
        new_ext(preset).execute_with(|| {
            assert_eq!(
                pallet_sudo::Key::<Runtime>::get(),
                Some(Alice.to_account_id())
            );
            assert_eq!(
                pallet_aura::Authorities::<Runtime>::get().len(),
                members.len()
            );
            // the rules are in force from the first block
            assert_eq!(pallet_capita::ParamsHistory::<Runtime>::get().len(), 1);
            // members: an identity, a membership, and the two agree
            for (who, index) in members.iter().zip(1u32..) {
                let account: AccountId = who.to_account_id();
                assert_eq!(Identity::identity_index_of(&account), Some(index));
                assert_eq!(status(index), IdtyStatus::Member);
                assert!(Membership::is_member(&index));
            }
            assert_eq!(Identity::identities_count(), members.len() as u32);
            check();
        });
    }
    assert!(genesis_config_presets::get_preset(&"no such preset".into()).is_none());
}

#[test]
fn a_new_key_sends_transactions_without_fee_and_each_runs_once() {
    dev().execute_with(|| {
        start_block(1, 0);
        // Charlie: no identity, no funds, never seen by the chain.
        assert_eq!(System::account_nonce(Charlie.to_account_id()), 0);
        let open = signed(Charlie, 0, pallet_capita::Call::open {}.into());
        assert_ok!(Executive::apply_extrinsic(open.clone()).unwrap());
        assert!(pallet_capita::Accounts::<Runtime>::contains_key(core_id(
            Charlie
        )));
        assert_eq!(System::account_nonce(Charlie.to_account_id()), 1);
        // the same transaction a second time: refused, not run
        assert_eq!(
            Executive::apply_extrinsic(open),
            Err(TransactionValidityError::Invalid(InvalidTransaction::Stale))
        );
        // a transaction that skips a number waits; it does not run early
        let early = signed(Charlie, 5, pallet_capita::Call::claim {}.into());
        assert_eq!(
            Executive::apply_extrinsic(early),
            Err(TransactionValidityError::Invalid(
                InvalidTransaction::Future
            ))
        );
        // a signature by another key is refused
        let mut forged = signed(Charlie, 1, pallet_capita::Call::claim {}.into());
        forged.function = pallet_capita::Call::close {}.into();
        assert_eq!(
            Executive::apply_extrinsic(forged),
            Err(TransactionValidityError::Invalid(
                InvalidTransaction::BadProof
            ))
        );
        end_block();
    });
}

#[test]
fn the_rules_run_on_the_chain_from_opening_to_claim() {
    dev().execute_with(|| {
        start_block(1, 0);
        // Bob opens an account and becomes the verification service.
        assert_ok!(run(Bob, 0, pallet_capita::Call::open {}));
        sudo(
            0,
            pallet_capita::Call::set_service_account {
                account: Bob.to_account_id(),
            },
        );
        // Charlie opens, pays the verification with the start grant …
        assert_ok!(run(Charlie, 0, pallet_capita::Call::open {}));
        assert_ok!(run(Charlie, 1, pallet_capita::Call::pay_verification {}));
        // Bob: own start grant (still earmarked) plus the payment
        assert_eq!(balance(Bob), 200);
        // … the identity service (here: root) attests level 1 …
        let attestation = Attestation {
            person: PersonId([7; 32]),
            level: VerificationLevel(1),
            residence: None,
            attributes: vec![],
            issued_at: Capita::now(),
            expires_at: None,
        };
        sudo(
            1,
            pallet_capita::Call::attest {
                account: Charlie.to_account_id(),
                attestation: Some(attestation),
            },
        );
        // … and Charlie moves to the ladder step of that level.
        assert_ok!(run(
            Charlie,
            2,
            pallet_capita::Call::set_tier { tier: TierId(1) }
        ));
        // only root may attest: the same call from Charlie fails and changes nothing
        assert_noop!(
            RuntimeCall::from(pallet_capita::Call::attest {
                account: Charlie.to_account_id(),
                attestation: None
            })
            .dispatch(RuntimeOrigin::signed(Charlie.to_account_id())),
            DispatchError::BadOrigin
        );
        end_block();

        // One hour of block time later: 60 minutes at 1 cent.
        start_block(2, 3_600);
        assert_ok!(run(Charlie, 3, pallet_capita::Call::claim {}));
        assert_eq!(balance(Charlie), 60);
        // a payment, without fee
        assert_ok!(run(
            Charlie,
            4,
            pallet_capita::Call::transfer {
                to: Bob.to_account_id(),
                amount: capita_core::types::Amount::from_cents(25),
            }
        ));
        assert_eq!((balance(Charlie), balance(Bob)), (35, 225));
        assert_eq!(pallet_capita::TotalSupply::<Runtime>::get(), 260);
        end_block();
    });
}

#[test]
fn identity_and_membership_stay_in_step_without_a_web_of_trust() {
    dev().execute_with(|| {
        start_block(1, 0);
        // An identity needs an account that exists: without an open Capita
        // account Dave cannot receive one.
        let create = pallet_identity::Call::create_identity {
            owner_key: Dave.to_account_id(),
        };
        assert_eq!(
            run(Alice, 0, create.clone()),
            Err(pallet_identity::Error::<Runtime>::AccountNotExist.into())
        );
        assert_ok!(run(Dave, 0, pallet_capita::Call::open {}));
        // Alice (member since genesis) creates the identity, Dave confirms it.
        assert_ok!(run(Alice, 1, create));
        assert_eq!(status(2), IdtyStatus::Unconfirmed);
        assert_ok!(run(
            Dave,
            1,
            pallet_identity::Call::confirm_identity {
                idty_name: IdtyName::from("Dave")
            }
        ));
        assert_eq!(status(2), IdtyStatus::Unvalidated);
        assert!(!Membership::is_member(&2));
        // No call grants a membership yet (M3). Granted from inside the
        // runtime, the identity follows …
        assert_ok!(Membership::try_add_membership(2));
        assert_eq!(status(2), IdtyStatus::Member);
        // … and follows again when the membership ends,
        Membership::do_remove_membership(2, pallet_membership::MembershipRemovalReason::Expired);
        assert_eq!(status(2), IdtyStatus::NotMember);
        // and a revoked identity loses its membership.
        assert!(Membership::is_member(&1));
        Identity::do_revoke_identity(1, pallet_identity::RevocationReason::Root);
        assert_eq!(status(1), IdtyStatus::Revoked);
        assert!(!Membership::is_member(&1));
        let _ = NoWebOfTrust;
        end_block();
    });
}
