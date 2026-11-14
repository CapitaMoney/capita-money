// Capita Money – pallet-capita-credential
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

//! A minimal runtime for the tests: system, timestamp and this pallet. The
//! identities of the chain and the monetary rules are stand-ins that record
//! what the pallet tells them.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use capita_core::attributes::{Attestation, AttrKey, VerificationLevel};
use capita_core::types::{AccountId as CoreId, Duration, PersonId, Timestamp as Instant};
use capita_credential::encoding::{claim_bytes, revocation_bytes};
use capita_credential::types::{
    Claim, Issuer, IssuerId, IssuerStatus, PublicKey, Realm, Revocation, Serial, Signature,
    SubjectRef,
};
use frame_support::derive_impl;
use frame_support::pallet_prelude::{DispatchError, DispatchResult};
use frame_system::EnsureRoot;
use sp_core::Pair;
use sp_runtime::traits::IdentityLookup;
use sp_runtime::{AccountId32, BuildStorage};

use crate as pallet_capita_credential;

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Timestamp: pallet_timestamp,
        Credential: pallet_capita_credential,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = AccountId32;
    type Lookup = IdentityLookup<Self::AccountId>;
}

#[derive_impl(pallet_timestamp::config_preludes::TestDefaultConfig)]
impl pallet_timestamp::Config for Test {}

impl pallet_capita_credential::Config for Test {
    type Time = Timestamp;
    type RegistryOrigin = EnsureRoot<AccountId32>;
    type Memberships = Identities;
    type Rules = RulesStub;
    type MoveDelay = MoveDelay;
}

frame_support::parameter_types! {
    /// Waiting time of a move in the tests: seven days, or what a test sets.
    pub storage MoveDelay: u64 = 7 * DAY;
}

/// What the stand-ins hold.
#[derive(Default)]
pub struct World {
    /// Identity of an account: its name.
    pub identities: BTreeMap<AccountId32, Vec<u8>>,
    /// Accounts that are members, with the number of grants so far.
    pub members: BTreeMap<AccountId32, u32>,
    /// Accounts whose identity was revoked: no membership any more.
    pub revoked: BTreeSet<AccountId32>,
    /// Accounts opened with the rules.
    pub open: BTreeSet<AccountId32>,
    /// What the rules were last told about an account.
    pub attested: BTreeMap<AccountId32, Attestation>,
    /// The account each person holds.
    pub persons: BTreeMap<PersonId, AccountId32>,
    /// Accounts frozen by the rules.
    pub frozen: BTreeSet<AccountId32>,
    /// Accounts whose identity ended because their person moved away.
    pub retired: BTreeSet<AccountId32>,
    /// Accounts that moved, with where to.
    pub moved: BTreeMap<AccountId32, AccountId32>,
}

thread_local! {
    pub static WORLD: RefCell<World> = RefCell::new(World::default());
}

/// Reads or changes the state of the stand-ins.
pub fn world<R>(f: impl FnOnce(&mut World) -> R) -> R {
    WORLD.with(|world| f(&mut world.borrow_mut()))
}

/// Stand-in for the identity pallets: an account is an identity with a
/// name, and a member or not.
pub struct Identities;

impl pallet_capita_credential::Memberships<AccountId32> for Identities {
    fn grant(who: &AccountId32, name: Option<&[u8]>) -> DispatchResult {
        world(|w| {
            if w.revoked.contains(who) {
                return Err(DispatchError::Other("identity revoked"));
            }
            let name = name.map_or_else(|| b"derived".to_vec(), <[u8]>::to_vec);
            if !w.identities.contains_key(who) {
                if w.identities.values().any(|taken| *taken == name) && name != b"derived" {
                    return Err(DispatchError::Other("name taken"));
                }
                w.identities.insert(who.clone(), name);
            }
            *w.members.entry(who.clone()).or_default() += 1;
            Ok(())
        })
    }

    fn withdraw(who: &AccountId32) {
        let was_member = world(|w| w.members.remove(who).is_some());
        if was_member {
            // as the runtime does when a membership ends
            Credential::membership_ended(who);
        }
    }

    fn retire(who: &AccountId32) {
        world(|w| {
            w.members.remove(who);
            w.identities.remove(who);
            w.retired.insert(who.clone());
        });
    }
}

/// Stand-in for `pallet-capita`: one person, one account; a level per
/// account; closed accounts refuse everything.
pub struct RulesStub;

impl pallet_capita_credential::Rules<AccountId32> for RulesStub {
    fn attest(who: &AccountId32, attestation: Option<Attestation>) -> DispatchResult {
        world(|w| {
            if !w.open.contains(who) {
                return Err(DispatchError::Other("no account"));
            }
            let Some(attestation) = attestation else {
                w.attested.remove(who);
                return Ok(());
            };
            let bound = w.attested.get(who).map(|a| a.person);
            if bound.is_some_and(|person| person != attestation.person) {
                return Err(DispatchError::Other("person mismatch"));
            }
            match w.persons.get(&attestation.person) {
                Some(other) if other != who => {
                    return Err(DispatchError::Other("duplicate person"))
                }
                _ => {}
            }
            w.persons.insert(attestation.person, who.clone());
            w.attested.insert(who.clone(), attestation);
            Ok(())
        })
    }

    fn freeze(who: &AccountId32) -> DispatchResult {
        world(|w| w.frozen.insert(who.clone()));
        Ok(())
    }

