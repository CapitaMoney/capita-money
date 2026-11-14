// Capita Money – pallet-capita-keys
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

//! # pallet-capita-keys
//!
//! Accounts acted for by the keys of a person's devices
//! (`docs/adr/0020-keys-passkeys-linked-devices.md`): passkeys (WebAuthn,
//! P-256) and device keys (Ed25519), up to [`Config::MaxKeys`] per account.
//!
//! - [`Pallet::open`]: the first key opens the account derived from it
//!   (`capita_keys::encoding::account_of`) and acts once.
//! - [`Pallet::act`]: a key of an account signs a call; the pallet runs it
//!   with origin `Signed(account)`. Every call that works for a signed
//!   account works this way – no other pallet changes.
//! - Both are transactions **without signer and without fee**, like
//!   `door.redeem`: the signature of the key is the authority. Each action
//!   carries the account's next nonce, so it runs once.
//! - Devices: [`Pallet::add_key`] (the new key proves it is held),
//!   [`Pallet::remove_key`]. The first key is the *home key*: only it
//!   removes other keys; any key removes itself. A linked key becomes the
//!   home key after [`Config::HomeDelay`] ([`Pallet::claim_home`],
//!   [`Pallet::take_home`]) unless the home key acts in between.
//!
//! Which sites a passkey may come from – relying party ids and origins – is
//! a list kept by [`Config::PolicyOrigin`].
//!
//! Open: weights are placeholders (no benchmarks yet); P-256 is checked in
//! the runtime itself, not by a host function.

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

/// The Ed25519 check of the node the runtime runs on.
pub struct HostVerify;

impl capita_keys::check::Verify for HostVerify {
    fn ed25519(key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
        sp_io::crypto::ed25519_verify(
            &sp_core::ed25519::Signature::from_raw(*signature),
            message,
            &sp_core::ed25519::Public::from_raw(*key),
        )
    }
}

#[frame_support::pallet]
pub mod pallet {
    use alloc::boxed::Box;
    use alloc::string::String;
    use alloc::vec::Vec;

    use capita_keys::check::verify_proof;
    use capita_keys::encoding::{account_of, act_challenge, add_challenge};
    use capita_keys::types::{Audience, DeviceKey, Proof};
    use frame_support::dispatch::{GetDispatchInfo, PostDispatchInfo};
    use frame_support::pallet_prelude::*;
    use frame_support::traits::{IsSubType, UnixTime};
    use frame_system::pallet_prelude::*;
    use sp_runtime::traits::{Dispatchable, Zero};

    use super::HostVerify;

    /// Most relying party ids on the list.
    pub type MaxRelyingParties = ConstU32<4>;
    /// Most origins on the list.
    pub type MaxOrigins = ConstU32<8>;
    /// Longest origin, in bytes.
    pub type MaxOriginLength = ConstU32<128>;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// What the runtime supplies. Account identifiers must be 32 bytes, the
    /// form `capita-keys` derives.
    #[pallet::config]
    pub trait Config:
        frame_system::Config<
        RuntimeEvent: From<Event<Self>>,
        AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    >
    {
        /// The calls a key can make for its account.
        type RuntimeCall: Parameter
            + Dispatchable<RuntimeOrigin = Self::RuntimeOrigin, PostInfo = PostDispatchInfo>
            + GetDispatchInfo
            + From<frame_system::Call<Self>>
            + IsSubType<Call<Self>>
            + IsType<<Self as frame_system::Config>::RuntimeCall>;
        /// Who keeps the list of sites passkeys may come from.
        type PolicyOrigin: EnsureOrigin<Self::RuntimeOrigin>;
        /// The time of the chain.
        type Time: UnixTime;
        /// Most keys of one account (rule `keys.max_keys`).
        #[pallet::constant]
        type MaxKeys: Get<u32>;
        /// How long a linked key waits before it becomes the home key, in
        /// seconds (rule `keys.home_delay`).
        type HomeDelay: Get<u64>;
        /// Priority of an action (no signer, no fee) in the pool.
        #[pallet::constant]
        type UnsignedPriority: Get<TransactionPriority>;
    }

    /// The keys of an account; the first is the home key.
    #[pallet::storage]
    pub type Keys<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, BoundedVec<DeviceKey, T::MaxKeys>, ValueQuery>;

    /// Which account a key acts for. A key acts for one account only.
    #[pallet::storage]
    pub type KeyOwner<T: Config> = StorageMap<_, Blake2_128Concat, DeviceKey, T::AccountId>;

