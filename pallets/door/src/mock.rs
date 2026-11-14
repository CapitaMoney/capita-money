// Capita Money – pallet-capita-door
// SPDX-License-Identifier: AGPL-3.0-only

//! A minimal runtime for the tests: system and this pallet. The monetary
//! rules are a stand-in that records what the pallet mints.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use capita_core::types::{AccountId as CoreId, Amount};
use capita_door::encoding::voucher_bytes;
use capita_door::types::{Door as DoorEntry, DoorId, DoorSignature, DoorStatus, PaymentRef, Voucher};
use frame_support::derive_impl;
use frame_support::pallet_prelude::{DispatchError, DispatchResult};
use frame_system::EnsureRoot;
use sp_core::Pair;
use sp_runtime::traits::IdentityLookup;
use sp_runtime::{AccountId32, BuildStorage};

use crate as pallet_capita_door;

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        CapitaDoor: pallet_capita_door,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = AccountId32;
    type Lookup = IdentityLookup<Self::AccountId>;
}

frame_support::parameter_types! {
    pub const Priority: u64 = 1 << 20;
}

impl pallet_capita_door::Config for Test {
    type RegistryOrigin = EnsureRoot<AccountId32>;
    type Rules = RulesStub;
    type UnsignedPriority = Priority;
}

/// What the stand-in holds.
#[derive(Default)]
pub struct World {
    /// Accounts that can receive.
    pub open: BTreeSet<AccountId32>,
    /// What was minted, per account.
    pub bought: BTreeMap<AccountId32, u64>,
}

thread_local! {
    pub static WORLD: RefCell<World> = RefCell::new(World::default());
}

/// The monetary rules, as a record.
pub struct RulesStub;
impl crate::Rules<AccountId32> for RulesStub {
    fn can_buy(who: &AccountId32) -> bool {
        WORLD.with(|w| w.borrow().open.contains(who))
    }
    fn purchase(who: &AccountId32, amount: Amount) -> DispatchResult {
        WORLD.with(|w| {
            let mut w = w.borrow_mut();
            if !w.open.contains(who) {
                return Err(DispatchError::Other("closed"));
            }
            *w.bought.entry(who.clone()).or_default() += amount.cents();
            Ok(())
        })
    }
}

pub fn acct(n: u8) -> AccountId32 {
    AccountId32::new([n; 32])
}

pub fn open(n: u8) {
    WORLD.with(|w| w.borrow_mut().open.insert(acct(n)));
}

pub fn bought(n: u8) -> u64 {
    WORLD.with(|w| w.borrow().bought.get(&acct(n)).copied().unwrap_or(0))
}

/// The development door of the test chains, and a second key.
pub fn pair(n: u8) -> sp_core::ed25519::Pair {
    sp_core::ed25519::Pair::from_seed(&[n; 32])
}

pub fn door(n: u8, max: u64) -> DoorEntry {
    DoorEntry {
        key: pair(n).public().0,
        max_amount: Amount::from_cents(max),
        status: DoorStatus::Active,
    }
}

pub fn voucher(door: u32, payment: u8, account: u8, cents: u64) -> Voucher {
    Voucher {
        door: DoorId(door),
        payment: PaymentRef([payment; 32]),
        account: CoreId([account; 32]),
        amount: Amount::from_cents(cents),
    }
}

/// Signs a voucher for the chain of the tests with the key `n`.
pub fn sign(n: u8, voucher: &Voucher) -> DoorSignature {
    let bytes = voucher_bytes(&crate::Pallet::<Test>::audience(), voucher);
    DoorSignature(pair(n).sign(&bytes).0)
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    WORLD.with(|w| *w.borrow_mut() = World::default());
    let storage = RuntimeGenesisConfig {
        system: Default::default(),
        capita_door: pallet_capita_door::GenesisConfig {
            doors: vec![door(1, 10_000)],
            ..Default::default()
        },
    }
    .build_storage()
    .unwrap();
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| System::set_block_number(1));
    ext
}
