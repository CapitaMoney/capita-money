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

//! Per-account state kept by the core rules.
//!
//! The ledger holds balances; this state holds everything the rules need in
//! addition. Of a person it holds two facts: the person key and, for issuance
//! by age, the date of birth (ADR 0026).

use crate::attributes::{Residence, VerificationLevel};
use crate::types::{AccountId, Amount, Date, PersonId, Rate, TierId, Timestamp};

/// Life cycle of an account.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
pub enum AccountStatus {
    /// Normal operation.
    #[default]
    Active,
    /// Suspended by the authority (for example a suspected duplicate):
    /// receives nothing, and does not move to a new key.
    Frozen,
    /// Ended (exit or death): receives nothing, never reopened.
    Closed,
    /// Locked by its own holder (a stolen key, the own identity revoked):
    /// nobody can spend or claim. What the person is entitled to keeps
    /// accruing; the account can still move to a new key after a new
    /// verification (rules diary 15).
    HolderFrozen,
    /// The old key stopped the move to a new key: locked for both sides
    /// until a second check decides (rules diary 15). What the person is
    /// entitled to keeps accruing for whoever the decision names.
    Disputed,
}

impl AccountStatus {
    /// Whether what the holder is entitled to keeps accruing: in normal
    /// operation and while the account is only locked. A lock is not a
    /// sanction – behind it there is still one verified person.
    #[must_use]
    pub const fn accrues(self) -> bool {
        matches!(self, Self::Active | Self::HolderFrozen | Self::Disputed)
    }

    /// Whether the account may move to a new key of the same person: not
    /// one the authority froze – a freeze is not escaped by a new key – and
    /// not a closed one.
    #[must_use]
    pub const fn may_move(self) -> bool {
        self.accrues()
    }

    /// Locked: open, but nobody can spend or claim until a new verification
    /// or a second check settles to whom the account belongs.
    #[must_use]
    pub const fn locked(self) -> bool {
        matches!(self, Self::HolderFrozen | Self::Disputed)
    }
}

/// Bookkeeping of lazy issuance (ADR 0002).
///
/// Within one segment the rate is constant. Entitlement at time `t` =
/// `rate` × (`t` − `anchor`), rounded down, minus `paid_since_anchor`.
/// Sub-cent remainders are thus never lost and never rounded up. When the
/// rate changes, the segment is closed and a new one starts.
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
pub struct IssuanceState {
    /// Start of the current segment.
    pub anchor: Timestamp,
    /// Effective rate of the current segment (step rate limited by the cap).
    pub rate: Rate,
    /// Amount already accounted for the time from `anchor` to `paid_until`.
    pub paid_since_anchor: Amount,
    /// Up to when entitlement has been accounted (minted or carried).
    pub paid_until: Timestamp,
    /// Time of the last claim that minted money.
    pub last_claim: Timestamp,
    /// Entitlement accounted but not yet minted: set aside when a step,
    /// level, residence or status changed, minted by the next claim after
    /// `carried_ripe_at`.
    pub carried: Amount,
    /// From when `carried` may be minted.
    pub carried_ripe_at: Timestamp,
}

/// Starting point of the seniority ramp of the cap (phase 3).
///
/// The cap rises from `from` towards the cap of the country of residence,
/// beginning at `since`. A new account starts at the global cap; a move to a
/// richer country restarts the ramp for the difference.
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
pub struct CapRamp {
    /// Cap level the ramp starts from (amount per period, kept as a rate so
    /// that a later change of the cap period does not distort it).
    pub from: Rate,
    /// When the ramp started.
    pub since: Timestamp,
}

/// Interest-free credit of one account (ADR 0006).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
pub struct LoanState {
    /// How far the balance may go below zero.
    pub credit_line: Amount,
    /// Recorded debt to `lender`, still to be repaid.
    pub outstanding: Amount,
    /// Who is owed `outstanding` (a person or a pool account). `None`
    /// exactly when nothing is outstanding.
    pub lender: Option<AccountId>,
}

/// Everything the rules remember about one account.
#[derive(Clone, Debug, PartialEq, Eq)]
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
pub struct AccountState {
    /// The account.
    pub id: AccountId,
    /// The unique person behind it, once verified. One person, one account.
    pub person: Option<PersonId>,
    /// Life-cycle state.
    pub status: AccountStatus,
    /// When the account was opened (basis of the seniority ramp).
    pub opened_at: Timestamp,
    /// Current step on the incentive ladder.
    pub tier: TierId,
    /// Since when the account has been on `tier`.
    pub tier_since: Timestamp,
    /// Verification level last attested (0 = unverified).
    pub level: VerificationLevel,
    /// End of validity of `level`; from then on the account counts as
    /// unverified until a new attestation arrives. `None` = until revoked.
    pub level_until: Option<Timestamp>,
    /// Proven residence, if any (source of the country cap).
    pub residence: Option<Residence>,
    /// Date of birth of the holder, as an identity service stated it:
    /// the basis of issuance by age (ADR 0026). `None` until stated.
    pub born: Option<Date>,
    /// Whether the one-time amount at the first verification was accounted.
    /// Once per person: it follows the person to a new key.
    pub first_payout_made: bool,
    /// Seniority ramp of the cap.
    pub cap_ramp: CapRamp,
    /// Start grant received and not yet spent. Earmarked: it can be paid
    /// only for the verification of the account itself, and no demurrage is
    /// charged on it. Zero once spent.
    pub start_grant_reserved: Amount,
    /// Lazy-issuance bookkeeping.
    pub issuance: IssuanceState,
    /// Credit line and outstanding debt.
    pub loan: LoanState,
    /// Up to when demurrage has been settled.
    pub demurrage_settled_at: Timestamp,
    /// Sub-cent remainder of the demurrage calculation, carried so that
    /// frequent settling does not round the charge away (ADR 0007).
    pub demurrage_carry: u64,
    /// Latest host time seen for this account; time may never go backwards.
    pub last_seen: Timestamp,
    /// Sequence number of the next ledger operation (`OpId::seq`).
    pub next_op_seq: u64,
    /// Incremented on every save; stores reject a stale revision.
    pub revision: u64,
}
