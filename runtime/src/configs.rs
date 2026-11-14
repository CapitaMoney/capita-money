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
//! Numbers here are technical ones of a test chain. Rule values come from
//! the rules files, through `capita-rules` (`docs/adr/0012-rules-file.md`).

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
    blocks,
    identity::{CapitaPurchases, CapitaRules, CredentialLink, DuniterIdentities},
    AccountId, Aura, Block, BlockNumber, Hash, Identity, IdtyIndex, Nonce, PalletInfo, Runtime,
    RuntimeCall, RuntimeEvent, RuntimeOrigin, RuntimeTask, Signature, Timestamp, DAYS,
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
/// (E1, F3, C1), and a test chain needs no answer. Persons are attested by
/// the credential pallet below; root can still do so by hand.
impl pallet_capita::Config for Runtime {
    type Time = Timestamp;
    type GovernanceOrigin = EnsureRoot<AccountId>;
    type IdentityOrigin = EnsureRoot<AccountId>;
    type PurchaseOrigin = EnsureRoot<AccountId>;
    type CreditOrigin = EnsureRoot<AccountId>;
    type TierGate = capita_core::ports::NoGate;
}

/// Who is a person: claims of the identity services on the list, which root
/// maintains (F3 and E1 are not decided).
impl pallet_capita_credential::Config for Runtime {
    type Time = Timestamp;
    type RegistryOrigin = EnsureRoot<AccountId>;
    type Memberships = DuniterIdentities;
    type Rules = CapitaRules;
    type MoveDelay = KeyMoveDelay;
}

/// The euro door: the list of doors kept by root (who may sell Capita is not
/// decided: E1, P04.3). A voucher mints through `pallet-capita`.
impl pallet_capita_door::Config for Runtime {
    type RegistryOrigin = EnsureRoot<AccountId>;
    type Rules = CapitaPurchases;
    type UnsignedPriority = DoorPriority;
}

parameter_types! {
    /// Priority of a redeeming transaction (no signer, no fee) in the pool.
    pub const DoorPriority: u64 = 1 << 20;
}

parameter_types! {
    /// How long the move of an account to a new key waits, in seconds. A
    /// rule (`identity.key_move_delay` of `rules/alpha.toml`). Kept in
    /// storage, so that whoever holds root can set another time on a
    /// running chain (`system.setStorage`, key `:KeyMoveDelay:` hashed with
    /// twox_128) – a test chain need not wait seven days.
    pub storage KeyMoveDelay: u64 = capita_rules::alpha::KEY_MOVE_DELAY.secs();
}

// Periods of Duniter's pallets, in blocks. The one that is a rule comes from
// the rules file; the others are never reached on this chain.
parameter_types! {
    /// Not reached: the credential pallet creates and confirms an identity
    /// in one step.
    pub const ConfirmPeriod: BlockNumber = 14 * DAYS;
    /// Not reached: a membership follows in the same step.
    pub const ValidationPeriod: BlockNumber = 60 * DAYS;
    /// In effect never: an identity without membership is not revoked by
    /// the passing of time. A revoked identity is final, and its account
    /// would stay frozen; a person who lets a claim lapse presents a new
    /// one. Revoking remains an act of the holder of the key, or of root.
    pub const AutorevocationPeriod: BlockNumber = 100 * 365 * DAYS;
    pub const DeletionPeriod: BlockNumber = 10 * 365 * DAYS;
    /// Not reached: identities do not move to another key.
    pub const ChangeOwnerKeyPeriod: BlockNumber = 180 * DAYS;
    /// None: the one creator of identities, the credential pallet, creates
    /// as many as claims are accepted.
    pub const IdtyCreationPeriod: BlockNumber = 0;
    /// How long a membership lasts before the holder renews it with the
    /// claim held (`credential.renew`) or presents a new one. A rule
    /// (`identity.membership_period` of `rules/alpha.toml`), in blocks.
    pub const MembershipPeriod: BlockNumber =
        blocks(capita_rules::alpha::MEMBERSHIP_PERIOD.secs());
    /// Not used: Duniter's web of trust reads it, the pallets here do not.
    pub const MembershipRenewalPeriod: BlockNumber = 60 * DAYS;
}

/// Duniter's rule for the name of an identity.
pub struct IdtyNameValidatorImpl;
impl pallet_identity::traits::IdtyNameValidator for IdtyNameValidatorImpl {
    fn validate(idty_name: &pallet_identity::IdtyName) -> bool {
        duniter_primitives::validate_idty_name(&idty_name.0)
    }
}

/// Duniter's identities, created by the credential pallet alone
/// (`identity.rs`).
impl pallet_identity::Config for Runtime {
    type AccountId32 = AccountId;
    type AccountLinker = ();
    type AutorevocationPeriod = AutorevocationPeriod;
    type ChangeOwnerKeyPeriod = ChangeOwnerKeyPeriod;
    type CheckAccountWorthiness = ();
    type CheckIdtyCallAllowed = CredentialLink;
    type ConfirmPeriod = ConfirmPeriod;
    type DeletionPeriod = DeletionPeriod;
    type IdtyCreationPeriod = IdtyCreationPeriod;
    type IdtyData = ();
    type IdtyIndex = IdtyIndex;
    type IdtyNameValidator = IdtyNameValidatorImpl;
    type OnKeyChange = CredentialLink;
    type OnNewIdty = ();
    type OnRemoveIdty = CredentialLink;
    type Signature = Signature;
    type Signer = <Signature as Verify>::Signer;
    type ValidationPeriod = ValidationPeriod;
    type WeightInfo = ();
}

/// Duniter's memberships. The pallet has no calls: the credential pallet
/// grants, renews and ends them; they also end by expiry.
impl pallet_membership::Config for Runtime {
    type CheckMembershipOpAllowed = ();
    type IdtyAttr = Identity;
    type IdtyId = IdtyIndex;
    type MembershipPeriod = MembershipPeriod;
    type MembershipRenewalPeriod = MembershipRenewalPeriod;
    type OnNewMembership = CredentialLink;
    type OnRemoveMembership = CredentialLink;
    type WeightInfo = ();
}
