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

//! All numbers and policies of the currency in one versioned value.
//!
//! Nothing in the core is hard-coded: a rule that is still open is a
//! parameter with a neutral default. Parameters change only through the
//! governance port, as a new version with a start time that is not in the
//! past – never retroactively.

use alloc::vec::Vec;

use crate::attributes::{AttrKey, VerificationLevel};
use crate::errors::CoreError;
use crate::types::{Amount, CountryCode, Duration, ParamsVersion, Rate, Share, TierId, Timestamp};

/// Most pieces a single claim may be split into (changes of rules, expiry
/// of a verification, steps of the seniority ramp). A guard against
/// unbounded work inside a chain runtime; [`Params::validate`] keeps real
/// parameter sets far below it.
pub const MAX_SEGMENTS: u32 = 1_024;

/// Longest demurrage period, in seconds: keeps the carried remainder of the
/// demurrage calculation within 64 bits (period × 1 000 000 < 2^64).
pub const MAX_DEMURRAGE_PERIOD_SECS: u64 = u64::MAX / 1_000_000;

/// One step of the incentive ladder.
///
/// The ladder is an ordered list; steps can be inserted and rates changed
/// by a new parameter version. User interfaces show only the next possible
/// step.
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
pub struct LadderStep {
    /// Stable identifier of the step.
    pub id: TierId,
    /// Verification level required to stand on this step.
    pub min_level: VerificationLevel,
    /// Issuance rate on this step (before caps).
    pub rate: Rate,
}

/// Input for the cap of one country.
///
/// Cap per year = `gni_per_capita` × `pct`. `gni_per_capita` is a published
/// governance input (World Bank Atlas method, yearly), not an exchange rate.
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
pub struct CountryCap {
    /// The country.
    pub country: CountryCode,
    /// Gross national income per person and year, in guide units.
    pub gni_per_capita: Amount,
    /// Share of it that issuance may reach (`cap_pct_by_country`).
    pub pct: Share,
}

/// Upper limits of issuance. The three phases are three parameter versions:
/// phase 1 `by_country` empty; phase 2 `by_country` filled; phase 3
/// additionally `ramp` above zero.
///
/// Offene Frage: A2 target level and A3 final stage – values, not structure.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        scale_info::TypeInfo
    )
)]
pub struct CapParams {
    /// Period the caps refer to (normally [`Duration::MONTH`]).
    pub period: Duration,
    /// Cap for everybody; also the floor below any country cap.
    pub global_cap: Amount,
    /// Caps per country of residence. Empty = global cap only.
    pub by_country: Vec<CountryCap>,
    /// Seniority ramp: account age at which the cap of the country of
    /// residence is fully reached. Zero = no ramp.
    pub ramp: Duration,
}

/// Rules of interest-free lending. There is no interest field on purpose.
///
/// Offene Frage: C1 who lends (peer, pool), C2 fees (default none), C3
/// default handling (default: deduct from future issuance), C4 maximum
/// amount and term. The fields are hooks; none of them is decided here.
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
pub struct LoanParams {
    /// Highest credit line an account may be granted.
    pub max_credit_line: Amount,
    /// Highest recorded debt an account may owe to a lender.
    pub max_outstanding: Amount,
    /// Share of each claim withheld for repayment while debt is outstanding.
    pub repayment_share: Share,
}

/// What happens to the balance of an account that is closed.
///
/// Offene Frage: B5 – death, exit, duplicate. Default: keep (nothing is
/// destroyed; the host decides later).
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
pub enum ClosedBalance {
    /// The balance stays on the ledger.
    #[default]
    Keep,
    /// A positive balance is burnt when the account is closed.
    Burn,
}

/// Demurrage (a holding fee in the style of Gesell and Wörgl).
///
/// Offene Frage: B2 – whether demurrage is used at all. Default: off
/// (`rate` zero).
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
pub struct DemurrageParams {
    /// Share of the balance charged per `period`. Zero = off.
    pub rate: Share,
    /// Period of the charge.
    pub period: Duration,
    /// Part of the balance that is never charged.
    pub free_allowance: Amount,
}

/// One complete, versioned set of rules.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        scale_info::TypeInfo
    )
)]
pub struct Params {
    /// Version, strictly increasing.
    pub version: ParamsVersion,
    /// From when this version applies.
    pub valid_from: Timestamp,
    /// One-time grant for a new account, to be spent on its own
    /// verification.
    pub start_grant: Amount,
    /// The incentive ladder, lowest step first.
    pub ladder: Vec<LadderStep>,
    /// How far back a claim may reach; older entitlement lapses.
    pub claim_window: Duration,
    /// Issuance caps.
    pub caps: CapParams,
    /// Lending rules.
    pub loans: LoanParams,
    /// Demurrage rules.
    pub demurrage: DemurrageParams,
    /// Delay before issued money becomes available. Zero = at once.
    /// Purchased money is always available at once.
    ///
    /// Offene Frage: A1d – ripening period, and whether ripening money may
    /// be lent.
    pub ripening: Duration,
    /// Attribute keys sponsors may target. Public by construction.
    pub sponsor_keys: Vec<AttrKey>,
    /// Treatment of the balance of a closed account.
    pub closed_balance: ClosedBalance,
}

