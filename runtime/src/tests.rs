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
//! transactions through every check, and identity by credentials on the two
//! Duniter pallets.

use crate::{
    configs::MembershipPeriod,
    genesis_config_presets::{
        self, dev_issuer, test_issuer, CREATOR_IDTY, DEV_ISSUER_KEY, DEV_ISSUER_SEED,
    },
    identity::{creator, derived_name, CREATOR_NAME},
    nonce, AccountId, BlockNumber, Capita, Credential, Executive, Header, Identity, Membership,
    Nonce, Runtime, RuntimeCall, RuntimeGenesisConfig, RuntimeOrigin, Signature, SignedPayload,
    System, TxExtension, UncheckedExtrinsic, SLOT_DURATION,
};
use capita_core::account::AccountStatus;
use capita_core::attributes::{Attestation, VerificationLevel};
use capita_core::types::{AccountId as CoreId, PersonId, TierId, Timestamp};
use capita_credential::encoding::{claim_bytes, revocation_bytes};
use capita_credential::types::{
    Claim, Issuer, IssuerId, IssuerStatus, PublicKey, Revocation, Serial, SubjectRef,
};
use codec::Encode;
use frame_support::{assert_noop, assert_ok};
use pallet_identity::{IdtyName, IdtyStatus};
use serde_json::Value;
use sp_consensus_aura::{Slot, AURA_ENGINE_ID};
use sp_core::Pair as _;
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

/// Leaves out the blocks up to `number`, which is opened next. Nothing may
/// be due in the blocks left out.
fn skip_to(number: BlockNumber) {
    System::set_block_number(number - 1);
}

