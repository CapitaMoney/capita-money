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
//
// Derived from the solo-chain template of the Polkadot SDK (public domain).

//! Configuration of every pallet of the runtime.
//!
//! Numbers here are those of a test chain, not decisions about Capita Money.

use frame_support::{
    derive_impl,
    dispatch::DispatchClass,
    parameter_types,
    traits::{ConstBool, ConstU32, ConstU64},
    weights::{
        constants::{ParityDbWeight, WEIGHT_REF_TIME_PER_SECOND},
        Weight,
    },
};
use frame_system::{
    limits::{BlockLength, BlockWeights},
    EnsureRoot,
};
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_runtime::{traits::Verify, Perbill};
use sp_version::RuntimeVersion;

use super::{
    AccountId, Aura, Block, BlockNumber, Hash, Identity, IdtyIndex, Membership, Nonce, PalletInfo,
    Runtime, RuntimeCall, RuntimeEvent, RuntimeOrigin, RuntimeTask, Signature, Timestamp, DAYS,
    SLOT_DURATION, VERSION,
};

const NORMAL_DISPATCH_RATIO: Perbill = Perbill::from_percent(75);

parameter_types! {
    pub const BlockHashCount: BlockNumber = 2400;
    pub const Version: RuntimeVersion = VERSION;

    /// 2 seconds of computation per 6-second block.
    pub RuntimeBlockWeights: BlockWeights = BlockWeights::with_sensible_defaults(
        Weight::from_parts(2u64 * WEIGHT_REF_TIME_PER_SECOND, u64::MAX),
        NORMAL_DISPATCH_RATIO,
    );
    pub RuntimeBlockLength: BlockLength = BlockLength::builder()
        .max_length(5 * 1024 * 1024)
        .modify_max_length_for_class(DispatchClass::Normal, |m| *m = NORMAL_DISPATCH_RATIO * *m)
        .build();
    pub const SS58Prefix: u8 = 42;
}

#[derive_impl(frame_system::config_preludes::SolochainDefaultConfig)]
impl frame_system::Config for Runtime {
    type Block = Block;
    type BlockWeights = RuntimeBlockWeights;
    type BlockLength = RuntimeBlockLength;
    type AccountId = AccountId;
    type Nonce = Nonce;
    type Hash = Hash;
    type BlockHashCount = BlockHashCount;
    /// The node stores in ParityDB unless built with RocksDB.
    type DbWeight = ParityDbWeight;
    type Version = Version;
    /// Nothing: balances are kept by `pallet-capita`, not by the system pallet.
    type AccountData = ();
    /// 42 = generic address format. Offene Frage: an own prefix for Capita.
    type SS58Prefix = SS58Prefix;
    type MaxConsumers = ConstU32<16>;
}

impl pallet_aura::Config for Runtime {
    type AuthorityId = AuraId;
    type DisabledValidators = ();
    type MaxAuthorities = ConstU32<32>;
    type AllowMultipleBlocksPerSlot = ConstBool<false>;
    type SlotDuration = pallet_aura::MinimumPeriodTimesTwo<Runtime>;
}

impl pallet_grandpa::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type WeightInfo = ();
    type MaxAuthorities = ConstU32<32>;
    type MaxNominators = ConstU32<0>;
    type MaxSetIdSessionEntries = ConstU64<0>;
    type KeyOwnerProof = sp_core::Void;
    type EquivocationReportSystem = ();
}

impl pallet_timestamp::Config for Runtime {
    /// Milliseconds since the Unix epoch.
    type Moment = u64;
    type OnTimestampSet = Aura;
    type MinimumPeriod = ConstU64<{ SLOT_DURATION / 2 }>;
    type WeightInfo = ();
}

impl pallet_sudo::Config for Runtime {
    type RuntimeEvent = RuntimeEvent;
    type RuntimeCall = RuntimeCall;
    type WeightInfo = pallet_sudo::weights::SubstrateWeight<Runtime>;
}

/// The rules. Every authority is root, reached through the sudo key: the
/// questions who governs, who attests and who sells Capita are not decided
/// (E1, F3, C1), and a test chain needs no answer.
impl pallet_capita::Config for Runtime {
    type Time = Timestamp;
    type GovernanceOrigin = EnsureRoot<AccountId>;
    type IdentityOrigin = EnsureRoot<AccountId>;
    type PurchaseOrigin = EnsureRoot<AccountId>;
    type CreditOrigin = EnsureRoot<AccountId>;
    type TierGate = capita_core::ports::NoGate;
}

