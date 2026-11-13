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
// Adapted from `frame_system::CheckNonce` of the Polkadot SDK
// (Copyright (C) Parity Technologies (UK) Ltd., Apache-2.0).

//! Replay protection without payment.
//!
//! The nonce check of the SDK refuses a transaction from an account nobody has
//! paid storage for – on chains with fees, an account without funds. This
//! chain has no fees and no deposit, so a new key could never send its first
//! transaction, the one that opens its Capita account. This check keeps the
//! replay protection and drops that condition.
//!
//! Offene Frage: protection against spam (fees in Capita or a quota per
//! identity). Until then any key may write a nonce to the state.

use alloc::{vec, vec::Vec};
use codec::{Decode, DecodeWithMemTracking, Encode};
use frame_support::{dispatch::DispatchInfo, pallet_prelude::TransactionSource};
use frame_system::Config;
use scale_info::TypeInfo;
use sp_runtime::{
    traits::{
        AsSystemOriginSigner, DispatchInfoOf, DispatchOriginOf, Dispatchable, One,
        TransactionExtension, ValidateResult,
    },
    transaction_validity::{
        InvalidTransaction, TransactionLongevity, TransactionValidityError, ValidTransaction,
    },
    Saturating, Weight,
};

/// Nonce check and increment: every transaction of an account carries the
/// number of its predecessors and runs once.
///
/// Name and encoding equal those of the SDK, so wallets need no change.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Eq, PartialEq, TypeInfo)]
#[scale_info(skip_type_params(T))]
pub struct CheckNonce<T: Config>(#[codec(compact)] pub T::Nonce);

impl<T: Config> core::fmt::Debug for CheckNonce<T> {
    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "CheckNonce({})", self.0)
    }

    #[cfg(not(feature = "std"))]
    fn fmt(&self, _: &mut core::fmt::Formatter) -> core::fmt::Result {
        Ok(())
    }
}

impl<T: Config> TransactionExtension<T::RuntimeCall> for CheckNonce<T>
where
    T::RuntimeCall: Dispatchable<Info = DispatchInfo>,
    <T::RuntimeCall as Dispatchable>::RuntimeOrigin: AsSystemOriginSigner<T::AccountId> + Clone,
{
    const IDENTIFIER: &'static str = "CheckNonce";
    type Implicit = ();
    /// The sender, if the transaction has one.
    type Val = Option<T::AccountId>;
    type Pre = ();

    fn weight(&self, _: &T::RuntimeCall) -> Weight {
        <T::DbWeight as frame_support::traits::Get<_>>::get().reads_writes(1, 1)
    }

    fn validate(
        &self,
        origin: DispatchOriginOf<T::RuntimeCall>,
        _call: &T::RuntimeCall,
        _info: &DispatchInfoOf<T::RuntimeCall>,
        _len: usize,
        _self_implicit: Self::Implicit,
        _inherited_implication: &impl Encode,
        _source: TransactionSource,
    ) -> ValidateResult<Self::Val, T::RuntimeCall> {
        let Some(who) = origin.as_system_origin_signer() else {
            return Ok((ValidTransaction::default(), None, origin));
        };
        let current = frame_system::Pallet::<T>::account_nonce(who);
        if self.0 < current {
            return Err(InvalidTransaction::Stale.into());
        }
        let provides: Vec<Vec<u8>> = vec![(who.clone(), self.0).encode()];
        let requires: Vec<Vec<u8>> = if current < self.0 {
            vec![(who.clone(), self.0.saturating_sub(One::one())).encode()]
        } else {
            vec![]
        };
        let validity = ValidTransaction {
            priority: 0,
            requires,
            provides,
            longevity: TransactionLongevity::MAX,
            propagate: true,
        };
        let who = who.clone();
        Ok((validity, Some(who), origin))
    }

    fn prepare(
        self,
        val: Self::Val,
        _origin: &DispatchOriginOf<T::RuntimeCall>,
        _call: &T::RuntimeCall,
        _info: &DispatchInfoOf<T::RuntimeCall>,
        _len: usize,
    ) -> Result<Self::Pre, TransactionValidityError> {
        let Some(who) = val else { return Ok(()) };
        if self.0 > frame_system::Pallet::<T>::account_nonce(&who) {
            return Err(InvalidTransaction::Future.into());
        }
        frame_system::Pallet::<T>::inc_account_nonce(&who);
        Ok(())
    }
}
