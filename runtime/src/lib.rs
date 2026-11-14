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

//! Runtime of the Capita Money test chain.
//!
//! The rules live in `capita-core`, their chain form in `pallet-capita`. This
//! crate only composes: consensus (Aura, GRANDPA), time, a sudo key that
//! stands in for governance, the rules, Duniter's identity and membership
//! pallets, and the credential pallet that decides who is a person. Design:
//! `docs/adr/0010-test-chain.md`, `docs/adr/0011-identity-by-credentials.md`.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
include!(concat!(env!("OUT_DIR"), "/wasm_binary.rs"));

extern crate alloc;

pub mod apis;
pub mod configs;
pub mod genesis_config_presets;
pub mod identity;
pub mod nonce;
#[cfg(test)]
mod tests;

use alloc::vec::Vec;
use sp_runtime::{
    generic, impl_opaque_keys,
    traits::{BlakeTwo256, IdentifyAccount, Verify},
    MultiAddress, MultiSignature,
};
#[cfg(feature = "std")]
use sp_version::NativeVersion;
use sp_version::RuntimeVersion;

pub use frame_system::Call as SystemCall;
pub use pallet_capita::Call as CapitaCall;
pub use pallet_capita_credential::Call as CredentialCall;
pub use pallet_timestamp::Call as TimestampCall;
#[cfg(any(feature = "std", test))]
pub use sp_runtime::BuildStorage;

/// Types the node handles without knowing the runtime: blocks whose
/// transactions stay opaque bytes.
pub mod opaque {
    use super::*;
    use sp_runtime::{
        generic,
        traits::{BlakeTwo256, Hash as HashT},
    };

    pub use sp_runtime::OpaqueExtrinsic as UncheckedExtrinsic;

    /// Opaque block header type.
    pub type Header = generic::Header<BlockNumber, BlakeTwo256>;
    /// Opaque block type.
    pub type Block = generic::Block<Header, UncheckedExtrinsic>;
    /// Opaque block identifier type.
    pub type BlockId = generic::BlockId<Block>;
    /// Opaque block hash type.
    pub type Hash = <BlakeTwo256 as HashT>::Output;
}

impl_opaque_keys! {
    /// The keys a block author holds.
    pub struct SessionKeys {
        /// Block production.
        pub aura: Aura,
        /// Finality.
        pub grandpa: Grandpa,
    }
}

/// Name and version of this runtime. `spec_version` rises with every change
/// of the rules of the chain, `transaction_version` with every change of the
/// form of a transaction.
#[sp_version::runtime_version]
pub const VERSION: RuntimeVersion = RuntimeVersion {
    spec_name: alloc::borrow::Cow::Borrowed("capita-test"),
    impl_name: alloc::borrow::Cow::Borrowed("capita-test"),
    authoring_version: 1,
    spec_version: 3,
    impl_version: 1,
    apis: apis::RUNTIME_API_VERSIONS,
    transaction_version: 1,
    system_version: 1,
};

/// Target time between two blocks, in milliseconds. It cannot change once the
/// chain runs: block production derives its slots from it.
pub const MILLI_SECS_PER_BLOCK: u64 = 6000;
/// Length of a slot of block production, in milliseconds.
pub const SLOT_DURATION: u64 = MILLI_SECS_PER_BLOCK;

/// Blocks per minute.
pub const MINUTES: BlockNumber = 60_000 / (MILLI_SECS_PER_BLOCK as BlockNumber);
/// Blocks per hour.
pub const HOURS: BlockNumber = MINUTES * 60;
/// Blocks per day.
pub const DAYS: BlockNumber = HOURS * 24;

/// A span of `secs` seconds in blocks, rounded down; the highest block
/// number if it does not fit.
pub const fn blocks(secs: u64) -> BlockNumber {
    let blocks = secs.saturating_mul(1000) / MILLI_SECS_PER_BLOCK;
    if blocks > BlockNumber::MAX as u64 {
        BlockNumber::MAX
    } else {
        blocks as BlockNumber
    }
}

