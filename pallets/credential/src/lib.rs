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

//! # pallet-capita-credential
//!
//! Who is a person, on chain. The pallet keeps the public list of trusted
//! identity services (issuers) and accepts the claims they sign about the
//! holders of accounts. Design: `docs/adr/0011-identity-by-credentials.md`.
//!
//! The pallet decides nothing about a claim itself: `capita-credential`
//! checks it – the same code a service host runs. What the pallet adds is
//! what a host has to supply:
//! - **storage**: the issuer list, the claim that currently counts for each
//!   account, and every claim number seen (presented or taken back);
//! - **cryptography**: signature checks and the hash function of the node;
//! - **time**: the block timestamp;
//! - **authority**: who maintains the issuer list – an origin chosen by the
//!   runtime;
//! - **two neighbours**, each behind a trait: the identities of the chain
//!   ([`Memberships`]; on the test chain Duniter's identity and membership
//!   pallets) and the monetary rules ([`Rules`]; `pallet-capita`).
//!
//! A person presents a claim once ([`Pallet::present`]). From then on the
//! account is a member identity and carries the verification level of the
//! claim, until the claim expires, the issuer takes it back
//! ([`Pallet::revoke`]) or the issuer itself is struck off the list.
//!
//! No personal data reaches the chain: a claim holds a pseudonym, a level, a
//! country and numbered attributes.
//!
//! Open: weights are placeholders (no benchmarks yet).

#![cfg_attr(not(feature = "std"), no_std)]
// FRAME derives `Clone` for events and calls field by field. Clippy versions
// after 1.97 look into that generated code and report every `Copy` field.
#![allow(clippy::clone_on_copy)]
#![forbid(unsafe_code)]

extern crate alloc;

pub use pallet::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

use capita_core::attributes::Attestation;
use capita_credential::check::Crypto;
use capita_credential::types::{PublicKey, Signature};
use frame_support::pallet_prelude::DispatchResult;

/// The identities of the chain, as far as this pallet needs them.
///
/// On the test chain Duniter's identity and membership pallets stand behind
/// it (`runtime/src/identity.rs`); any other identity pallet can.
pub trait Memberships<AccountId> {
    /// Makes the holder of `who` a member: creates the identity if there is
    /// none – under `name`, or a name derived from the account – and starts
    /// or renews the membership.
    fn grant(who: &AccountId, name: Option<&[u8]>) -> DispatchResult;
    /// Ends the membership of `who`, if there is one.
    fn withdraw(who: &AccountId);
}

/// The monetary rules, as far as this pallet needs them: `pallet-capita`.
pub trait Rules<AccountId> {
    /// Records what an identity service states about the holder of `who`.
    /// `None`: nothing counts any longer.
    fn attest(who: &AccountId, attestation: Option<Attestation>) -> DispatchResult;
    /// Suspends the account: its identity was revoked.
    fn freeze(who: &AccountId) -> DispatchResult;
}

/// Signature checks and hashing of the node the runtime runs on.
pub struct HostCrypto;

impl Crypto for HostCrypto {
    fn verify(key: &PublicKey, message: &[u8], signature: &Signature) -> bool {
        match (key, signature) {
            (PublicKey::Ed25519(key), Signature::Ed25519(signature)) => {
                sp_io::crypto::ed25519_verify(
                    &sp_core::ed25519::Signature::from_raw(*signature),
                    message,
                    &sp_core::ed25519::Public::from_raw(*key),
                )
            }
            (PublicKey::Sr25519(key), Signature::Sr25519(signature)) => {
                sp_io::crypto::sr25519_verify(
                    &sp_core::sr25519::Signature::from_raw(*signature),
                    message,
                    &sp_core::sr25519::Public::from_raw(*key),
                )
            }
            _ => false,
        }
    }

    fn hash(data: &[u8]) -> [u8; 32] {
        sp_io::hashing::blake2_256(data)
    }
}

#[frame_support::pallet]
pub mod pallet {
    use alloc::vec::Vec;

    use capita_core::attributes::VerificationLevel;
    use capita_core::types::Timestamp;
    use capita_credential::check::{attestation, check_claim, check_revocation, still_counts};
    use capita_credential::errors::CredentialError;
    use capita_credential::types::{
        Audience, Claim, Issuer, IssuerId, IssuerStatus, Revocation, Serial, Signature,
    };
    use frame_support::pallet_prelude::*;
    use frame_support::storage::with_storage_layer;
    use frame_support::traits::UnixTime;
    use frame_system::pallet_prelude::*;
    use sp_runtime::traits::Zero;