    fn holder(person: &PersonId) -> Option<AccountId32> {
        world(|w| w.persons.get(person).cloned())
    }

    fn move_account(
        from: &AccountId32,
        to: &AccountId32,
        attestation: &Attestation,
    ) -> DispatchResult {
        world(|w| {
            if w.frozen.contains(from) {
                return Err(DispatchError::Other("frozen"));
            }
            if !w.open.contains(to) || w.attested.contains_key(to) {
                return Err(DispatchError::Other("target not free"));
            }
            w.attested.remove(from);
            w.open.remove(from);
            w.persons.insert(attestation.person, to.clone());
            w.attested.insert(to.clone(), attestation.clone());
            w.moved.insert(from.clone(), to.clone());
            Ok(())
        })
    }
}

/// Start of the test clock, seconds since the Unix epoch.
pub const T0: u64 = 1_800_000_000;
pub const DAY: u64 = 86_400;

pub fn acct(n: u8) -> AccountId32 {
    AccountId32::new([n; 32])
}

pub fn signed(n: u8) -> RuntimeOrigin {
    RuntimeOrigin::signed(acct(n))
}

pub fn root() -> RuntimeOrigin {
    RuntimeOrigin::root()
}

/// `T0` plus `offset` seconds.
pub fn ts(offset: u64) -> Instant {
    Instant::from_secs(T0 + offset)
}

/// Sets the block time to `T0` plus `offset` seconds.
pub fn set_time(offset: u64) {
    Timestamp::set_timestamp((T0 + offset) * 1_000);
}

/// The level the rules currently hold for account `n`.
pub fn level(n: u8) -> u8 {
    world(|w| w.attested.get(&acct(n)).map_or(0, |a| a.level.0))
}

pub fn is_member(n: u8) -> bool {
    world(|w| w.members.contains_key(&acct(n)))
}

/// Secret key number `n` of an issuer (Ed25519).
pub fn secret(n: u8) -> sp_core::ed25519::Pair {
    sp_core::ed25519::Pair::from_seed(&[n; 32])
}

pub fn public(n: u8) -> PublicKey {
    PublicKey::Ed25519(secret(n).public().0)
}

/// Issuer 0 of the tests, signing with key 1: level up to 3, residence,
/// attribute 1, claims of two years at most.
pub fn issuer() -> Issuer {
    Issuer {
        did: b"did:web:id.example.org".to_vec(),
        keys: vec![public(1)],
        realm: Realm(1),
        max_level: VerificationLevel(3),
        residence: true,
        attributes: vec![AttrKey(1)],
        max_validity: Some(Duration::from_secs(2 * 365 * DAY)),
        status: IssuerStatus::Active,
    }
}

/// A claim of issuer 0 for account `n` about person `n`: `level`, issued
/// now, valid one year. Its number is `serial` in every byte.
pub fn claim(n: u8, level: u8, serial: u8) -> Claim {
    Claim {
        issuer: IssuerId(0),
        serial: Serial([serial; 16]),
        subject: CoreId([n; 32]),
        subject_ref: SubjectRef([n; 32]),
        replaces: None,
        level: VerificationLevel(level),
        residence: None,
        attributes: vec![],
        issued_at: Credential::now(),
        expires_at: Some(Instant::from_secs(Credential::now().secs() + 365 * DAY)),
    }
}

/// The signature of issuer key `key` under `claim`, for this chain.
pub fn sign(key: u8, claim: &Claim) -> Signature {
    let bytes = claim_bytes(&Credential::audience(), claim);
    Signature::Ed25519(secret(key).sign(&bytes).0)
}

/// The signature of issuer key `key` under a revocation, for this chain.
pub fn sign_revocation(key: u8, revocation: &Revocation) -> Signature {
    let bytes = revocation_bytes(&Credential::audience(), revocation);
    Signature::Ed25519(secret(key).sign(&bytes).0)
}

/// A chain at block 1, time `T0`, with [`issuer`] as issuer 0 and the
/// accounts 1 to 9 opened with the rules.
pub fn new_test_ext() -> sp_io::TestExternalities {
    WORLD.with(|world| *world.borrow_mut() = World::default());
    world(|w| w.open = (1..=9).map(acct).collect());
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    pallet_capita_credential::GenesisConfig::<Test> {
        issuers: vec![issuer()],
        ..Default::default()
    }
    .assimilate_storage(&mut storage)
    .unwrap();
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| {
        System::set_block_number(1);
        set_time(0);
    });
    ext
}

/// Dispatches a call of the pallet the way a block does: inside a storage
/// transaction, so that a failed call leaves no trace on chain. (The
/// stand-ins live outside the chain state and keep what they were told.)
pub fn call(
    origin: RuntimeOrigin,
    call: pallet_capita_credential::Call<Test>,
) -> sp_runtime::DispatchResult {
    use sp_runtime::traits::Dispatchable;
    RuntimeCall::Credential(call)
        .dispatch(origin)
        .map(|_| ())
        .map_err(|e| e.error)
}

/// The events of the pallet deposited so far, oldest first.
pub fn events() -> Vec<pallet_capita_credential::Event<Test>> {
    System::events()
        .into_iter()
        .filter_map(|record| match record.event {
            RuntimeEvent::Credential(event) => Some(event),
            _ => None,
        })
        .collect()
}