/// The version information used to identify this runtime when compiled natively.
#[cfg(feature = "std")]
pub fn native_version() -> NativeVersion {
    NativeVersion {
        runtime_version: VERSION,
        can_author_with: Default::default(),
    }
}

/// Signature of a transaction: sr25519, ed25519 or ECDSA.
pub type Signature = MultiSignature;

/// An account is its public key: 32 bytes, the form `capita-core` uses.
pub type AccountId = <<Signature as Verify>::Signer as IdentifyAccount>::AccountId;

/// Number of transactions an account has sent.
pub type Nonce = u32;

/// A hash of some data used by the chain.
pub type Hash = sp_core::H256;

/// Number of a block.
pub type BlockNumber = u32;

/// Number of an identity (Duniter's identity pallet).
pub type IdtyIndex = u32;

/// The address format for describing accounts.
pub type Address = MultiAddress<AccountId, ()>;

/// Block header type as expected by this runtime.
pub type Header = generic::Header<BlockNumber, BlakeTwo256>;

/// Block type as expected by this runtime.
pub type Block = generic::Block<Header, UncheckedExtrinsic>;

/// A block signed with a justification.
pub type SignedBlock = generic::SignedBlock<Block>;

/// BlockId type as expected by this runtime.
pub type BlockId = generic::BlockId<Block>;

/// What every transaction is checked against before it runs. No entry charges
/// a fee: the test chain has none (`docs/adr/0010-test-chain.md`).
pub type TxExtension = (
    frame_system::AuthorizeCall<Runtime>,
    frame_system::CheckNonZeroSender<Runtime>,
    frame_system::CheckSpecVersion<Runtime>,
    frame_system::CheckTxVersion<Runtime>,
    frame_system::CheckGenesis<Runtime>,
    frame_system::CheckEra<Runtime>,
    nonce::CheckNonce<Runtime>,
    frame_system::CheckWeight<Runtime>,
    frame_system::WeightReclaim<Runtime>,
);

/// Unchecked extrinsic type as expected by this runtime.
pub type UncheckedExtrinsic =
    generic::UncheckedExtrinsic<Address, RuntimeCall, Signature, TxExtension>;

/// The payload being signed in transactions.
pub type SignedPayload = generic::SignedPayload<RuntimeCall, TxExtension>;

/// Executive: handles dispatch to the various modules.
pub type Executive = frame_executive::Executive<
    Runtime,
    Block,
    frame_system::ChainContext<Runtime>,
    Runtime,
    AllPalletsWithSystem,
>;

/// The pallets of the chain. An index never changes once a chain runs; the
/// gaps leave room.
#[frame_support::runtime]
mod runtime {
    #[runtime::runtime]
    #[runtime::derive(
        RuntimeCall,
        RuntimeEvent,
        RuntimeError,
        RuntimeOrigin,
        RuntimeFreezeReason,
        RuntimeHoldReason,
        RuntimeSlashReason,
        RuntimeLockId,
        RuntimeTask,
        RuntimeViewFunction
    )]
    pub struct Runtime;

    #[runtime::pallet_index(0)]
    pub type System = frame_system;

    #[runtime::pallet_index(1)]
    pub type Timestamp = pallet_timestamp;

    #[runtime::pallet_index(2)]
    pub type Aura = pallet_aura;

    #[runtime::pallet_index(3)]
    pub type Grandpa = pallet_grandpa;

    #[runtime::pallet_index(4)]
    pub type Sudo = pallet_sudo;

    /// The rules of Capita Money.
    #[runtime::pallet_index(10)]
    pub type Capita = pallet_capita;

    /// Duniter: one identity per person.
    #[runtime::pallet_index(20)]
    pub type Identity = pallet_identity;

    /// Duniter: which identities currently count as members.
    #[runtime::pallet_index(21)]
    pub type Membership = pallet_membership;

    /// Who is a person: trusted identity services and their claims.
    #[runtime::pallet_index(22)]
    pub type Credential = pallet_capita_credential;
}