/// The invariants of the rules and of the claims hold on the whole state,
/// and identities, memberships and claims agree.
fn check() {
    #[cfg(feature = "try-runtime")]
    {
        assert_eq!(Capita::do_try_state(), Ok(()));
        assert_eq!(Credential::do_try_state(), Ok(()));
    }
    for (idty_index, value) in pallet_identity::Identities::<Runtime>::iter() {
        // identity and membership in step
        assert_eq!(
            value.status == IdtyStatus::Member,
            Membership::is_member(&idty_index)
        );
        // a member is the creator of identities or holds a claim
        if value.status == IdtyStatus::Member && idty_index != CREATOR_IDTY {
            assert!(
                pallet_capita_credential::Credentials::<Runtime>::contains_key(&value.owner_key)
            );
        }
    }
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

/// The verification level the rules hold for `who` right now.
fn level(who: Sr25519Keyring) -> u8 {
    let state = pallet_capita::Accounts::<Runtime>::get(core_id(who)).expect("account is open");
    capita_core::issuance::level_at(&state, Capita::now()).0
}

/// A claim of the identity service of the test chain for `who`, about a
/// person known to it under `who`'s public key as pseudonym: `level`, issued
/// now, valid for `days`. Its number is `serial` in every byte.
fn claim(who: Sr25519Keyring, level: u8, serial: u8, days: u64) -> Claim {
    let now = Credential::now();
    Claim {
        issuer: IssuerId(0),
        serial: Serial([serial; 16]),
        subject: core_id(who),
        subject_ref: SubjectRef(who.public().0),
        level: VerificationLevel(level),
        residence: None,
        attributes: vec![],
        issued_at: now,
        expires_at: Some(Timestamp::from_secs(now.secs() + days * 86_400)),
    }
}

fn issuer_key() -> sp_core::ed25519::Pair {
    sp_core::ed25519::Pair::from_seed(&DEV_ISSUER_SEED)
}

/// The call by which the subject of `claim` presents it, with the signature
/// of the identity service of the test chain.
fn present(claim: Claim, name: Option<&str>) -> pallet_capita_credential::Call<Runtime> {
    let bytes = claim_bytes(&Credential::audience(), &claim);
    pallet_capita_credential::Call::present {
        claim,
        signature: capita_credential::types::Signature::Ed25519(issuer_key().sign(&bytes).0),
        name: name.map(|name| name.as_bytes().to_vec()),
    }
}

/// Block 1: Bob opens an account and becomes the verification service;
/// Charlie opens one, pays the verification and presents a claim of level 1
/// that is valid for two years. Charlie is identity 2.
fn charlie_is_verified() {
    start_block(1, 0);
    assert_ok!(run(Bob, 0, pallet_capita::Call::open {}));
    sudo(
        0,
        pallet_capita::Call::set_service_account {
            account: Bob.to_account_id(),
        },
    );
    assert_ok!(run(Charlie, 0, pallet_capita::Call::open {}));
    assert_ok!(run(Charlie, 1, pallet_capita::Call::pay_verification {}));
    assert_ok!(run(
        Charlie,
        2,
        present(claim(Charlie, 1, 1, 730), Some("Charlie"))
    ));
    end_block();
}

#[test]
fn every_preset_builds_a_consistent_state() {
    // each preset starts with a rule set of the rules files
    use capita_rules::{alpha, dev_fast};
    for (preset, authors, rules, top_level) in [
        (
            sp_genesis_builder::DEV_RUNTIME_PRESET,
            1,
            dev_fast::params(),
            dev_fast::TOP_LEVEL,
        ),
        (
            sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET,
            2,
            alpha::params(),
            alpha::TOP_LEVEL,
        ),
    ] {
        new_ext(preset).execute_with(|| {
            assert_eq!(
                pallet_sudo::Key::<Runtime>::get(),
                Some(Alice.to_account_id())
            );
            assert_eq!(pallet_aura::Authorities::<Runtime>::get().len(), authors);
            // the rules are in force from the first block
            let history = pallet_capita::ParamsHistory::<Runtime>::get();
            assert_eq!(history.len(), 1);
            assert_eq!(history.first(), rules.as_ref());
            // one identity: the creator of identities, a member for good
            assert_eq!(Identity::identities_count(), 1);
            assert_eq!(Identity::identity_index_of(creator()), Some(CREATOR_IDTY));
            assert_eq!(
                Identity::identity_by_did(IdtyName::from(CREATOR_NAME)),
                Some(CREATOR_IDTY)
            );
            assert_eq!(status(CREATOR_IDTY), IdtyStatus::Member);
            assert_eq!(
                Membership::membership(CREATOR_IDTY).map(|data| data.expire_on),
                Some(BlockNumber::MAX)
            );
            // one identity service: that of the test chain
            assert_eq!(
                pallet_capita_credential::Issuers::<Runtime>::get(IssuerId(0)),
                Some(test_issuer(top_level))
            );
            assert_eq!(pallet_capita_credential::NextIssuer::<Runtime>::get(), 1);
            check();
        });
    }
    assert!(genesis_config_presets::get_preset(&"no such preset".into()).is_none());
}

#[test]
fn the_key_of_the_test_identity_service_is_the_published_one() {
    assert_eq!(DEV_ISSUER_KEY, PublicKey::Ed25519(issuer_key().public().0));
    assert_eq!(dev_issuer().validate(), Ok(()));
    assert!(dev_issuer().did.starts_with(b"did:key:z6Mk"));
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

/// The rules themselves (`rules/alpha.toml`, preset `local_testnet`), with
/// no number expected: whatever the file says, a verified person on the
/// highest step is issued what that step gives over the time a claim
/// reaches back, and an account without verification what the first step
/// gives.
#[test]
fn the_alpha_rules_run_on_the_chain_whatever_their_numbers() {
    let rules = capita_rules::alpha::params().expect("checked by the build");
    let (first, top) = (rules.ladder[0], rules.ladder[rules.ladder.len() - 1]);
    let elapsed = rules.caps.period.secs();
    let counted = capita_core::types::Duration::from_secs(elapsed.min(rules.claim_window.secs()));
    let due = |step: &capita_core::params::LadderStep| {
        let cents = step.rate.accrued(counted).unwrap().cents();
        i64::try_from(cents.min(rules.caps.global_cap.cents())).unwrap()
    };
    let grant = i64::try_from(rules.start_grant.cents()).unwrap();

    new_ext(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET).execute_with(|| {
        start_block(1, 0);
        assert_ok!(run(Bob, 0, pallet_capita::Call::open {}));
        sudo(
            0,
            pallet_capita::Call::set_service_account {
                account: Bob.to_account_id(),
            },
        );
        // Charlie: verified at the highest level of the ladder, on its top step
        assert_ok!(run(Charlie, 0, pallet_capita::Call::open {}));
        assert_ok!(run(Charlie, 1, pallet_capita::Call::pay_verification {}));
        let level = capita_rules::alpha::TOP_LEVEL.0;
        assert_ok!(run(
            Charlie,
            2,
            present(claim(Charlie, level, 1, 730), None)
        ));
        assert_ok!(run(
            Charlie,
            3,
            pallet_capita::Call::set_tier { tier: top.id }
        ));
        // Dave: an account and nothing else
        assert_ok!(run(Dave, 0, pallet_capita::Call::open {}));
        assert_eq!((balance(Charlie), balance(Dave)), (0, grant));
        end_block();

        // one cap period later
        start_block(2, elapsed);
        assert_ok!(run(Charlie, 4, pallet_capita::Call::claim {}));
        assert_ok!(run(Dave, 1, pallet_capita::Call::claim {}));
        assert_eq!(balance(Charlie), due(&top));
        assert_eq!(balance(Dave), grant + due(&first));
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
fn a_person_becomes_a_member_by_presenting_a_claim() {
    dev().execute_with(|| {
        charlie_is_verified();
        // an identity under the chosen name, a membership of one period
        let charlie: AccountId = Charlie.to_account_id();
        assert_eq!(Identity::identity_index_of(&charlie), Some(2));
        assert_eq!(
            Identity::identity_by_did(IdtyName::from("Charlie")),
            Some(2)
        );
        assert_eq!(status(2), IdtyStatus::Member);
        assert_eq!(
            Membership::membership(2).map(|data| data.expire_on),
            Some(1 + MembershipPeriod::get())
        );
        // the rules know a person and a level; nothing else about Charlie
        let state = pallet_capita::Accounts::<Runtime>::get(core_id(Charlie)).unwrap();
        assert!(state.person.is_some());
        assert_eq!(level(Charlie), 1);

        // the level opens the ladder step: one hour at 1 cent per minute
        start_block(2, 6);
        assert_ok!(run(
            Charlie,
            3,
            pallet_capita::Call::set_tier { tier: TierId(1) }
        ));
        end_block();
        start_block(3, 3_606);
        assert_ok!(run(Charlie, 4, pallet_capita::Call::claim {}));
        assert_eq!(balance(Charlie), 60);

        // Dave: the claim of somebody else is of no use, …
        assert_ok!(run(Dave, 0, pallet_capita::Call::open {}));
        assert_eq!(
            run(Dave, 1, present(claim(Charlie, 1, 2, 730), None)),
            Err(pallet_capita_credential::Error::<Runtime>::NotSubject.into())
        );
        // … one made out to Dave about the person Charlie is none either, …
        let mut second = claim(Dave, 1, 3, 730);
        second.subject_ref = SubjectRef(Charlie.public().0);
        assert_eq!(
            run(Dave, 2, present(second, None)),
            Err(pallet_capita::Error::<Runtime>::DuplicatePerson.into())
        );
        // … and no member creates an identity by hand, as Duniter's do.
        let create = pallet_identity::Call::create_identity {
            owner_key: Dave.to_account_id(),
        };
        assert_eq!(
            run(Charlie, 5, create.clone()),
            Err(DispatchError::Other(
                "identities are created by presenting a claim"
            ))
        );
        assert_eq!(
            run(Dave, 3, create),
            Err(pallet_identity::Error::<Runtime>::IdtyIndexNotFound.into())
        );
        assert_eq!(Identity::identity_index_of(Dave.to_account_id()), None);

        // With a claim of its own, and no name chosen, Dave is identity 3
        // under a name derived from the key.
        assert_ok!(run(Dave, 4, present(claim(Dave, 2, 4, 30), None)));
        let name = derived_name(&Dave.to_account_id());
        assert_eq!(name.0.len(), 42);
        assert!(name.0.starts_with(b"0x"));
        assert_eq!(Identity::identity_by_did(name.clone()), Some(3));
        assert_eq!(level(Dave), 2);
        // such names are reserved, and a name is taken once
        let taken = String::from_utf8(derived_name(&Bob.to_account_id()).0).unwrap();
        assert_eq!(
            run(Bob, 1, present(claim(Bob, 1, 5, 30), Some(&taken))),
            Err(DispatchError::Other("names starting with 0x are reserved"))
        );
        assert_eq!(
            run(Bob, 2, present(claim(Bob, 1, 5, 30), Some("Charlie"))),
            Err(pallet_identity::Error::<Runtime>::IdtyNameAlreadyExist.into())
        );
        assert_eq!(
            run(Bob, 3, present(claim(Bob, 1, 5, 30), Some("no spaces"))),
            Err(pallet_identity::Error::<Runtime>::IdtyNameInvalid.into())
        );
        assert_eq!(Identity::identity_index_of(Bob.to_account_id()), None);
        assert_eq!(level(Bob), 0);
        end_block();
    });
}

#[test]
fn a_claim_needs_an_open_account() {
    dev().execute_with(|| {
        start_block(1, 0);
        assert_eq!(
            run(Dave, 0, present(claim(Dave, 1, 1, 30), None)),
            Err(pallet_capita::Error::<Runtime>::NoAccount.into())
        );
        assert_eq!(Identity::identities_count(), 1);
        end_block();
    });
}

#[test]
fn a_membership_that_runs_out_takes_the_level_until_renewed() {
    dev().execute_with(|| {
        charlie_is_verified();
        // One membership period later, to the block: Duniter's membership
        // pallet ends the membership, the identity follows, and the rules
        // count Charlie as unverified although the claim still holds.
        let period = u64::from(MembershipPeriod::get());
        skip_to(1 + MembershipPeriod::get());
        start_block(1 + MembershipPeriod::get(), period * 6);
        assert!(!Membership::is_member(&2));
        assert_eq!(status(2), IdtyStatus::NotMember);
        assert_eq!(level(Charlie), 0);
        assert!(
            pallet_capita_credential::Credentials::<Runtime>::contains_key(Charlie.to_account_id())
        );
        // Charlie renews with the claim held: no new signature needed.
        assert_ok!(run(Charlie, 3, pallet_capita_credential::Call::renew {}));
        assert!(Membership::is_member(&2));
        assert_eq!(status(2), IdtyStatus::Member);
        assert_eq!(level(Charlie), 1);
        end_block();

        // A renewal before the membership ran out extends it.
        start_block(2 + MembershipPeriod::get(), period * 6 + 6);
        assert_ok!(run(Charlie, 4, pallet_capita_credential::Call::renew {}));
        assert_eq!(
            Membership::membership(2).map(|data| data.expire_on),
            Some(2 + 2 * MembershipPeriod::get())
        );
        end_block();

        // Two years after its issue the claim itself has run out: the rules
        // drop the level on their own, no renewal helps, and anybody may
        // clear the claim away, which ends the membership.
        start_block(3 + MembershipPeriod::get(), 730 * 86_400);
        assert_eq!(level(Charlie), 0);
        assert_eq!(
            run(Charlie, 5, pallet_capita_credential::Call::renew {}),
            Err(pallet_capita_credential::Error::<Runtime>::NoLongerCounts.into())
        );
        assert!(Membership::is_member(&2));
        assert_ok!(run(
            Dave,
            0,
            pallet_capita_credential::Call::drop_invalid {
                account: Charlie.to_account_id()
            }
        ));
        assert!(!Membership::is_member(&2));
        assert_eq!(status(2), IdtyStatus::NotMember);
        // A new claim makes the same identity a member again.
        assert_ok!(run(Charlie, 6, present(claim(Charlie, 2, 2, 30), None)));
        assert_eq!(
            Identity::identity_index_of(Charlie.to_account_id()),
            Some(2)
        );
        assert_eq!(status(2), IdtyStatus::Member);
        assert_eq!(level(Charlie), 2);
        end_block();
    });
}

#[test]
fn an_identity_service_takes_a_claim_back_or_is_struck_off() {
    dev().execute_with(|| {
        charlie_is_verified();
        start_block(2, 6);
        assert_ok!(run(Dave, 0, pallet_capita::Call::open {}));
        assert_ok!(run(Dave, 1, present(claim(Dave, 2, 2, 30), None)));

        // The service takes the claim of Charlie back; anybody hands the
        // revocation in.
        let revocation = Revocation {
            issuer: IssuerId(0),
            serial: Serial([1; 16]),
        };
        let bytes = revocation_bytes(&Credential::audience(), &revocation);
        let signature = capita_credential::types::Signature::Ed25519(issuer_key().sign(&bytes).0);
        assert_ok!(run(
            Dave,
            2,
            pallet_capita_credential::Call::revoke {
                revocation,
                signature
            }
        ));
        assert!(!Membership::is_member(&2));
        assert_eq!(status(2), IdtyStatus::NotMember);
        assert_eq!(level(Charlie), 0);
        // the account itself stays as it is
        let state = pallet_capita::Accounts::<Runtime>::get(core_id(Charlie)).unwrap();
        assert_eq!(state.status, AccountStatus::Active);
        assert_eq!(
            run(Charlie, 3, pallet_capita_credential::Call::renew {}),
            Err(pallet_capita_credential::Error::<Runtime>::NoCredential.into())
        );

        // Root strikes the whole service off the list: what it stated stops
        // counting as soon as somebody clears each claim away.
        let drop_dave = pallet_capita_credential::Call::drop_invalid {
            account: Dave.to_account_id(),
        };
        assert_eq!(
            run(Charlie, 4, drop_dave.clone()),
            Err(pallet_capita_credential::Error::<Runtime>::StillCounts.into())
        );
        // only root keeps the list
        let struck_off = pallet_capita_credential::Call::update_issuer {
            id: IssuerId(0),
            issuer: Issuer {
                status: IssuerStatus::Revoked,
                ..dev_issuer()
            },
        };
        assert_noop!(
            RuntimeCall::from(struck_off.clone())
                .dispatch(RuntimeOrigin::signed(Charlie.to_account_id())),
            DispatchError::BadOrigin
        );
        sudo(1, struck_off);
        assert_ok!(run(Charlie, 5, drop_dave));
        assert!(!Membership::is_member(&3));
        assert_eq!(level(Dave), 0);
        assert_eq!(
            run(Charlie, 6, present(claim(Charlie, 1, 3, 30), None)),
            Err(pallet_capita_credential::Error::<Runtime>::IssuerNotActive.into())
        );
        end_block();
    });
}

#[test]
fn a_revoked_identity_freezes_the_account_and_no_identity_moves_to_another_key() {
    dev().execute_with(|| {
        charlie_is_verified();
        start_block(2, 6);
        let genesis_hash = System::block_hash(0);

        // Moving the identity to the key of Dave: Dave agrees by signature,
        // the chain refuses – account and claim would stay behind.
        let payload = (
            pallet_identity::NEW_OWNER_KEY_PAYLOAD_PREFIX,
            pallet_identity::IdtyIndexAccountIdPayload {
                genesis_hash: &genesis_hash,
                idty_index: 2u32,
                old_owner_key: &Charlie.to_account_id(),
            },
        );
        let agreed = payload.using_encoded(|bytes| Dave.sign(bytes));
        assert_eq!(
            run(
                Charlie,
                3,
                pallet_identity::Call::change_owner_key {
                    new_key: Dave.to_account_id(),
                    new_key_sig: Signature::Sr25519(agreed),
                }
            ),
            Err(DispatchError::Other(
                "an identity cannot move to another key"
            ))
        );
        assert_eq!(
            Identity::identity_index_of(Charlie.to_account_id()),
            Some(2)
        );
        assert_eq!(Identity::identity_index_of(Dave.to_account_id()), None);

        // Charlie revokes the identity with a signature of the key – what
        // one does when a key is no longer safe. Anybody hands it in.
        let payload = (
            pallet_identity::REVOCATION_PAYLOAD_PREFIX,
            pallet_identity::RevocationPayload {
                genesis_hash,
                idty_index: 2u32,
            },
        );
        let revoked = payload.using_encoded(|bytes| Charlie.sign(bytes));
        assert_ok!(run(Dave, 0, pallet_capita::Call::open {}));
        assert_ok!(run(
            Dave,
            1,
            pallet_identity::Call::revoke_identity {
                idty_index: 2,
                revocation_key: Charlie.to_account_id(),
                revocation_sig: Signature::Sr25519(revoked),
            }
        ));
        assert_eq!(status(2), IdtyStatus::Revoked);
        assert!(!Membership::is_member(&2));
        // the claim counts no longer and the account is frozen: no claim,
        // no payment
        assert!(
            !pallet_capita_credential::Credentials::<Runtime>::contains_key(
                Charlie.to_account_id()
            )
        );
        let state = pallet_capita::Accounts::<Runtime>::get(core_id(Charlie)).unwrap();
        assert_eq!(state.status, AccountStatus::Frozen);
        assert_eq!(
            run(Charlie, 4, pallet_capita::Call::claim {}),
            Err(pallet_capita::Error::<Runtime>::AccountNotActive.into())
        );
        // A revoked identity is final: a new claim for the same key is
        // refused. Offene Frage B5: moving the account to a new key.
        assert_eq!(
            run(Charlie, 5, present(claim(Charlie, 1, 2, 30), None)),
            Err(DispatchError::Other("identity revoked"))
        );
        // root can reopen the account by hand
        sudo(
            1,
            pallet_capita::Call::set_status {
                account: Charlie.to_account_id(),
                status: AccountStatus::Active,
            },
        );
        assert_eq!(level(Charlie), 0);
        end_block();
    });
}