    use super::{HostCrypto, Memberships, Rules};

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// What the runtime supplies. Account identifiers must be 32 bytes, the
    /// form a claim names its subject in.
    #[pallet::config]
    pub trait Config:
        frame_system::Config<RuntimeEvent: From<Event<Self>>, AccountId: Into<[u8; 32]>>
    {
        /// The clock: block time (`pallet-timestamp`).
        type Time: UnixTime;
        /// Who maintains the list of trusted identity services.
        ///
        /// Offene Frage: F3 – identity providers; E1 – who decides.
        type RegistryOrigin: EnsureOrigin<Self::RuntimeOrigin>;
        /// The identities of the chain.
        type Memberships: Memberships<Self::AccountId>;
        /// The monetary rules.
        type Rules: Rules<Self::AccountId>;
    }

    /// What became of a claim number.
    #[derive(
        Clone, Debug, PartialEq, Eq, Encode, Decode, DecodeWithMemTracking, MaxEncodedLen, TypeInfo,
    )]
    pub enum SerialState<AccountId> {
        /// Presented by this account. A claim is presented once.
        Presented(AccountId),
        /// Taken back by its issuer.
        Revoked,
    }

    /// Why a claim stopped counting.
    #[derive(
        Clone,
        Copy,
        Debug,
        PartialEq,
        Eq,
        Encode,
        Decode,
        DecodeWithMemTracking,
        MaxEncodedLen,
        TypeInfo,
    )]
    pub enum DropReason {
        /// Its issuer took it back.
        Revoked,
        /// It expired, or its issuer was struck off the list.
        Invalid,
        /// The identity of the account was revoked.
        IdentityRevoked,
    }

    /// The trusted identity services.
    #[pallet::storage]
    #[pallet::unbounded]
    pub type Issuers<T: Config> = StorageMap<_, Twox64Concat, IssuerId, Issuer>;

    /// The number the next issuer gets.
    #[pallet::storage]
    pub type NextIssuer<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// The claim that currently counts for an account.
    #[pallet::storage]
    #[pallet::unbounded]
    pub type Credentials<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, Claim>;

    /// Every claim number seen, per issuer: presented or taken back.
    #[pallet::storage]
    pub type Serials<T: Config> = StorageDoubleMap<
        _,
        Twox64Concat,
        IssuerId,
        Blake2_128Concat,
        Serial,
        SerialState<T::AccountId>,
    >;

    /// The identity services the chain trusts from its first block.
    #[pallet::genesis_config]
    #[derive(frame_support::DefaultNoBound)]
    pub struct GenesisConfig<T: Config> {
        /// Numbered from zero in this order.
        pub issuers: Vec<Issuer>,
        #[serde(skip)]
        pub _config: core::marker::PhantomData<T>,
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            for issuer in &self.issuers {
                assert!(issuer.validate().is_ok(), "invalid genesis issuer");
                let id = NextIssuer::<T>::get();
                Issuers::<T>::insert(IssuerId(id), issuer);
                NextIssuer::<T>::put(id.checked_add(1).expect("fewer than 2^32 issuers"));
            }
        }
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// An identity service was put on the list.
        IssuerRegistered {
            /// Its number.
            id: IssuerId,
        },
        /// The entry of an identity service was changed.
        IssuerUpdated {
            /// Its number.
            id: IssuerId,
            /// Whether it is trusted now.
            status: IssuerStatus,
        },
        /// An account presented a claim; it counts from now on.
        Presented {
            /// The account.
            account: T::AccountId,
            /// Who stated it.
            issuer: IssuerId,
            /// Number of the claim at the issuer.
            serial: Serial,
            /// Verification level stated.
            level: VerificationLevel,
            /// End of validity, if any.
            expires_at: Option<Timestamp>,
        },
        /// An account renewed its membership with the claim it holds.
        Renewed {
            /// The account.
            account: T::AccountId,
        },
        /// An issuer took a claim back.
        Revoked {
            /// The issuer.
            issuer: IssuerId,
            /// Number of the claim.
            serial: Serial,
        },
        /// The claim of an account stopped counting.
        Dropped {
            /// The account.
            account: T::AccountId,
            /// Why.
            reason: DropReason,
        },
    }

    /// Errors of the pallet itself, followed by the reasons for which a
    /// claim is refused (`capita_credential::errors::CredentialError`, one
    /// variant each).
    #[pallet::error]
    pub enum Error<T> {
        /// No identity service has this number.
        UnknownIssuer,
        /// More identity services than numbers.
        TooManyIssuers,
        /// The realm of an identity service cannot change: the persons it
        /// named would become other persons.
        RealmFixed,
        /// The claim is made out to another account.
        NotSubject,
        /// The claim was presented before.
        AlreadyPresented,
        /// The issuer took the claim back.
        ClaimRevoked,
        /// The account holds a claim issued later than this one.
        OlderThanCurrent,
        /// The account holds no claim.
        NoCredential,
        /// The claim of the account has expired, or its issuer was struck
        /// off the list.
        NoLongerCounts,
        /// The claim of the account still counts.
        StillCounts,
        /// The claim was taken back before.
        AlreadyRevoked,
        /// The issuer is suspended or revoked.
        IssuerNotActive,
        /// The claim names another issuer.
        WrongIssuer,
        /// No key of the issuer made this signature over this claim for
        /// this chain.
        BadSignature,
        /// The claim is dated in the future.
        NotYetValid,
        /// The claim has expired.
        Expired,
        /// The claim ends before it begins.
        ExpiryBeforeIssue,
        /// The claim holds longer than the issuer may state.
        ValidityTooLong,
        /// Level zero, or a level above what the issuer may state.
        LevelNotAllowed,
        /// The issuer may not state a residence.
        ResidenceNotAllowed,
        /// The residence was proven after the claim was issued.
        ResidenceFromFuture,
        /// The issuer may not state this attribute.
        AttributeNotAllowed,
        /// More attributes than a claim may carry.
        TooManyAttributes,
        /// Not a decentralised identifier.
        InvalidDid,
        /// A `did:key` identifier that does not belong to the key.
        DidKeyMismatch,
        /// An issuer without a key, or with too many.
        InvalidKeys,
        /// More attribute keys than an issuer entry may hold.
        TooManyAttributeKeys,
    }

    impl<T> From<CredentialError> for Error<T> {
        fn from(error: CredentialError) -> Self {
            match error {
                CredentialError::IssuerNotActive => Self::IssuerNotActive,
                CredentialError::WrongIssuer => Self::WrongIssuer,
                CredentialError::BadSignature => Self::BadSignature,
                CredentialError::NotYetValid => Self::NotYetValid,
                CredentialError::Expired => Self::Expired,
                CredentialError::ExpiryBeforeIssue => Self::ExpiryBeforeIssue,
                CredentialError::ValidityTooLong => Self::ValidityTooLong,
                CredentialError::LevelNotAllowed => Self::LevelNotAllowed,
                CredentialError::ResidenceNotAllowed => Self::ResidenceNotAllowed,
                CredentialError::ResidenceFromFuture => Self::ResidenceFromFuture,
                CredentialError::AttributeNotAllowed => Self::AttributeNotAllowed,
                CredentialError::TooManyAttributes => Self::TooManyAttributes,
                CredentialError::InvalidDid => Self::InvalidDid,
                CredentialError::DidKeyMismatch => Self::DidKeyMismatch,
                CredentialError::InvalidKeys => Self::InvalidKeys,
                CredentialError::TooManyAttributeKeys => Self::TooManyAttributeKeys,
            }
        }
    }

    /// Placeholder weight until the pallet is benchmarked: a fixed base –
    /// generous where a signature is checked – plus the database accesses.
    fn weight_of<T: Config>(base: u64, reads: u64, writes: u64) -> Weight {
        Weight::from_parts(base, 0).saturating_add(T::DbWeight::get().reads_writes(reads, writes))
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        #[cfg(feature = "try-runtime")]
        fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
            Self::do_try_state().map_err(Into::into)
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Present a claim an identity service made out to the caller. The
        /// account becomes a member identity – named `name` if it had no
        /// identity, or after its key if no name is given – and carries the
        /// verification level of the claim. A claim presented later
        /// replaces the one held.
        ///
        /// The account must be open (`capita.open`).
        #[pallet::call_index(0)]
        #[pallet::weight(weight_of::<T>(300_000_000, 12, 12))]
        pub fn present(
            origin: OriginFor<T>,
            claim: Claim,
            signature: Signature,
            name: Option<Vec<u8>>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            ensure!(
                who.clone().into() == claim.subject.0,
                Error::<T>::NotSubject
            );
            let issuer = Issuers::<T>::get(claim.issuer).ok_or(Error::<T>::UnknownIssuer)?;
            match Serials::<T>::get(claim.issuer, claim.serial) {
                Some(SerialState::Presented(_)) => return Err(Error::<T>::AlreadyPresented.into()),
                Some(SerialState::Revoked) => return Err(Error::<T>::ClaimRevoked.into()),
                None => {}
            }
            if let Some(held) = Credentials::<T>::get(&who) {
                ensure!(
                    claim.issued_at >= held.issued_at,
                    Error::<T>::OlderThanCurrent
                );
            }
            let stated = check_claim::<HostCrypto>(
                &claim,
                &signature,
                claim.issuer,
                &issuer,
                &Self::audience(),
                Self::now(),
            )
            .map_err(Self::refused)?;
            T::Rules::attest(&who, Some(stated))?;
            T::Memberships::grant(&who, name.as_deref())?;
            Serials::<T>::insert(
                claim.issuer,
                claim.serial,
                SerialState::Presented(who.clone()),
            );
            Self::deposit_event(Event::Presented {
                account: who.clone(),
                issuer: claim.issuer,
                serial: claim.serial,
                level: claim.level,
                expires_at: claim.expires_at,
            });
            Credentials::<T>::insert(who, claim);
            Ok(())
        }

        /// Renew the membership of the caller with the claim the account
        /// holds, as long as that claim counts. Needed when a membership
        /// ran out before the claim did.
        #[pallet::call_index(1)]
        #[pallet::weight(weight_of::<T>(100_000_000, 10, 10))]
        pub fn renew(origin: OriginFor<T>) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let claim = Credentials::<T>::get(&who).ok_or(Error::<T>::NoCredential)?;
            let issuer = Issuers::<T>::get(claim.issuer).ok_or(Error::<T>::UnknownIssuer)?;
            ensure!(
                still_counts(&claim, &issuer, Self::now()),
                Error::<T>::NoLongerCounts
            );
            let stated = attestation::<HostCrypto>(&claim, issuer.realm);
            T::Rules::attest(&who, Some(stated))?;
            T::Memberships::grant(&who, None)?;
            Self::deposit_event(Event::Renewed { account: who });
            Ok(())
        }

        /// Take a claim back, with the signature of its issuer. Anybody may
        /// hand the revocation in. If the claim is the one an account holds,
        /// the account loses its membership and its verification level; a
        /// claim not yet presented can no longer be.
        #[pallet::call_index(2)]
        #[pallet::weight(weight_of::<T>(300_000_000, 10, 10))]
        pub fn revoke(
            origin: OriginFor<T>,
            revocation: Revocation,
            signature: Signature,
        ) -> DispatchResult {
            ensure_signed(origin)?;
            let Revocation { issuer: id, serial } = revocation;
            let issuer = Issuers::<T>::get(id).ok_or(Error::<T>::UnknownIssuer)?;
            check_revocation::<HostCrypto>(&revocation, &signature, id, &issuer, &Self::audience())
                .map_err(Self::refused)?;
            let holder = match Serials::<T>::get(id, serial) {
                Some(SerialState::Revoked) => return Err(Error::<T>::AlreadyRevoked.into()),
                Some(SerialState::Presented(account)) => Some(account),
                None => None,
            };
            Serials::<T>::insert(id, serial, SerialState::Revoked);
            Self::deposit_event(Event::Revoked { issuer: id, serial });
            if let Some(account) = holder {
                let holds = Credentials::<T>::get(&account)
                    .is_some_and(|held| held.issuer == id && held.serial == serial);
                if holds {
                    Self::drop_credential(&account, DropReason::Revoked);
                }
            }
            Ok(())
        }

        /// Clear away the claim of `account` once it counts no longer – it
        /// expired, or its issuer was struck off the list. Anybody may call
        /// this; the account loses its membership.
        #[pallet::call_index(3)]
        #[pallet::weight(weight_of::<T>(100_000_000, 10, 10))]
        pub fn drop_invalid(origin: OriginFor<T>, account: T::AccountId) -> DispatchResult {
            ensure_signed(origin)?;
            let claim = Credentials::<T>::get(&account).ok_or(Error::<T>::NoCredential)?;
            let counts = Issuers::<T>::get(claim.issuer)
                .is_some_and(|issuer| still_counts(&claim, &issuer, Self::now()));
            ensure!(!counts, Error::<T>::StillCounts);
            Self::drop_credential(&account, DropReason::Invalid);
            Ok(())
        }

        /// Put an identity service on the list. Its number is the next
        /// free one.
        #[pallet::call_index(10)]
        #[pallet::weight(weight_of::<T>(50_000_000, 1, 2))]
        pub fn register_issuer(origin: OriginFor<T>, issuer: Issuer) -> DispatchResult {
            T::RegistryOrigin::ensure_origin(origin)?;
            issuer.validate().map_err(Self::refused)?;
            let id = NextIssuer::<T>::get();
            let next = id.checked_add(1).ok_or(Error::<T>::TooManyIssuers)?;
            NextIssuer::<T>::put(next);
            Issuers::<T>::insert(IssuerId(id), issuer);
            Self::deposit_event(Event::IssuerRegistered { id: IssuerId(id) });
            Ok(())
        }

        /// Replace the entry of an identity service: other keys, other
        /// limits, suspended, revoked or active again. Its realm stays.
        ///
        /// Limits apply to claims presented from now on. Striking an issuer
        /// off (`Revoked`) ends what it stated: `drop_invalid` clears each
        /// claim away.
        #[pallet::call_index(11)]
        #[pallet::weight(weight_of::<T>(50_000_000, 1, 1))]
        pub fn update_issuer(origin: OriginFor<T>, id: IssuerId, issuer: Issuer) -> DispatchResult {
            T::RegistryOrigin::ensure_origin(origin)?;
            let previous = Issuers::<T>::get(id).ok_or(Error::<T>::UnknownIssuer)?;
            ensure!(previous.realm == issuer.realm, Error::<T>::RealmFixed);
            issuer.validate().map_err(Self::refused)?;
            let status = issuer.status;
            Issuers::<T>::insert(id, issuer);
            Self::deposit_event(Event::IssuerUpdated { id, status });
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        /// Block time in whole seconds.
        pub fn now() -> Timestamp {
            Timestamp::from_secs(T::Time::now().as_secs())
        }

        /// What claims for this chain are made out to: the hash of its
        /// first block.
        pub fn audience() -> Audience {
            let genesis = frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero());
            let mut out = [0u8; 32];
            for (to, from) in out.iter_mut().zip(genesis.as_ref()) {
                *to = *from;
            }
            Audience(out)
        }

        /// For the identity pallets of the runtime: the membership of `who`
        /// ended. The account counts as unverified until a claim is
        /// presented or the membership renewed.
        pub fn membership_ended(who: &T::AccountId) {
            // Called from hooks, where nothing rolls back on its own.
            let _ = with_storage_layer(|| T::Rules::attest(who, None));
        }

        /// For the identity pallets of the runtime: the identity of `who`
        /// was revoked. The claim stops counting and the account is frozen.
        pub fn identity_revoked(who: &T::AccountId) {
            if Credentials::<T>::take(who).is_some() {
                Self::deposit_event(Event::Dropped {
                    account: who.clone(),
                    reason: DropReason::IdentityRevoked,
                });
            }
            let _ = with_storage_layer(|| T::Rules::freeze(who));
        }

        fn refused(error: CredentialError) -> DispatchError {
            Error::<T>::from(error).into()
        }

        /// The claim of `account` stops counting: no membership, no level.
        fn drop_credential(account: &T::AccountId, reason: DropReason) {
            Credentials::<T>::remove(account);
            T::Memberships::withdraw(account);
            // The runtime reports the end of a membership as well; said here
            // too, the result does not depend on how it is wired.
            Self::membership_ended(account);
            Self::deposit_event(Event::Dropped {
                account: account.clone(),
                reason,
            });
        }

        /// The invariants of the state. Run after every block by
        /// `try-runtime`, and by the tests.
        #[cfg(any(test, feature = "try-runtime"))]
        pub fn do_try_state() -> Result<(), &'static str> {
            let mut highest = None;
            for (id, issuer) in Issuers::<T>::iter() {
                if issuer.validate().is_err() {
                    return Err("invalid issuer on the list");
                }
                highest = highest.max(Some(id.0));
            }
            if highest.is_some_and(|id| id >= NextIssuer::<T>::get()) {
                return Err("issuer numbered beyond the counter");
            }
            for (account, claim) in Credentials::<T>::iter() {
                if account.clone().into() != claim.subject.0 {
                    return Err("claim held by an account it is not made out to");
                }
                if !Issuers::<T>::contains_key(claim.issuer) {
                    return Err("claim of an unknown issuer");
                }
                if Serials::<T>::get(claim.issuer, claim.serial)
                    != Some(SerialState::Presented(account))
                {
                    return Err("claim held without its number being recorded");
                }
            }
            Ok(())
        }
    }
}