// Periods of Duniter's pallets, in blocks. Placeholders in the order of
// magnitude Duniter uses; the credential pallet (M3) will set them.
parameter_types! {
    pub const ConfirmPeriod: BlockNumber = 14 * DAYS;
    pub const ValidationPeriod: BlockNumber = 60 * DAYS;
    pub const AutorevocationPeriod: BlockNumber = 365 * DAYS;
    pub const DeletionPeriod: BlockNumber = 10 * 365 * DAYS;
    pub const ChangeOwnerKeyPeriod: BlockNumber = 180 * DAYS;
    pub const IdtyCreationPeriod: BlockNumber = DAYS;
    pub const MembershipPeriod: BlockNumber = 365 * DAYS;
    pub const MembershipRenewalPeriod: BlockNumber = 60 * DAYS;
}

/// Duniter's rule for the name of an identity.
pub struct IdtyNameValidatorImpl;
impl pallet_identity::traits::IdtyNameValidator for IdtyNameValidatorImpl {
    fn validate(idty_name: &pallet_identity::IdtyName) -> bool {
        duniter_primitives::validate_idty_name(&idty_name.0)
    }
}

/// Stands where Duniter's runtime has its web of trust: keeps the identity
/// and the membership of a person in step and decides nothing. Who becomes a
/// member is for the credential pallet (M3) to say; the same handlers will
/// then report to `pallet-capita` (`do_attest`, `do_set_status`).
pub struct NoWebOfTrust;

impl sp_membership::traits::OnNewMembership<IdtyIndex> for NoWebOfTrust {
    fn on_created(idty_index: &IdtyIndex) {
        Identity::membership_added(*idty_index);
    }

    fn on_renewed(_idty_index: &IdtyIndex) {}
}

impl sp_membership::traits::OnRemoveMembership<IdtyIndex> for NoWebOfTrust {
    fn on_removed(idty_index: &IdtyIndex) -> Weight {
        Identity::membership_removed(*idty_index)
    }
}

impl pallet_identity::traits::OnRemoveIdty<Runtime> for NoWebOfTrust {
    fn on_removed(idty_index: &IdtyIndex) -> Weight {
        Membership::do_remove_membership(
            *idty_index,
            pallet_membership::MembershipRemovalReason::System,
        )
    }

    fn on_revoked(idty_index: &IdtyIndex) -> Weight {
        Membership::do_remove_membership(
            *idty_index,
            pallet_membership::MembershipRemovalReason::Revoked,
        )
    }
}

/// Duniter's identities. `()` = the handler that allows everything and does
/// nothing: any identity may create further ones.
impl pallet_identity::Config for Runtime {
    type AccountId32 = AccountId;
    type AccountLinker = ();
    type AutorevocationPeriod = AutorevocationPeriod;
    type ChangeOwnerKeyPeriod = ChangeOwnerKeyPeriod;
    type CheckAccountWorthiness = ();
    type CheckIdtyCallAllowed = ();
    type ConfirmPeriod = ConfirmPeriod;
    type DeletionPeriod = DeletionPeriod;
    type IdtyCreationPeriod = IdtyCreationPeriod;
    type IdtyData = ();
    type IdtyIndex = IdtyIndex;
    type IdtyNameValidator = IdtyNameValidatorImpl;
    type OnKeyChange = ();
    type OnNewIdty = ();
    type OnRemoveIdty = NoWebOfTrust;
    type Signature = Signature;
    type Signer = <Signature as Verify>::Signer;
    type ValidationPeriod = ValidationPeriod;
    type WeightInfo = ();
}

/// Duniter's memberships. No call adds one yet: they come from the genesis
/// block and end by expiry.
impl pallet_membership::Config for Runtime {
    type CheckMembershipOpAllowed = ();
    type IdtyAttr = Identity;
    type IdtyId = IdtyIndex;
    type MembershipPeriod = MembershipPeriod;
    type MembershipRenewalPeriod = MembershipRenewalPeriod;
    type OnNewMembership = NoWebOfTrust;
    type OnRemoveMembership = NoWebOfTrust;
    type WeightInfo = ();
}
