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

//! The states a chain can start from: `development` (one node, Alice) and
//! `local_testnet` (two nodes, Alice and Bob).

use crate::{
    identity::{creator, CREATOR_NAME},
    AccountId, BlockNumber, IdtyIndex, Runtime, RuntimeGenesisConfig,
};
use alloc::{collections::BTreeMap, vec, vec::Vec};
use capita_core::attributes::VerificationLevel;
use capita_core::params::{
    CapParams, ClosedBalance, DemurrageParams, LadderStep, LoanParams, Params,
};
use capita_core::types::{Amount, Duration, ParamsVersion, Rate, Share, TierId, Timestamp};
use capita_credential::did::did_key;
use capita_credential::types::{Issuer, IssuerStatus, PublicKey, Realm};
use frame_support::build_struct_json_patch;
use pallet_identity::{GenesisIdty, IdtyName, IdtyStatus, IdtyValue};
use serde_json::Value;
use sp_consensus_aura::sr25519::AuthorityId as AuraId;
use sp_consensus_grandpa::AuthorityId as GrandpaId;
use sp_genesis_builder::{self, PresetId};
use sp_keyring::{Ed25519Keyring, Sr25519Keyring};
use sp_membership::MembershipData;

/// Rules for trying things out – every number is an example, none a decision:
/// start grant 1 Capita; without verification 1 cent per hour, at level 1
/// 1 cent per minute, at level 2 3 cents per minute; claims reach back
/// 14 days; credit lines up to 500 Capita; no demurrage, no ripening.
pub fn test_params() -> Option<Params> {
    let step = |id: u16, level: u8, cents: u64, per: Duration| {
        Some(LadderStep {
            id: TierId(id),
            min_level: VerificationLevel(level),
            rate: Rate::new(Amount::from_cents(cents), per).ok()?,
        })
    };
    Some(Params {
        version: ParamsVersion(1),
        valid_from: Timestamp::from_secs(0),
        start_grant: Amount::from_cents(100),
        ladder: vec![
            step(0, 0, 1, Duration::HOUR)?,
            step(1, 1, 1, Duration::MINUTE)?,
            step(2, 2, 3, Duration::MINUTE)?,
        ],
        claim_window: Duration::from_secs(14 * Duration::DAY.secs()),
        caps: CapParams {
            period: Duration::MONTH,
            global_cap: Amount::from_cents(100_000_000),
            by_country: vec![],
            ramp: Duration::ZERO,
        },
        loans: LoanParams {
            max_credit_line: Amount::from_cents(50_000),
            max_outstanding: Amount::from_cents(100_000),
            repayment_share: Share::from_ppm(500_000).ok()?,
        },
        demurrage: DemurrageParams {
            rate: Share::ZERO,
            period: Duration::MONTH,
            free_allowance: Amount::ZERO,
        },
        ripening: Duration::ZERO,
        sponsor_keys: vec![],
        closed_balance: ClosedBalance::Keep,
    })
}

/// Secret key (Ed25519 seed) of the identity service of the test chain.
/// Public on purpose: anybody can sign claims for a test chain, for example
/// with `cargo run -p capita-credential --example issue`.
pub const DEV_ISSUER_SEED: [u8; 32] = *b"capita-money development issuer!";

/// The public key that belongs to [`DEV_ISSUER_SEED`].
pub const DEV_ISSUER_KEY: PublicKey = PublicKey::Ed25519([
    0x9e, 0x59, 0x68, 0xb2, 0xe9, 0x5b, 0xa9, 0xb2, 0xd2, 0xef, 0x05, 0x4e, 0x05, 0x61, 0xe6, 0xc5,
    0x6e, 0x8b, 0x00, 0x3b, 0x8a, 0xbd, 0xf3, 0x5b, 0xb7, 0xeb, 0x25, 0x8e, 0x2f, 0xd7, 0xb3, 0xe4,
]);

