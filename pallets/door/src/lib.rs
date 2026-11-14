// Capita Money – pallet-capita-door
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

//! # pallet-capita-door
//!
//! The euro door, on chain: the one way from ordinary money into Capita
//! (diary 3). Design: `docs/adr/0015-euro-door.md`.
//!
//! A door service takes a payment in euro (through GNU Taler) and signs a
//! voucher: this payment bought this much Capita for this account. Anybody
//! brings the voucher to the chain – the door itself, the app of the payer –
//! as a transaction without signer and without fee ([`Pallet::redeem`]):
//! the signature of the door is the authority. The pallet checks it with
//! `capita-door`, mints the amount through the monetary rules ([`Rules`];
//! `pallet-capita`) and remembers the payment, so that each payment mints
//! once.
//!
//! The list of doors (key, most Capita per voucher, active or suspended) is
//! kept by an origin the runtime chooses.
//!
//! Nothing about the payer reaches the chain but the account: no name, no
//! bank, no amount in euro.
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

use capita_door::check::Verify;
use frame_support::pallet_prelude::DispatchResult;

/// The monetary rules, as far as this pallet needs them: `pallet-capita`.
pub trait Rules<AccountId> {
    /// Whether `who` can receive bought money now (an open, active
    /// account).
    fn can_buy(who: &AccountId) -> bool;
    /// Mints `amount` of bought money on `who`.
    fn purchase(who: &AccountId, amount: capita_core::types::Amount) -> DispatchResult;
}

/// The signature check of the node the runtime runs on.
pub struct HostVerify;

impl Verify for HostVerify {
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
    use alloc::vec::Vec;

    use capita_core::types::Amount;
    use capita_door::check::{check_voucher, DoorError};
    use capita_door::types::{Audience, Door, DoorId, DoorSignature, PaymentRef, Voucher};
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;
    use sp_runtime::traits::Zero;

    use super::{HostVerify, Rules};

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// What the runtime supplies. Account identifiers must be 32 bytes, the
    /// form a voucher names its account in.
    #[pallet::config]
    pub trait Config:
        frame_system::Config<
        RuntimeEvent: From<Event<Self>>,
        AccountId: From<[u8; 32]> + Into<[u8; 32]>,
    >
    {
        /// Who maintains the list of doors.
        ///
        /// Offene Frage: E1 – who decides; P04.3 – who may sell Capita.
        type RegistryOrigin: EnsureOrigin<Self::RuntimeOrigin>;
        /// The monetary rules.
        type Rules: Rules<Self::AccountId>;
        /// Priority of a redeeming transaction in the pool.
        #[pallet::constant]
        type UnsignedPriority: Get<TransactionPriority>;
    }

    /// The doors, by number.
    #[pallet::storage]
    pub type Doors<T: Config> = StorageMap<_, Twox64Concat, DoorId, Door>;

    /// The number the next door gets.
    #[pallet::storage]
    pub type NextDoor<T: Config> = StorageValue<_, u32, ValueQuery>;

    /// Every payment redeemed: by which door, for which account.
    #[pallet::storage]
    pub type Redeemed<T: Config> =
        StorageMap<_, Blake2_128Concat, PaymentRef, (DoorId, T::AccountId)>;

    /// The doors the chain trusts from its first block.
    #[pallet::genesis_config]
    #[derive(frame_support::DefaultNoBound)]
    pub struct GenesisConfig<T: Config> {
        /// Numbered from zero in this order.
        pub doors: Vec<Door>,
        #[serde(skip)]
        pub _config: core::marker::PhantomData<T>,
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            for door in &self.doors {
                let id = NextDoor::<T>::get();
                Doors::<T>::insert(DoorId(id), door);
                NextDoor::<T>::put(id.checked_add(1).expect("fewer than 2^32 doors"));
            }
        }
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A payment bought Capita.
        Redeemed {
            /// The door that took it.
            door: DoorId,
            /// The payment.
            payment: PaymentRef,
            /// Who received the Capita.
            account: T::AccountId,
            /// How much.
            amount: Amount,
        },
        /// A door was put on the list.
        DoorRegistered {
            /// Its number.
            id: DoorId,
        },
        /// The entry of a door changed.
        DoorUpdated {
            /// Its number.
            id: DoorId,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// No door with this number is on the list.
        UnknownDoor,
        /// The door is suspended.
        DoorSuspended,
        /// The voucher is for nothing.
        ZeroAmount,
        /// The voucher is for more than the door may sign for.
        AmountTooHigh,
        /// The signature does not fit the voucher, the door or this chain.
        BadSignature,
        /// This payment bought Capita before.
        AlreadyRedeemed,
        /// The account cannot receive money now: not opened, frozen or
        /// closed. The voucher stays good.
        CannotReceive,
        /// More than 2^32 doors.
        TooManyDoors,
    }