    /// The nonce the next action of an account must carry.
    #[pallet::storage]
    pub type Nonce<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, u32, ValueQuery>;

    /// A linked key that asked to become the home key, and since when
    /// (seconds of the chain's clock).
    #[pallet::storage]
    pub type HomeClaim<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, (DeviceKey, u64)>;

    /// SHA-256 of the relying party ids whose passkeys count.
    #[pallet::storage]
    pub type RelyingParties<T: Config> =
        StorageValue<_, BoundedVec<[u8; 32], MaxRelyingParties>, ValueQuery>;

    /// The web origins passkey assertions may come from.
    #[pallet::storage]
    pub type Origins<T: Config> =
        StorageValue<_, BoundedVec<BoundedVec<u8, MaxOriginLength>, MaxOrigins>, ValueQuery>;

    /// The key whose action runs right now; empty outside an action.
    #[pallet::storage]
    pub type Acting<T: Config> = StorageValue<_, DeviceKey>;

    /// The sites passkeys may come from when the chain starts.
    #[pallet::genesis_config]
    #[derive(frame_support::DefaultNoBound)]
    pub struct GenesisConfig<T: Config> {
        /// Relying party ids (domain names), such as `app.capitamoney.com`.
        pub relying_parties: Vec<String>,
        /// Origins, such as `https://app.capitamoney.com`.
        pub origins: Vec<String>,
        #[serde(skip)]
        pub _config: core::marker::PhantomData<T>,
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            let parties: Vec<Vec<u8>> = self.relying_parties.iter().map(|s| s.as_bytes().to_vec()).collect();
            let origins: Vec<Vec<u8>> = self.origins.iter().map(|s| s.as_bytes().to_vec()).collect();
            Pallet::<T>::store_policy(parties, origins).expect("a short list of sites");
        }
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A first key opened an account.
        Opened {
            /// The account.
            account: T::AccountId,
            /// Its first key, the home key.
            key: DeviceKey,
        },
        /// A key acted for its account.
        Acted {
            /// The account.
            account: T::AccountId,
            /// The key.
            key: DeviceKey,
            /// The nonce the action carried.
            nonce: u32,
            /// What the call answered.
            result: DispatchResult,
        },
        /// A key was linked to an account.
        KeyAdded {
            /// The account.
            account: T::AccountId,
            /// The new key.
            key: DeviceKey,
        },
        /// A key no longer acts for an account.
        KeyRemoved {
            /// The account.
            account: T::AccountId,
            /// The key.
            key: DeviceKey,
        },
        /// A linked key asked to become the home key.
        HomeClaimed {
            /// The account.
            account: T::AccountId,
            /// The key that asked.
            key: DeviceKey,
        },
        /// The home key acted: the claim is withdrawn.
        HomeRefused {
            /// The account.
            account: T::AccountId,
        },
        /// Another key is the home key now.
        HomeChanged {
            /// The account.
            account: T::AccountId,
            /// The new home key.
            key: DeviceKey,
        },
        /// The list of sites passkeys may come from changed.
        PolicyChanged,
    }

    #[pallet::error]
    pub enum Error<T> {
        /// The key acts for an account already.
        KeyTaken,
        /// The account of this first key is open already.
        AccountExists,
        /// The key does not act for this account.
        UnknownKey,
        /// The action carries another nonce than the account's next one.
        BadNonce,
        /// The signature does not fit key, action and chain, or the passkey
        /// came from a site not on the list.
        BadProof,
        /// An action cannot contain an action.
        NestedCall,
        /// The account has the most keys it may have.
        TooManyKeys,
        /// This call is made by a key of the account, through `act`.
        NotByKey,
        /// Only the home key removes other keys.
        NotHome,
        /// The last key of an account is not removed: a lost key is
        /// replaced by verifying again (ADR 0013).
        LastKey,
        /// No claim to become the home key, or one by another key.
        NoClaim,
        /// The claim has not waited long enough.
        TooEarly,
        /// The key is the home key already.
        AlreadyHome,
        /// Too many sites, or a site name too long.
        PolicyTooLong,
    }

    /// Placeholder weight until the pallet is benchmarked.
    fn weight_of<T: Config>(base: u64, reads: u64, writes: u64) -> Weight {
        Weight::from_parts(base, 0).saturating_add(T::DbWeight::get().reads_writes(reads, writes))
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Run `call` for `account`, signed by one of its keys. Without
        /// signer and without fee; once (`nonce`).
        #[pallet::call_index(0)]
        #[pallet::weight({
            let inner = call.get_dispatch_info();
            weight_of::<T>(300_000_000, 6, 4).saturating_add(inner.call_weight)
        })]
        pub fn act(
            origin: OriginFor<T>,
            account: T::AccountId,
            key: DeviceKey,
            call: Box<<T as Config>::RuntimeCall>,
            nonce: u32,
            proof: Proof,
        ) -> DispatchResult {
            ensure_none(origin)?;
            Self::check_act(&account, &key, &call, nonce, &proof)?;
            Nonce::<T>::insert(&account, nonce.saturating_add(1));
            Self::run(account, key, *call, nonce);
            Ok(())
        }

        /// Open the account derived from `key` and run its first call
        /// (nonce 0). Without signer and without fee.
        #[pallet::call_index(1)]
        #[pallet::weight({
            let inner = call.get_dispatch_info();
            weight_of::<T>(300_000_000, 6, 6).saturating_add(inner.call_weight)
        })]
        pub fn open(
            origin: OriginFor<T>,
            key: DeviceKey,
            call: Box<<T as Config>::RuntimeCall>,
            proof: Proof,
        ) -> DispatchResult {
            ensure_none(origin)?;
            let account = Self::check_open(&key, &call, &proof)?;
            let mut keys = BoundedVec::<DeviceKey, T::MaxKeys>::new();
            keys.try_push(key).map_err(|_| Error::<T>::TooManyKeys)?;
            Keys::<T>::insert(&account, keys);
            KeyOwner::<T>::insert(key, &account);
            Nonce::<T>::insert(&account, 1);
            Self::deposit_event(Event::Opened {
                account: account.clone(),
                key,
            });
            Self::run(account, key, *call, 0);
            Ok(())
        }

        /// Link another key to the account. `possession` is the new key's
        /// signature over `capita_keys::encoding::add_challenge`.
        #[pallet::call_index(2)]
        #[pallet::weight(weight_of::<T>(300_000_000, 3, 2))]
        pub fn add_key(origin: OriginFor<T>, key: DeviceKey, possession: Proof) -> DispatchResult {
            let (account, _) = Self::acting(origin)?;
            ensure!(!KeyOwner::<T>::contains_key(key), Error::<T>::KeyTaken);
            let challenge = add_challenge(&Self::audience(), &account.clone().into(), &key);
            Self::verify(&key, &challenge, &possession)?;
            Keys::<T>::try_mutate(&account, |keys| {
                keys.try_push(key).map_err(|_| Error::<T>::TooManyKeys)
            })?;
            KeyOwner::<T>::insert(key, &account);
            Self::deposit_event(Event::KeyAdded { account, key });
            Ok(())
        }

        /// Unlink a key. The home key removes any other key; any key
        /// removes itself; the last key stays. Removing the home key makes
        /// the next key the home key.
        #[pallet::call_index(3)]
        #[pallet::weight(weight_of::<T>(50_000_000, 3, 3))]
        pub fn remove_key(origin: OriginFor<T>, key: DeviceKey) -> DispatchResult {
            let (account, acting) = Self::acting(origin)?;
            let keys = Keys::<T>::get(&account);
            ensure!(keys.contains(&key), Error::<T>::UnknownKey);
            ensure!(
                acting == key || keys.first() == Some(&acting),
                Error::<T>::NotHome
            );
            ensure!(keys.len() > 1, Error::<T>::LastKey);
            Keys::<T>::mutate(&account, |keys| keys.retain(|k| *k != key));
            KeyOwner::<T>::remove(key);
            if HomeClaim::<T>::get(&account).is_some_and(|(k, _)| k == key) {
                HomeClaim::<T>::remove(&account);
            }
            Self::deposit_event(Event::KeyRemoved { account, key });
            Ok(())
        }

        /// A linked key asks to become the home key (the phone with the
        /// home key is lost). It may take over after [`Config::HomeDelay`],
        /// unless the home key acts in between.
        #[pallet::call_index(4)]
        #[pallet::weight(weight_of::<T>(50_000_000, 3, 1))]
        pub fn claim_home(origin: OriginFor<T>) -> DispatchResult {
            let (account, acting) = Self::acting(origin)?;
            ensure!(!Self::is_home(&account, &acting), Error::<T>::AlreadyHome);
            HomeClaim::<T>::insert(&account, (acting, Self::now()));
            Self::deposit_event(Event::HomeClaimed { account, key: acting });
            Ok(())
        }

        /// The key that claimed the home takes it, once the delay is over.
        #[pallet::call_index(5)]
        #[pallet::weight(weight_of::<T>(50_000_000, 3, 2))]
        pub fn take_home(origin: OriginFor<T>) -> DispatchResult {
            let (account, acting) = Self::acting(origin)?;
            let (claimer, since) = HomeClaim::<T>::get(&account).ok_or(Error::<T>::NoClaim)?;
            ensure!(claimer == acting, Error::<T>::NoClaim);
            ensure!(
                Self::now() >= since.saturating_add(T::HomeDelay::get()),
                Error::<T>::TooEarly
            );
            Keys::<T>::try_mutate(&account, |keys| {
                keys.retain(|k| *k != acting);
                keys.try_insert(0, acting).map_err(|_| Error::<T>::TooManyKeys)
            })?;
            HomeClaim::<T>::remove(&account);
            Self::deposit_event(Event::HomeChanged { account, key: acting });
            Ok(())
        }

        /// The home key says no to a claim. (Any action of the home key
        /// withdraws a claim; this call is the action that does nothing
        /// else.)
        #[pallet::call_index(6)]
        #[pallet::weight(weight_of::<T>(20_000_000, 2, 0))]
        pub fn refuse_home(origin: OriginFor<T>) -> DispatchResult {
            let (account, acting) = Self::acting(origin)?;
            ensure!(Self::is_home(&account, &acting), Error::<T>::NotHome);
            Ok(())
        }

        /// Replace the list of sites passkeys may come from: relying party
        /// ids (domain names) and origins.
        #[pallet::call_index(10)]
        #[pallet::weight(weight_of::<T>(20_000_000, 0, 2))]
        pub fn set_policy(
            origin: OriginFor<T>,
            relying_parties: Vec<Vec<u8>>,
            origins: Vec<Vec<u8>>,
        ) -> DispatchResult {
            T::PolicyOrigin::ensure_origin(origin)?;
            Self::store_policy(relying_parties, origins)?;
            Self::deposit_event(Event::PolicyChanged);
            Ok(())
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;

        /// An action enters the pool if it would run: listed key, the
        /// account's next nonce, valid signature. One transaction per
        /// account and nonce (`provides`).
        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            match call {
                Call::act {
                    account,
                    key,
                    call: inner,
                    nonce,
                    proof,
                } => match Self::check_act(account, key, inner, *nonce, proof) {
                    Ok(()) => Self::valid(account, *nonce),
                    Err(error) if error == Error::<T>::BadNonce.into() => {
                        if *nonce < Nonce::<T>::get(account) {
                            InvalidTransaction::Stale.into()
                        } else {
                            InvalidTransaction::Future.into()
                        }
                    }
                    Err(_) => InvalidTransaction::BadProof.into(),
                },
                Call::open {
                    key,
                    call: inner,
                    proof,
                } => match Self::check_open(key, inner, proof) {
                    Ok(account) => Self::valid(&account, 0),
                    Err(error)
                        if error == Error::<T>::KeyTaken.into()
                            || error == Error::<T>::AccountExists.into() =>
                    {
                        InvalidTransaction::Stale.into()
                    }
                    Err(_) => InvalidTransaction::BadProof.into(),
                },
                _ => InvalidTransaction::Call.into(),
            }
        }
    }

    impl<T: Config> Pallet<T> {
        /// What keys sign for this chain: the hash of its first block.
        pub fn audience() -> Audience {
            let genesis = frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero());
            let mut out = [0u8; 32];
            for (to, from) in out.iter_mut().zip(genesis.as_ref()) {
                *to = *from;
            }
            Audience(out)
        }

        /// Whether `key` is the home key of `account`.
        pub fn is_home(account: &T::AccountId, key: &DeviceKey) -> bool {
            Keys::<T>::get(account).first() == Some(key)
        }

        fn now() -> u64 {
            T::Time::now().as_secs()
        }

        fn valid(account: &T::AccountId, nonce: u32) -> TransactionValidity {
            ValidTransaction::with_tag_prefix("CapitaKeys")
                .priority(T::UnsignedPriority::get())
                .and_provides((account, nonce))
                .longevity(64)
                .propagate(true)
                .build()
        }

        fn nested(call: &<T as Config>::RuntimeCall) -> bool {
            matches!(
                call.is_sub_type(),
                Some(Call::act { .. } | Call::open { .. })
            )
        }

        fn verify(key: &DeviceKey, challenge: &[u8; 32], proof: &Proof) -> DispatchResult {
            let parties = RelyingParties::<T>::get();
            let origins = Origins::<T>::get();
            verify_proof::<HostVerify>(key, challenge, proof, &parties, |o| {
                origins.iter().any(|x| x.as_slice() == o)
            })
            .map_err(|_| Error::<T>::BadProof.into())
        }

        /// Everything that decides whether an action runs, without changing
        /// anything.
        fn check_act(
            account: &T::AccountId,
            key: &DeviceKey,
            call: &<T as Config>::RuntimeCall,
            nonce: u32,
            proof: &Proof,
        ) -> DispatchResult {
            ensure!(!Self::nested(call), Error::<T>::NestedCall);
            ensure!(
                KeyOwner::<T>::get(key).as_ref() == Some(account),
                Error::<T>::UnknownKey
            );
            ensure!(Nonce::<T>::get(account) == nonce, Error::<T>::BadNonce);
            let call_hash = sp_io::hashing::blake2_256(&call.encode());
            let challenge = act_challenge(&Self::audience(), &account.clone().into(), nonce, &call_hash);
            Self::verify(key, &challenge, proof)
        }

        /// Everything that decides whether a first key opens its account.
        /// Answers the account.
        fn check_open(
            key: &DeviceKey,
            call: &<T as Config>::RuntimeCall,
            proof: &Proof,
        ) -> Result<T::AccountId, DispatchError> {
            ensure!(!Self::nested(call), Error::<T>::NestedCall);
            ensure!(!KeyOwner::<T>::contains_key(key), Error::<T>::KeyTaken);
            let raw = account_of(key);
            let account = T::AccountId::from(raw);
            ensure!(!Keys::<T>::contains_key(&account), Error::<T>::AccountExists);
            let call_hash = sp_io::hashing::blake2_256(&call.encode());
            let challenge = act_challenge(&Self::audience(), &raw, 0, &call_hash);
            Self::verify(key, &challenge, proof)?;
            Ok(account)
        }

        /// Runs a checked action. The call's failure is recorded in the
        /// event; the nonce is spent either way, so that the same action
        /// cannot fill blocks for free.
        fn run(account: T::AccountId, key: DeviceKey, call: <T as Config>::RuntimeCall, nonce: u32) {
            if Self::is_home(&account, &key) && HomeClaim::<T>::take(&account).is_some() {
                Self::deposit_event(Event::HomeRefused {
                    account: account.clone(),
                });
            }
            Acting::<T>::put(key);
            let origin: <T as frame_system::Config>::RuntimeOrigin =
                frame_system::RawOrigin::Signed(account.clone()).into();
            let result = call.dispatch(origin).map(|_| ()).map_err(|e| e.error);
            Acting::<T>::kill();
            Self::deposit_event(Event::Acted {
                account,
                key,
                nonce,
                result,
            });
        }

        /// The account and key of the action running now. Calls that change
        /// keys are made through `act` only.
        fn acting(origin: OriginFor<T>) -> Result<(T::AccountId, DeviceKey), DispatchError> {
            let who = ensure_signed(origin)?;
            let key = Acting::<T>::get().ok_or(Error::<T>::NotByKey)?;
            ensure!(
                KeyOwner::<T>::get(key).as_ref() == Some(&who),
                Error::<T>::NotByKey
            );
            Ok((who, key))
        }

        fn store_policy(relying_parties: Vec<Vec<u8>>, origins: Vec<Vec<u8>>) -> DispatchResult {
            let hashes: Vec<[u8; 32]> = relying_parties
                .iter()
                .map(|id| sp_io::hashing::sha2_256(id))
                .collect();
            let hashes: BoundedVec<[u8; 32], MaxRelyingParties> =
                hashes.try_into().map_err(|_| Error::<T>::PolicyTooLong)?;
            let mut list = BoundedVec::<BoundedVec<u8, MaxOriginLength>, MaxOrigins>::new();
            for origin in origins {
                let origin: BoundedVec<u8, MaxOriginLength> =
                    origin.try_into().map_err(|_| Error::<T>::PolicyTooLong)?;
                list.try_push(origin).map_err(|_| Error::<T>::PolicyTooLong)?;
            }
            RelyingParties::<T>::put(hashes);
            Origins::<T>::put(list);
            Ok(())
        }
    }
}
