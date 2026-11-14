// Capita Money – capita-core
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

//! Ports: the only doors between the rules and the outside world.
//!
//! The core computes; adapters behind these traits store, move money and
//! verify people. Each port has at least two planned adapters, which keeps
//! every outside system replaceable:
//!
//! | Port            | Adapter A (first)                 | Adapter B (later)            |
//! |-----------------|-----------------------------------|------------------------------|
//! | [`Ledger`]      | GNU Taler libeufin Core Bank API  | Substrate pallet             |
//! | [`Identity`]    | Taler Challenger (e-mail, SMS, post) | DID/VC (`ssi`), Idena-style |
//! | [`ParamsStore`], [`Governance`] | signed config file     | on-chain governance          |
//! | [`AccountStore`]| PostgreSQL                        | chain storage                |
//! | [`EventSink`]   | append-only table, public totals  | chain events                 |
//!
//! Later ports, named only: **messaging** and **content distribution**
//! (libp2p, iroh).
//!
//! There is deliberately no clock port: the host passes the time into every
//! call (server clock or block time). A device never supplies time.
//!
//! All traits are synchronous. The rules themselves never call a port: they
//! are pure planning functions (`plan_*`) that return a [`crate::plan::Plan`].
//! The host loads the inputs through the ports, and stores and carries out
//! the plan through them – with synchronous or asynchronous I/O.

use alloc::vec::Vec;

use crate::account::{AccountState, AccountStatus};
use crate::attributes::Attestation;
use crate::params::Params;
use crate::types::{AccountId, Amount, Balance, OpId, ParamsVersion, PersonId, TierId, Timestamp};

/// How new money comes into being.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub enum MintKind {
    /// Per-capita issuance (the basic income).
    Issued,
    /// Bought with ordinary money. The payment itself happens outside the
    /// core; there is no way back from Capita to ordinary money.
    Purchased,
}

/// Why money leaves circulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub enum BurnReason {
    /// Demurrage charge.
    Demurrage,
    /// Balance of a closed account.
    AccountClosed,
}

/// Why money moves between two accounts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub enum TransferPurpose {
    /// Payment for goods or services, or a gift.
    Payment,
    /// A lender hands out a loan.
    LoanDisbursement,
    /// A borrower repays.
    LoanRepayment,
    /// The whole balance of an account that moved to a new key
    /// (`recovery`). The same person on both sides: not counted as a
    /// payment.
    AccountMoved,
}

/// One instruction to the ledger. The core produces these; it never touches
/// a balance directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub enum LedgerOp {
    /// Create money on an account (from the `centralbank` account).
    Mint {
        /// Idempotency key.
        id: OpId,
        /// Receiving account.
        to: AccountId,
        /// How much.
        amount: Amount,
        /// Issued or purchased.
        kind: MintKind,
    },
    /// Remove money from an account (back to `centralbank`).
    Burn {
        /// Idempotency key.
        id: OpId,
        /// Paying account.
        from: AccountId,
        /// How much.
        amount: Amount,
        /// Why.
        reason: BurnReason,
    },
    /// Move money between two member accounts.
    Transfer {
        /// Idempotency key.
        id: OpId,
        /// Paying account.
        from: AccountId,
        /// Receiving account.
        to: AccountId,
        /// How much.
        amount: Amount,
        /// Why.
        purpose: TransferPurpose,
    },
    /// Set how far an account may go below zero.
    SetCreditLine {
        /// Idempotency key.
        id: OpId,
        /// The account.
        account: AccountId,
        /// New limit.
        limit: Amount,
    },
}

/// Result of applying a [`LedgerOp`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Applied {
    /// Applied now.
    Done,
    /// An operation with the same [`OpId`] was applied before; nothing
    /// changed. Makes retries safe.
    AlreadyApplied,
}

/// The book of balances.
///
/// Contract for adapters:
/// - apply each [`OpId`] at most once (replay protection);
/// - refuse a debit that would take a balance below minus its credit line;
/// - mint only from, and burn only to, the dedicated `centralbank` account –
///   never an administrator account.
pub trait Ledger {
    /// Adapter error (network, database, insufficient funds …).
    type Error;

    /// Current balance of an account.
    fn balance(&self, account: &AccountId) -> Result<Balance, Self::Error>;

    /// Apply one operation, idempotently.
    fn apply(&mut self, op: &LedgerOp) -> Result<Applied, Self::Error>;

    /// Money in circulation: everything minted minus everything burnt.
    fn total_supply(&self) -> Result<u128, Self::Error>;
}

/// Who is behind an account, as far as the rules need to know.
///
/// Contract for adapters: return derived facts only (see [`Attestation`]);
/// keep documents and personal data outside the core and outside public
/// records. Providers are interchangeable.
///
/// Offene Frage: F3 identity provider; B6 minimum age (the adapter decides
/// eligibility and reports a level, the core never sees an age).
pub trait Identity {
    /// Adapter error.
    type Error;

    /// The attestation valid at `now`, or `None` if the holder is unverified
    /// or the attestation has expired or been revoked.
    fn attestation(
        &self,
        account: &AccountId,
        now: Timestamp,
    ) -> Result<Option<Attestation>, Self::Error>;
}

/// Result of saving an account state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Saved {
    /// Stored.
    Done,
    /// Somebody else changed the account in the meantime; nothing stored.
    /// The caller starts again from a fresh load. Prevents double claims
    /// from parallel requests.
    Conflict,
}