impl Params {
    /// Checks the structural rules every parameter set must obey. It judges
    /// no value: whether a rate or a cap is wise is a matter for governance.
    pub fn validate(&self) -> Result<(), CoreError> {
        let Some(first) = self.ladder.first() else {
            return Err(CoreError::InvalidParams("ladder is empty"));
        };
        let mut level = first.min_level;
        for (i, step) in self.ladder.iter().enumerate() {
            if step.rate.per().is_zero() {
                return Err(CoreError::InvalidParams("ladder rate per zero time"));
            }
            if step.min_level < level {
                return Err(CoreError::InvalidParams(
                    "ladder not ordered by verification level",
                ));
            }
            level = step.min_level;
            if self
                .ladder
                .iter()
                .skip(i.saturating_add(1))
                .any(|s| s.id == step.id)
            {
                return Err(CoreError::InvalidParams("ladder step id used twice"));
            }
        }
        if self.claim_window.is_zero() {
            return Err(CoreError::InvalidParams("claim window is zero"));
        }
        if self.caps.period.is_zero() {
            return Err(CoreError::InvalidParams("cap period is zero"));
        }
        for (i, cap) in self.caps.by_country.iter().enumerate() {
            if !cap.country.0.iter().all(u8::is_ascii_uppercase) {
                return Err(CoreError::InvalidParams("country code not upper-case"));
            }
            if cap.pct > Share::ONE {
                return Err(CoreError::InvalidParams("country share above 100 %"));
            }
            let later = self.caps.by_country.iter().skip(i.saturating_add(1));
            if later.into_iter().any(|c| c.country == cap.country) {
                return Err(CoreError::InvalidParams("country listed twice"));
            }
        }
        if !self.caps.ramp.is_zero() {
            // The ramp rises once per cap period; a claim reaches back one
            // claim window (plus the ripening delay) at most.
            let pieces = self
                .claim_window
                .secs()
                .checked_div(self.caps.period.secs())
                .unwrap_or(u64::MAX);
            if pieces >= u64::from(MAX_SEGMENTS / 2) {
                return Err(CoreError::InvalidParams(
                    "claim window spans too many cap periods",
                ));
            }
        }
        if self.loans.repayment_share > Share::ONE {
            return Err(CoreError::InvalidParams("repayment share above 100 %"));
        }
        if self.demurrage.rate > Share::ONE {
            return Err(CoreError::InvalidParams("demurrage rate above 100 %"));
        }
        if !self.demurrage.rate.is_zero() {
            if self.demurrage.period.is_zero() {
                return Err(CoreError::InvalidParams("demurrage period is zero"));
            }
            if self.demurrage.period.secs() > MAX_DEMURRAGE_PERIOD_SECS {
                return Err(CoreError::InvalidParams("demurrage period too long"));
            }
        }
        for (i, key) in self.sponsor_keys.iter().enumerate() {
            if self
                .sponsor_keys
                .iter()
                .skip(i.saturating_add(1))
                .any(|k| k == key)
            {
                return Err(CoreError::InvalidParams("sponsor key listed twice"));
            }
        }
        Ok(())
    }

    /// Checks that `self` may follow `prev` when enacted at `now`: valid in
    /// itself, a higher version, and a start time neither in the past nor
    /// before the start of `prev`. Rules never change retroactively.
    pub fn validate_successor(&self, prev: &Self, now: Timestamp) -> Result<(), CoreError> {
        self.validate()?;
        if self.version <= prev.version {
            return Err(CoreError::InvalidParams("version not higher"));
        }
        if self.valid_from < now {
            return Err(CoreError::InvalidParams("start time in the past"));
        }
        if self.valid_from <= prev.valid_from {
            return Err(CoreError::InvalidParams(
                "start time not after the previous version",
            ));
        }
        Ok(())
    }
}

/// Checks a list of parameter versions as returned by
/// `ParamsStore::in_force`: not empty, oldest first, versions and start
/// times strictly increasing.
pub fn check_sequence(list: &[Params]) -> Result<(), CoreError> {
    if list.is_empty() {
        return Err(CoreError::NoParamsInForce);
    }
    for pair in list.windows(2) {
        if let [a, b] = pair {
            if b.version <= a.version || b.valid_from <= a.valid_from {
                return Err(CoreError::InvalidParams("versions not in order"));
            }
        }
    }
    Ok(())
}

/// The version in force at `t`: the latest one that has started.
pub fn in_force_at(list: &[Params], t: Timestamp) -> Result<&Params, CoreError> {
    list.iter()
        .rev()
        .find(|p| p.valid_from <= t)
        .ok_or(CoreError::NoParamsInForce)
}

/// Start of the first version that begins after `t`, if any.
pub(crate) fn next_change(list: &[Params], t: Timestamp) -> Option<Timestamp> {
    list.iter().map(|p| p.valid_from).find(|start| *start > t)
}
