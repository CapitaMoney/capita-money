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

use crate::{configs::MembershipPeriod, AccountId, IdtyIndex, Runtime, RuntimeGenesisConfig};
use alloc::{collections::BTreeMap, vec, vec::Vec};
use capita_core::attributes::VerificationLevel;
use capita_core::params::{
    CapParams, ClosedBalance, DemurrageParams, LadderStep, LoanParams, Params,
};
use capita_core::types::{Amount, Duration, ParamsVersion, Rate, Share, TierId, Timestamp};
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

/// A member identity for the genesis block.
fn member(index: IdtyIndex, name: &str, owner_key: AccountId) -> GenesisIdty<Runtime> {
    GenesisIdty {
        index,
        name: IdtyName::from(name),
        value: IdtyValue {
            data: (),
            next_creatable_identity_on: 0,
            old_owner_key: None,
            owner_key,
            next_scheduled: 0,
            status: IdtyStatus::Member,
        },
    }
}

/// Genesis of a test network: `initial_authorities` produce and finalise
/// blocks, `root` holds the sudo key, `members` start as identities with a
/// membership of one period.
fn testnet_genesis(
    initial_authorities: Vec<(AuraId, GrandpaId)>,
    root: AccountId,
    members: Vec<(&str, AccountId)>,
) -> Value {
    let identities: Vec<GenesisIdty<Runtime>> = members
        .into_iter()
        .zip(1..)
        .map(|((name, key), index)| member(index, name, key))
        .collect();
    let memberships: BTreeMap<IdtyIndex, MembershipData<crate::BlockNumber>> = identities
        .iter()
        .map(|idty| {
            (
                idty.index,
                MembershipData {
                    expire_on: MembershipPeriod::get(),
                },
            )
        })
        .collect();
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
    })
}

/// One node: Alice produces the blocks, holds the sudo key and is a member.
pub fn development_config_genesis() -> Value {
    testnet_genesis(
        vec![(
            Sr25519Keyring::Alice.public().into(),
            Ed25519Keyring::Alice.public().into(),
        )],
        Sr25519Keyring::Alice.to_account_id(),
        vec![("Alice", Sr25519Keyring::Alice.to_account_id())],
    )
}

/// Two nodes: Alice and Bob produce the blocks and are members; Alice holds
/// the sudo key.
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
        vec![
            ("Alice", Sr25519Keyring::Alice.to_account_id()),
            ("Bob", Sr25519Keyring::Bob.to_account_id()),
        ],
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