/// Storage of [`AccountState`]. Not a ledger: holds no balances.
///
/// Revisions: an account that does not exist yet counts as revision 0; the
/// state of a newly opened account carries revision 1. Every plan returns a
/// state whose revision is one higher than the one it started from.
pub trait AccountStore {
    /// Adapter error.
    type Error;

    /// State of one account, if it exists.
    fn load(&self, id: &AccountId) -> Result<Option<AccountState>, Self::Error>;

    /// The account already held by a person, if any (one person, one
    /// account).
    fn account_of(&self, person: &PersonId) -> Result<Option<AccountId>, Self::Error>;

    /// Store `state` if the stored revision still equals
    /// `expected_revision` (compare-and-swap); `state.revision` must be
    /// `expected_revision + 1`.
    fn save(&mut self, expected_revision: u64, state: &AccountState) -> Result<Saved, Self::Error>;
}

/// Read access to the versioned rules.
pub trait ParamsStore {
    /// Adapter error.
    type Error;

    /// Every parameter version in force at some moment between `from` and
    /// `to` (inclusive), oldest first. A claim that spans a change of rules
    /// is split at the change.
    fn in_force(&self, from: Timestamp, to: Timestamp) -> Result<Vec<Params>, Self::Error>;
}

/// The only way to change the rules.
///
/// Contract for adapters: accept a new version only with a valid
/// authorization, a higher version number and a start time not before
/// `now`.
///
/// Offene Frage: E1 – who may change the rules. The answer lives entirely in
/// the adapter (`Authorization`); the core does not decide it.
pub trait Governance {
    /// Adapter error (including "not authorized").
    type Error;
    /// Proof that the change is legitimate (signatures, a vote result …).
    type Authorization;

    /// Put a new parameter version into force from `next.valid_from`.
    fn enact(
        &mut self,
        next: Params,
        authorization: &Self::Authorization,
        now: Timestamp,
    ) -> Result<ParamsVersion, Self::Error>;
}

/// A fact that happened, for the public record. Contains no personal data.
/// Totals (money issued, purchased, burnt, lent) are sums over events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub enum Event {
    /// An account was opened.
    AccountOpened {
        /// The account.
        account: AccountId,
        /// When.
        at: Timestamp,
    },
    /// The one-time start grant was issued to a new account.
    StartGranted {
        /// The account.
        account: AccountId,
        /// How much.
        amount: Amount,
        /// When.
        at: Timestamp,
    },
    /// An account changed its life-cycle state.
    StatusChanged {
        /// The account.
        account: AccountId,
        /// Old state.
        from: AccountStatus,
        /// New state.
        to: AccountStatus,
        /// When.
        at: Timestamp,
    },
    /// An account moved on the incentive ladder.
    TierChanged {
        /// The account.
        account: AccountId,
        /// Old step.
        from: TierId,
        /// New step.
        to: TierId,
        /// When.
        at: Timestamp,
    },
    /// Per-capita money was claimed.
    Claimed {
        /// The account.
        account: AccountId,
        /// Amount issued (before repayment).
        issued: Amount,
        /// Part of it passed on to the lender to repay a loan. Not reported
        /// a second time as [`Event::Transferred`].
        repaid: Amount,
        /// Start of the period covered.
        from: Timestamp,
        /// End of the period covered.
        to: Timestamp,
    },
    /// Money was bought with ordinary money.
    Purchased {
        /// The receiving account.
        account: AccountId,
        /// How much.
        amount: Amount,
        /// When.
        at: Timestamp,
    },
    /// Money moved between two accounts.
    Transferred {
        /// Paying account.
        from: AccountId,
        /// Receiving account.
        to: AccountId,
        /// How much.
        amount: Amount,
        /// Why.
        purpose: TransferPurpose,
        /// When.
        at: Timestamp,
    },
    /// An account moved to a new key: the person, the balance and what
    /// was accrued went from one account to the other; the old one is
    /// closed.
    AccountMoved {
        /// The account left behind, closed.
        from: AccountId,
        /// The account of the person from now on.
        to: AccountId,
        /// When.
        at: Timestamp,
    },
    /// A credit line was granted or changed.
    CreditLineChanged {
        /// The account.
        account: AccountId,
        /// Old limit.
        from: Amount,
        /// New limit.
        to: Amount,
        /// When.
        at: Timestamp,
    },
    /// Money left circulation.
    Burnt {
        /// The paying account.
        account: AccountId,
        /// How much.
        amount: Amount,
        /// Why.
        reason: BurnReason,
        /// When.
        at: Timestamp,
    },
    /// A new parameter version was put into force.
    ParamsEnacted {
        /// Its version.
        version: ParamsVersion,
        /// From when it applies.
        valid_from: Timestamp,
    },
}

/// Decides whether a step of the ladder is unlocked.
///
/// Offene Frage: B3/B3a – supply versus market: higher steps could open only
/// once the market offers enough to buy. The decision and its data (market
/// size) live in the adapter. Default: [`NoGate`].
pub trait TierGate {
    /// `true` if accounts may move to `tier` at `now`.
    fn unlocked(&self, tier: TierId, now: Timestamp) -> bool;
}

/// The default gate: every step is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoGate;

impl TierGate for NoGate {
    fn unlocked(&self, _tier: TierId, _now: Timestamp) -> bool {
        true
    }
}

/// The public record of what happened.
pub trait EventSink {
    /// Adapter error. A failed record aborts the operation: nothing happens
    /// off the record.
    type Error;

    /// Append one event.
    fn emit(&mut self, event: &Event) -> Result<(), Self::Error>;
}
