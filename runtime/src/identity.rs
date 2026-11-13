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

//! Identities by credentials: Duniter's identity and membership pallets,
//! steered by `pallet-capita-credential` instead of a web of trust.
//!
//! Wiring only (`docs/adr/0011-identity-by-credentials.md`). In Duniter a
//! member certifies a newcomer, and a distance rule decides who is a member.
//! Here an identity service states who is a person, and the credential
//! pallet decides what counts. The two Duniter pallets are used unchanged:
//! everything below reaches them through their calls, their public
//! functions and the handler types of their configuration.

use alloc::vec::Vec;

use capita_core::account::AccountStatus;
use capita_core::attributes::Attestation;
use frame_support::pallet_prelude::{DispatchError, DispatchResult};
use frame_support::weights::Weight;
use frame_support::PalletId;
use pallet_identity::{IdtyName, IdtyStatus};
use pallet_membership::MembershipRemovalReason;
use sp_runtime::traits::AccountIdConversion;

use crate::{
    AccountId, Capita, Credential, Identity, IdtyIndex, Membership, Runtime, RuntimeOrigin,
};

/// Duniter's identity pallet lets only a member identity create another
/// one: the first certifier. On this chain that member is no person but
/// this account, which nobody holds a key for; the runtime acts through it
/// when a claim was accepted. It is identity 1 of the genesis block.
pub const CREDENTIAL_PALLET_ID: PalletId = PalletId(*b"cap/cred");

/// Name of the identity of [`creator`].
pub const CREATOR_NAME: &str = "capita-credentials";

/// The account in whose name identities are created.
pub fn creator() -> AccountId {
    CREDENTIAL_PALLET_ID.into_account_truncating()
}

/// The name an identity gets when its holder chooses none: `0x` and the
/// first 20 bytes of the account in hexadecimal. Chosen names may not start
/// with `0x`, so nobody can take the derived name of somebody else.
pub fn derived_name(who: &AccountId) -> IdtyName {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes: &[u8; 32] = who.as_ref();
    let mut name = Vec::with_capacity(42);
    name.extend_from_slice(b"0x");
    for byte in bytes.iter().take(20) {
        name.push(HEX[usize::from(byte >> 4)]);
        name.push(HEX[usize::from(byte & 0x0f)]);
    }
    IdtyName(name)
}

fn owner(idty_index: IdtyIndex) -> Option<AccountId> {
    Identity::identity(idty_index).map(|value| value.owner_key)
}

/// The identities of the chain for the credential pallet: Duniter's.
pub struct DuniterIdentities;

impl pallet_capita_credential::Memberships<AccountId> for DuniterIdentities {
    fn grant(who: &AccountId, name: Option<&[u8]>) -> DispatchResult {
        let idty_index = match Identity::identity_index_of(who) {
            Some(idty_index) => idty_index,
            None => {
                Identity::create_identity(RuntimeOrigin::signed(creator()), who.clone())?;
                Identity::identity_index_of(who)
                    .ok_or(DispatchError::Other("identity was not created"))?
            }
        };
        let status = Identity::identity(idty_index)
            .ok_or(DispatchError::Other("identity was not created"))?
            .status;
        match status {
            // Final in Duniter's life cycle of an identity.
            IdtyStatus::Revoked => return Err(DispatchError::Other("identity revoked")),
            IdtyStatus::Unconfirmed => {
                let name = match name {
                    Some(name) if name.starts_with(b"0x") => {
                        return Err(DispatchError::Other("names starting with 0x are reserved"))
                    }
                    Some(name) => IdtyName(name.to_vec()),
                    None => derived_name(who),
                };
                Identity::confirm_identity(RuntimeOrigin::signed(who.clone()), name)?;
            }
            IdtyStatus::Unvalidated | IdtyStatus::Member | IdtyStatus::NotMember => {}
        }
        if Membership::is_member(&idty_index) {
            Membership::try_renew_membership(idty_index)
        } else {
            Membership::try_add_membership(idty_index)
        }
    }

    fn withdraw(who: &AccountId) {
        if let Some(idty_index) = Identity::identity_index_of(who) {
            Membership::do_remove_membership(idty_index, MembershipRemovalReason::Revoked);
        }
    }
}

/// The monetary rules for the credential pallet: `pallet-capita`.
pub struct CapitaRules;

impl pallet_capita_credential::Rules<AccountId> for CapitaRules {
    fn attest(who: &AccountId, attestation: Option<Attestation>) -> DispatchResult {
        Capita::do_attest(Capita::id(who.clone()), attestation)
    }

    fn freeze(who: &AccountId) -> DispatchResult {
        Capita::do_set_status(Capita::id(who.clone()), AccountStatus::Frozen)
    }
}

/// Stands where Duniter's runtime has its web of trust: keeps identity and
/// membership of a person in step, and tells the credential pallet when a
/// membership ends or an identity is revoked.
pub struct CredentialLink;

impl sp_membership::traits::OnNewMembership<IdtyIndex> for CredentialLink {
    fn on_created(idty_index: &IdtyIndex) {
        Identity::membership_added(*idty_index);
    }

    fn on_renewed(_idty_index: &IdtyIndex) {}
}

impl sp_membership::traits::OnRemoveMembership<IdtyIndex> for CredentialLink {
    fn on_removed(idty_index: &IdtyIndex) -> Weight {
        let weight = Identity::membership_removed(*idty_index);
        if let Some(owner) = owner(*idty_index) {
            Credential::membership_ended(&owner);
        }
        weight
    }
}

impl pallet_identity::traits::OnRemoveIdty<Runtime> for CredentialLink {
    fn on_removed(idty_index: &IdtyIndex) -> Weight {
        Membership::do_remove_membership(*idty_index, MembershipRemovalReason::System)
    }

    fn on_revoked(idty_index: &IdtyIndex) -> Weight {
        let weight =
            Membership::do_remove_membership(*idty_index, MembershipRemovalReason::Revoked);
        // The identity pallet reports a rejected invitation here as well;
        // only a revocation concerns the account.
        if let Some(value) = Identity::identity(*idty_index) {
            if value.status == IdtyStatus::Revoked {
                Credential::identity_revoked(&value.owner_key);
            }
        }
        weight
    }
}

/// Identities come from claims: nobody creates one by hand, as Duniter's
/// members do for newcomers.
impl pallet_identity::traits::CheckIdtyCallAllowed<Runtime> for CredentialLink {
    fn check_create_identity(creator_index: IdtyIndex) -> Result<(), DispatchError> {
        if owner(creator_index) == Some(creator()) {
            Ok(())
        } else {
            Err(DispatchError::Other(
                "identities are created by presenting a claim",
            ))
        }
    }
}

/// An identity stays with its key. Moving it would leave the account, its
/// money and its claim behind with the old key.
///
/// Offene Frage: B5 – lost or stolen key: moving an account to a new key
/// after a new verification.
impl pallet_identity::traits::KeyChange<Runtime> for CredentialLink {
    fn on_changed(_idty_index: IdtyIndex, _account: AccountId) -> Result<(), DispatchError> {
        Err(DispatchError::Other(
            "an identity cannot move to another key",
        ))
    }
}