/// Issuer 0 of a test chain: named by its key (`did:key`), trusted for both
/// verified steps of [`test_params`] and for residence, claims of any
/// length. Its secret key is public – see [`DEV_ISSUER_SEED`].
pub fn dev_issuer() -> Issuer {
    Issuer {
        did: did_key(&DEV_ISSUER_KEY),
        keys: vec![DEV_ISSUER_KEY],
        realm: Realm(0),
        max_level: VerificationLevel(2),
        residence: true,
        attributes: vec![],
        max_validity: None,
        status: IssuerStatus::Active,
    }
}

/// Number of the identity in whose name the credential pallet creates
/// identities (`identity.rs`).
pub const CREATOR_IDTY: IdtyIndex = 1;

/// Genesis of a test network: `initial_authorities` produce and finalise
/// blocks, `root` holds the sudo key. No person is a member yet: members
/// come from claims. The one identity is the creator of identities, with a
/// membership that never runs out.
fn testnet_genesis(initial_authorities: Vec<(AuraId, GrandpaId)>, root: AccountId) -> Value {
    let identities = vec![GenesisIdty::<Runtime> {
        index: CREATOR_IDTY,
        name: IdtyName::from(CREATOR_NAME),
        value: IdtyValue {
            data: (),
            next_creatable_identity_on: 0,
            old_owner_key: None,
            owner_key: creator(),
            next_scheduled: 0,
            status: IdtyStatus::Member,
        },
    }];
    let memberships: BTreeMap<IdtyIndex, MembershipData<BlockNumber>> = BTreeMap::from([(
        CREATOR_IDTY,
        MembershipData {
            expire_on: BlockNumber::MAX,
        },
    )]);
    build_struct_json_patch!(RuntimeGenesisConfig {
        aura: pallet_aura::GenesisConfig {
            authorities: initial_authorities
                .iter()
                .map(|x| x.0.clone())
                .collect::<Vec<_>>(),
        },
        grandpa: pallet_grandpa::GenesisConfig {
            authorities: initial_authorities
                .iter()
                .map(|x| (x.1.clone(), 1))
                .collect::<Vec<_>>(),
        },
        sudo: pallet_sudo::GenesisConfig { key: Some(root) },
        capita: pallet_capita::GenesisConfig {
            params: test_params(),
        },
        identity: pallet_identity::GenesisConfig { identities },
        membership: pallet_membership::GenesisConfig { memberships },
        credential: pallet_capita_credential::GenesisConfig {
            issuers: vec![dev_issuer()],
        },
    })
}

/// One node: Alice produces the blocks and holds the sudo key.
pub fn development_config_genesis() -> Value {
    testnet_genesis(
        vec![(
            Sr25519Keyring::Alice.public().into(),
            Ed25519Keyring::Alice.public().into(),
        )],
        Sr25519Keyring::Alice.to_account_id(),
    )
}

/// Two nodes: Alice and Bob produce the blocks; Alice holds the sudo key.
pub fn local_config_genesis() -> Value {
    testnet_genesis(
        vec![
            (
                Sr25519Keyring::Alice.public().into(),
                Ed25519Keyring::Alice.public().into(),
            ),
            (
                Sr25519Keyring::Bob.public().into(),
                Ed25519Keyring::Bob.public().into(),
            ),
        ],
        Sr25519Keyring::Alice.to_account_id(),
    )
}

/// Provides the JSON representation of predefined genesis config for given `id`.
pub fn get_preset(id: &PresetId) -> Option<Vec<u8>> {
    let patch = match id.as_ref() {
        sp_genesis_builder::DEV_RUNTIME_PRESET => development_config_genesis(),
        sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET => local_config_genesis(),
        _ => return None,
    };
    serde_json::to_string(&patch)
        .ok()
        .map(|json| json.into_bytes())
}

/// List of supported presets.
pub fn preset_names() -> Vec<PresetId> {
    vec![
        PresetId::from(sp_genesis_builder::DEV_RUNTIME_PRESET),
        PresetId::from(sp_genesis_builder::LOCAL_TESTNET_RUNTIME_PRESET),
    ]
}
