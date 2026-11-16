// Capita Money – pallet-capita-quota
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

//! # pallet-capita-quota
//!
//! What stands at the door of the chain instead of a fee (rules diary 32,
//! `docs/adr/0029-free-quota-instead-of-fees.md`). The counting is
//! `capita_core::quota`; this pallet keeps the counters and answers the two
//! questions every way into the chain asks:
//!
//! - [`Pallet::check`]: may this sender send this call now – asked when a
//!   transaction enters the pool, and again right before it runs.
//! - [`Pallet::spend`]: count it – when it runs.
//!
//! Who asks: the nonce check of signed transactions and the actions of
//! device keys (`pallet-capita-keys`), both wired in the runtime. The
//! pallet has no calls and no events: a counter per account, and the number
//! of transactions of keys without a person in the present block.
//!
//! - A **verified person** sends [`Config::PersonPerDay`] transactions per
//!   day (UTC), whatever they are. Whoever sent less today comes first.
//! - A **key without a person** sends only the calls the runtime names
//!   ([`Senders::open_to_new_keys`]: open an account, present its claim),
//!   [`Config::NewKeyPerDay`] per day, and a block takes
//!   [`Config::NewKeysPerBlock`] of them. Keys cost nothing to make: the
//!   number per block is what keeps them from filling the ledger.
//! - A **service** key (the authority) is not counted.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

pub use pallet::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

use capita_core::quota::Sender;

/// What the runtime knows about senders and calls.
pub trait Senders<AccountId, Call> {
    /// Who stands behind the account `who`.
    fn sender(who: &AccountId) -> Sender;
    /// Whether a key without a person may send `call`: open an account,
    /// present its claim (rules diary 32).
    fn open_to_new_keys(call: &Call) -> bool;
}

/// Why a transaction is not let in, as the number of
/// `InvalidTransaction::Custom`: the quota of the day is used up.
pub const USED_UP: u8 = 32;
/// A key without a person sent something else than opening an account or
/// presenting its claim.
pub const NOT_FOR_NEW_KEYS: u8 = 33;

/// Place in the pool of the transactions of a service.
pub const SERVICE_PRIORITY: u64 = 1 << 40;
/// Place in the pool of the transactions of a person: above every key
/// without a person, and by what is left of the day.
pub const PERSON_PRIORITY: u64 = 1 << 32;

#[frame_support::pallet]
pub mod pallet {
    use capita_core::quota::{day_of, Limits, Sender, Usage};
    use capita_core::types::Timestamp;
    use frame_support::pallet_prelude::*;
    use frame_support::traits::UnixTime;
    use frame_system::pallet_prelude::*;
    use sp_runtime::transaction_validity::{InvalidTransaction, TransactionPriority};

    use super::{Senders, NOT_FOR_NEW_KEYS, PERSON_PRIORITY, SERVICE_PRIORITY, USED_UP};

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// What the runtime supplies.
    #[pallet::config]
    pub trait Config: frame_system::Config {
        /// The time of the chain: a day is a day of UTC.
        type Time: UnixTime;
        /// Who sends, and what a key without a person may send.
        type Senders: Senders<Self::AccountId, <Self as frame_system::Config>::RuntimeCall>;
        /// Transactions a verified person may send per day (rule
        /// `spam.person_per_day`).
        type PersonPerDay: Get<u32>;
        /// Transactions a key without a person may send per day (rule
        /// `spam.new_key_per_day`).
        type NewKeyPerDay: Get<u32>;
        /// Transactions of keys without a person one block takes (rule
        /// `spam.new_keys_per_block`).
        type NewKeysPerBlock: Get<u32>;
    }

    /// What an account sent on the last day it sent anything.
    #[pallet::storage]
    pub type Used<T: Config> = StorageMap<_, Blake2_128Concat, T::AccountId, Usage, ValueQuery>;

    /// Transactions of keys without a person in the block of that number.
    #[pallet::storage]
    pub type NewKeysInBlock<T: Config> = StorageValue<_, (BlockNumberFor<T>, u32), ValueQuery>;