    impl<T> From<DoorError> for Error<T> {
        fn from(error: DoorError) -> Self {
            match error {
                DoorError::UnknownDoor => Self::UnknownDoor,
                DoorError::DoorSuspended => Self::DoorSuspended,
                DoorError::ZeroAmount => Self::ZeroAmount,
                DoorError::AmountTooHigh => Self::AmountTooHigh,
                DoorError::BadSignature | DoorError::MalformedTicket => Self::BadSignature,
            }
        }
    }

    /// Placeholder weight until the pallet is benchmarked.
    fn weight_of<T: Config>(base: u64, reads: u64, writes: u64) -> Weight {
        Weight::from_parts(base, 0).saturating_add(T::DbWeight::get().reads_writes(reads, writes))
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Mint the Capita a payment bought, on the strength of the
        /// voucher of a door. A transaction without signer: anybody may
        /// bring it; nobody pays for it. Once per payment.
        #[pallet::call_index(0)]
        #[pallet::weight(weight_of::<T>(100_000_000, 12, 8))]
        pub fn redeem(
            origin: OriginFor<T>,
            voucher: Voucher,
            signature: DoorSignature,
        ) -> DispatchResult {
            ensure_none(origin)?;
            let account = Self::check(&voucher, &signature)?;
            T::Rules::purchase(&account, voucher.amount)?;
            Redeemed::<T>::insert(voucher.payment, (voucher.door, account.clone()));
            Self::deposit_event(Event::Redeemed {
                door: voucher.door,
                payment: voucher.payment,
                account,
                amount: voucher.amount,
            });
            Ok(())
        }

        /// Put a door on the list. Its number is the next free one.
        #[pallet::call_index(10)]
        #[pallet::weight(weight_of::<T>(50_000_000, 1, 2))]
        pub fn register_door(origin: OriginFor<T>, door: Door) -> DispatchResult {
            T::RegistryOrigin::ensure_origin(origin)?;
            let id = NextDoor::<T>::get();
            let next = id.checked_add(1).ok_or(Error::<T>::TooManyDoors)?;
            NextDoor::<T>::put(next);
            Doors::<T>::insert(DoorId(id), door);
            Self::deposit_event(Event::DoorRegistered { id: DoorId(id) });
            Ok(())
        }

        /// Replace the entry of a door: another key, another limit,
        /// suspended or active again. What it minted stays.
        #[pallet::call_index(11)]
        #[pallet::weight(weight_of::<T>(50_000_000, 1, 1))]
        pub fn update_door(origin: OriginFor<T>, id: DoorId, door: Door) -> DispatchResult {
            T::RegistryOrigin::ensure_origin(origin)?;
            ensure!(Doors::<T>::contains_key(id), Error::<T>::UnknownDoor);
            Doors::<T>::insert(id, door);
            Self::deposit_event(Event::DoorUpdated { id });
            Ok(())
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;

        /// A voucher enters the pool if it would mint: right signature,
        /// door active, limit kept, payment not redeemed, account able to
        /// receive. One transaction per payment (`provides`).
        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            let Call::redeem { voucher, signature } = call else {
                return InvalidTransaction::Call.into();
            };
            match Self::check(voucher, signature) {
                Ok(_) => ValidTransaction::with_tag_prefix("CapitaDoor")
                    .priority(T::UnsignedPriority::get())
                    .and_provides(voucher.payment)
                    .longevity(64)
                    .propagate(true)
                    .build(),
                Err(error) if error == Error::<T>::AlreadyRedeemed.into() => {
                    InvalidTransaction::Stale.into()
                }
                Err(error) if error == Error::<T>::CannotReceive.into() => {
                    InvalidTransaction::Custom(1).into()
                }
                Err(_) => InvalidTransaction::BadProof.into(),
            }
        }
    }

    impl<T: Config> Pallet<T> {
        /// What vouchers for this chain are made out to: the hash of its
        /// first block.
        pub fn audience() -> Audience {
            let genesis = frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero());
            let mut out = [0u8; 32];
            for (to, from) in out.iter_mut().zip(genesis.as_ref()) {
                *to = *from;
            }
            Audience(out)
        }

        /// Everything that decides whether a voucher mints, without
        /// changing anything. Answers the account to receive.
        fn check(
            voucher: &Voucher,
            signature: &DoorSignature,
        ) -> Result<T::AccountId, DispatchError> {
            let door = Doors::<T>::get(voucher.door);
            check_voucher::<HostVerify>(&Self::audience(), door.as_ref(), voucher, signature)
                .map_err(Error::<T>::from)?;
            ensure!(
                !Redeemed::<T>::contains_key(voucher.payment),
                Error::<T>::AlreadyRedeemed
            );
            let account = T::AccountId::from(voucher.account.0);
            ensure!(T::Rules::can_buy(&account), Error::<T>::CannotReceive);
            Ok(account)
        }
    }
}
