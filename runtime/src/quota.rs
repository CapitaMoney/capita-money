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

//! No fees: who may send what, free (rules diary 32,
//! `docs/adr/0029-free-quota-instead-of-fees.md`).
//!
//! Wiring only. `pallet-capita-quota` counts; this file tells it who a
//! sender is and what a key without a person may send, and puts it at the
//! two ways into the chain: the nonce check of signed transactions
//! (`crate::nonce`) and the actions of device keys (`pallet-capita-keys`,
//! [`KeysGate`]). The third way, the voucher of a euro door, needs no
//! quota: each payment in euro redeems once.

use capita_core::quota::Sender;
use sp_runtime::transaction_validity::{InvalidTransaction, TransactionPriority};

use crate::{AccountId, Capita, Quota, Runtime, RuntimeCall};

/// Who stands behind a key, and what a key without a person may send.
pub struct WhoSends;

impl pallet_capita_quota::Senders<AccountId, RuntimeCall> for WhoSends {
    /// - The key of the authority (sudo on the test chains) is the one
    ///   service: it keeps the lists and the rules, and is not counted.
    /// - An account bound to a person – verified once by a document check –
    ///   is that person, also while its claim has lapsed or it is locked:
    ///   one person, one quota.
    /// - Everything else is a key nobody stands behind.
    fn sender(who: &AccountId) -> Sender {
        if pallet_sudo::Key::<Runtime>::get().as_ref() == Some(who) {
            Sender::Service
        } else if Capita::has_person(&Capita::id(who.clone())) {
            Sender::Person
        } else {
            Sender::NewKey
        }
    }

    /// "Open an account and present its claim": open the account, pay the
    /// own verification from the start grant (rules diary 5), present the
    /// claim, and – a lost key – ask for the move of the account, withdraw
    /// that, complete it after the waiting time. Nothing else: no payment,
    /// no second device, no remark.
    fn open_to_new_keys(call: &RuntimeCall) -> bool {
        matches!(
            call,
            RuntimeCall::Capita(
                pallet_capita::Call::open { .. } | pallet_capita::Call::pay_verification { .. }
            ) | RuntimeCall::Credential(
                pallet_capita_credential::Call::present { .. }
                    | pallet_capita_credential::Call::request_move { .. }
                    | pallet_capita_credential::Call::stop_move { .. }
                    | pallet_capita_credential::Call::complete_move { .. }
            )
        )
    }
}

/// The quota at the actions of device keys.
pub struct KeysGate;

impl pallet_capita_keys::Gate<AccountId, RuntimeCall> for KeysGate {
    fn check(
        who: &AccountId,
        call: &RuntimeCall,
        in_block: bool,
    ) -> Result<TransactionPriority, InvalidTransaction> {
        Quota::check(who, call, in_block)
    }

    fn spend(who: &AccountId, call: &RuntimeCall) -> Result<(), InvalidTransaction> {
        Quota::spend(who, call)
    }
}
