// Capita Money – pallet-capita-quota
// SPDX-License-Identifier: AGPL-3.0-only

//! A minimal runtime for the tests: system and this pallet. Who is a person
//! and who a service is a list the tests write.

use std::cell::RefCell;
use std::collections::BTreeSet;

use capita_core::quota::Sender;
use frame_support::derive_impl;
use frame_support::traits::ConstU32;
use sp_runtime::traits::IdentityLookup;
use sp_runtime::{AccountId32, BuildStorage};

use crate as pallet_capita_quota;

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Quota: pallet_capita_quota,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = AccountId32;
    type Lookup = IdentityLookup<Self::AccountId>;
}

/// 2026-11-16 12:00:00 UTC.
pub const NOON: u64 = 1_794_787_200 + 12 * 3600;
pub const DAY: u64 = 86_400;

thread_local! {
    /// Seconds of the test clock.
    pub static NOW: RefCell<u64> = const { RefCell::new(NOON) };
    pub static PERSONS: RefCell<BTreeSet<AccountId32>> = const { RefCell::new(BTreeSet::new()) };
    pub static SERVICES: RefCell<BTreeSet<AccountId32>> = const { RefCell::new(BTreeSet::new()) };
}

/// The test clock.
pub struct Clock;

impl frame_support::traits::UnixTime for Clock {
    fn now() -> core::time::Duration {
        core::time::Duration::from_secs(NOW.with(|n| *n.borrow()))
    }
}

pub fn advance(secs: u64) {
    NOW.with(|n| *n.borrow_mut() += secs);
}

/// The lists of the tests. A key without a person may send a remark that
/// starts with `1` – the stand-in for "open an account, present a claim".
pub struct Lists;

impl crate::Senders<AccountId32, RuntimeCall> for Lists {
    fn sender(who: &AccountId32) -> Sender {
        if SERVICES.with(|s| s.borrow().contains(who)) {
            Sender::Service
        } else if PERSONS.with(|p| p.borrow().contains(who)) {
            Sender::Person
        } else {
            Sender::NewKey
        }
    }

    fn open_to_new_keys(call: &RuntimeCall) -> bool {
        matches!(
            call,
            RuntimeCall::System(frame_system::Call::remark { remark }) if remark.first() == Some(&1)
        )
    }
}

impl pallet_capita_quota::Config for Test {
    type Time = Clock;
    type Senders = Lists;
    type PersonPerDay = ConstU32<4>;
    type NewKeyPerDay = ConstU32<2>;
    type NewKeysPerBlock = ConstU32<3>;
}

pub fn account(n: u8) -> AccountId32 {
    AccountId32::new([n; 32])
}

/// An account a verified person stands behind.
pub fn person(n: u8) -> AccountId32 {
    let who = account(n);
    PERSONS.with(|p| p.borrow_mut().insert(who.clone()));
    who
}

/// A key of the operation of the system.
pub fn service(n: u8) -> AccountId32 {
    let who = account(n);
    SERVICES.with(|s| s.borrow_mut().insert(who.clone()));
    who
}

/// What a key without a person may send.
pub fn opening() -> RuntimeCall {
    RuntimeCall::System(frame_system::Call::remark { remark: vec![1] })
}

/// Anything else.
pub fn other() -> RuntimeCall {
    RuntimeCall::System(frame_system::Call::remark { remark: vec![0] })
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    NOW.with(|n| *n.borrow_mut() = NOON);
    PERSONS.with(|p| p.borrow_mut().clear());
    SERVICES.with(|s| s.borrow_mut().clear());
    let storage = RuntimeGenesisConfig {
        system: Default::default(),
    }
    .build_storage()
    .unwrap();
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| System::set_block_number(1));
    ext
}