    /// A transaction that may run: what `check` found.
    struct Admitted {
        sender: Sender,
        /// The day and the number per day; `None`: not counted.
        counted: Option<(u32, u32)>,
        usage: Usage,
    }

    impl<T: Config> Pallet<T> {
        /// The numbers in force.
        pub fn limits() -> Limits {
            Limits {
                person_per_day: T::PersonPerDay::get(),
                new_key_per_day: T::NewKeyPerDay::get(),
                new_keys_per_block: T::NewKeysPerBlock::get(),
            }
        }

        /// The day of the chain's clock.
        pub fn today() -> u32 {
            day_of(Timestamp::from_secs(T::Time::now().as_secs()))
        }

        /// How many transactions `who` may still send today; `None`: not
        /// counted. For the screens.
        pub fn left(who: &T::AccountId) -> Option<u32> {
            let limit = Self::limits().per_day(T::Senders::sender(who))?;
            Some(Used::<T>::get(who).left(Self::today(), limit))
        }

        /// Transactions of keys without a person the present block holds.
        pub fn new_keys_in_block() -> u32 {
            let (block, count) = NewKeysInBlock::<T>::get();
            if block == frame_system::Pallet::<T>::block_number() {
                count
            } else {
                0
            }
        }

        /// Whether `who` may send `call` now, and its place in the pool.
        ///
        /// `in_block`: right before the transaction runs – only then the
        /// number of keys without a person per block is known. A
        /// transaction over that number is not wrong, the block is full
        /// for it (`ExhaustsResources`): it waits for the next one.
        pub fn check(
            who: &T::AccountId,
            call: &<T as frame_system::Config>::RuntimeCall,
            in_block: bool,
        ) -> Result<TransactionPriority, InvalidTransaction> {
            let admitted = Self::admit(who, call, in_block)?;
            Ok(match (admitted.sender, admitted.counted) {
                (Sender::Service, _) | (_, None) => SERVICE_PRIORITY,
                (Sender::Person, Some((today, limit))) => PERSON_PRIORITY
                    .saturating_add(TransactionPriority::from(admitted.usage.left(today, limit))),
                (Sender::NewKey, Some((today, limit))) => {
                    TransactionPriority::from(admitted.usage.left(today, limit))
                }
            })
        }

        /// Counts a transaction of `who` that runs now. Checks again: the
        /// state may have changed since the transaction entered the pool.
        pub fn spend(
            who: &T::AccountId,
            call: &<T as frame_system::Config>::RuntimeCall,
        ) -> Result<(), InvalidTransaction> {
            let admitted = Self::admit(who, call, true)?;
            let Some((today, limit)) = admitted.counted else {
                return Ok(());
            };
            let usage = admitted
                .usage
                .spend(today, limit)
                .ok_or(InvalidTransaction::Custom(USED_UP))?;
            Used::<T>::insert(who, usage);
            if admitted.sender == Sender::NewKey {
                let count = Self::new_keys_in_block().saturating_add(1);
                NewKeysInBlock::<T>::put((frame_system::Pallet::<T>::block_number(), count));
            }
            Ok(())
        }

        fn admit(
            who: &T::AccountId,
            call: &<T as frame_system::Config>::RuntimeCall,
            in_block: bool,
        ) -> Result<Admitted, InvalidTransaction> {
            let sender = T::Senders::sender(who);
            let limits = Self::limits();
            let Some(limit) = limits.per_day(sender) else {
                return Ok(Admitted {
                    sender,
                    counted: None,
                    usage: Usage::default(),
                });
            };
            if sender == Sender::NewKey {
                if !T::Senders::open_to_new_keys(call) {
                    return Err(InvalidTransaction::Custom(NOT_FOR_NEW_KEYS));
                }
                if in_block && Self::new_keys_in_block() >= limits.new_keys_per_block {
                    return Err(InvalidTransaction::ExhaustsResources);
                }
            }
            let today = Self::today();
            let usage = Used::<T>::get(who);
            if usage.left(today, limit) == 0 {
                return Err(InvalidTransaction::Custom(USED_UP));
            }
            Ok(Admitted {
                sender,
                counted: Some((today, limit)),
                usage,
            })
        }
    }
}
