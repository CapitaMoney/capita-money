// Capita Money – pallet-capita
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

//! A minimal runtime for the tests: system, timestamp and this pallet.

use capita_core::attributes::VerificationLevel;
use capita_core::params::{
    CapParams, ClosedBalance, DemurrageParams, LadderStep, LoanParams, Params,
};
use capita_core::ports::{Event as RuleEvent, NoGate};
use capita_core::types::{
    AccountId as CoreId, Amount, Duration, ParamsVersion, Rate, Share, TierId, Timestamp as Instant,
};
use frame_support::derive_impl;
use frame_system::EnsureRoot;
use sp_runtime::traits::IdentityLookup;
use sp_runtime::{AccountId32, BuildStorage};

use crate as pallet_capita;

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Timestamp: pallet_timestamp,
        Capita: pallet_capita,
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

impl pallet_capita::Config for Test {
    type Time = Timestamp;
    type GovernanceOrigin = EnsureRoot<AccountId32>;
    type IdentityOrigin = EnsureRoot<AccountId32>;
    type PurchaseOrigin = EnsureRoot<AccountId32>;
    type CreditOrigin = EnsureRoot<AccountId32>;
    type TierGate = NoGate;
}

/// Start of the test clock, seconds since the Unix epoch.
pub const T0: u64 = 1_800_000_000;
pub const MINUTE: u64 = 60;
pub const HOUR: u64 = 3_600;
pub const DAY: u64 = 86_400;
pub const MONTH: u64 = 2_629_800;

pub fn acct(n: u8) -> AccountId32 {
    AccountId32::new([n; 32])
}

pub fn cid(n: u8) -> CoreId {
    CoreId([n; 32])
}

pub fn signed(n: u8) -> RuntimeOrigin {
    RuntimeOrigin::signed(acct(n))
}

pub fn root() -> RuntimeOrigin {
    RuntimeOrigin::root()
}

pub fn cents(n: u64) -> Amount {
    Amount::from_cents(n)
}

/// `T0` plus `offset` seconds.
pub fn ts(offset: u64) -> Instant {
    Instant::from_secs(T0 + offset)
}

/// Sets the block time to `T0` plus `offset` seconds.
pub fn set_time(offset: u64) {
    Timestamp::set_timestamp((T0 + offset) * 1_000);
}

pub fn balance(n: u8) -> i64 {
    pallet_capita::Balances::<Test>::get(cid(n)).cents()
}

fn step(id: u16, level: u8, amount: u64, per: u64) -> LadderStep {
    LadderStep {
        id: TierId(id),
        min_level: VerificationLevel(level),
        rate: Rate::new(cents(amount), Duration::from_secs(per)).unwrap(),
    }
}

/// Version 1, in force since the epoch. Ladder: step 0 = 1 cent per hour
/// (unverified), step 1 = 1 cent per minute (level 1), step 2 = 3 cents per
/// minute (level 2). Start grant 100 cents, caps far above the rates,
/// 14-day claim window, no ripening, no demurrage, repayment share 50 %.
pub fn params() -> Params {
    Params {
        version: ParamsVersion(1),
        valid_from: Instant::from_secs(0),
        start_grant: cents(100),
        ladder: vec![
            step(0, 0, 1, HOUR),
            step(1, 1, 1, MINUTE),
            step(2, 2, 3, MINUTE),
        ],
        claim_window: Duration::from_secs(14 * DAY),
        caps: CapParams {
            period: Duration::MONTH,
            global_cap: cents(100_000_000),
            by_country: vec![],
            ramp: Duration::ZERO,
        },
        loans: LoanParams {
            max_credit_line: cents(50_000),
            max_outstanding: cents(100_000),
            repayment_share: Share::from_ppm(500_000).unwrap(),
        },
        demurrage: DemurrageParams {
            rate: Share::ZERO,
            period: Duration::MONTH,
            free_allowance: Amount::ZERO,
        },
        ripening: Duration::ZERO,
        sponsor_keys: vec![],
        closed_balance: ClosedBalance::Keep,
    }
}

/// [`params`] with demurrage of 1 % per month on everything above 10 Capita.
pub fn params_with_demurrage() -> Params {
    let mut p = params();
    p.demurrage.rate = Share::from_ppm(10_000).unwrap();
    p.demurrage.free_allowance = cents(1_000);
    p
}

/// A chain at block 1, time `T0`, with `params` as genesis rules.
pub fn new_test_ext(params: Params) -> sp_io::TestExternalities {
    ext(Some(params))
}

/// A chain at block 1, time `T0`, with or without genesis rules.
pub fn ext(params: Option<Params>) -> sp_io::TestExternalities {
    let mut storage = frame_system::GenesisConfig::<Test>::default()
        .build_storage()
        .unwrap();
    pallet_capita::GenesisConfig::<Test> {
        params,
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
/// transaction, so that a failed call leaves no trace.
pub fn call(origin: RuntimeOrigin, call: pallet_capita::Call<Test>) -> sp_runtime::DispatchResult {
    use sp_runtime::traits::Dispatchable;
    RuntimeCall::Capita(call)
        .dispatch(origin)
        .map(|_| ())
        .map_err(|e| e.error)
}

/// The rule events deposited so far, oldest first.
pub fn rule_events() -> Vec<RuleEvent> {
    System::events()
        .into_iter()
        .filter_map(|record| match record.event {
            RuntimeEvent::Capita(pallet_capita::Event::Rule(event)) => Some(event),
            _ => None,
        })
        .collect()
}
