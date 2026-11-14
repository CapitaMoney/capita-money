// Capita Money – capita-rulefile
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

//! # capita-rulefile
//!
//! Reads a rules file of [Capita Money](https://capitamoney.com) – one file
//! that holds every rule value, written for people (`rules/alpha.toml`) –
//! and checks it (`docs/adr/0012-rules-file.md`).
//!
//! Two users:
//! - the build of `capita-rules` turns the files of `rules/` into constants
//!   and parameters every host of this repository is built with
//!   ([`codegen`]);
//! - a service host or a tool reads a file while it runs
//!   ([`RuleSet::from_toml`], [`RuleSet::params`]).
//!
//! A rules file holds two kinds of entries: values the software reads
//! (money, time spans, shares, lists), and a register of rules that are
//! behaviour and live in code, with the place where. Every entry carries its
//! [`Status`]: decided by the owner, a working default, or an example.

#![forbid(unsafe_code)]

pub mod codegen;
pub mod units;

use core::fmt;

use capita_core::attributes::{AttrKey, VerificationLevel};
use capita_core::params::{
    CapParams, ClosedBalance, CountryCap, DemurrageParams, LadderStep, LoanParams, Params,
};
use capita_core::types::{CountryCode, ParamsVersion, Rate, TierId, Timestamp};
use serde::Deserialize;

use crate::units::{Money, Percent, Span};

/// What is wrong with a rules file, in words for the person who edits it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl Error {
    fn at(rule: &str, problem: impl fmt::Display) -> Self {
        Self(format!("{rule}: {problem}"))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::error::Error for Error {}

/// How firm a rule is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Decided by the owner.
    Decided,
    /// A working assumption until the owner decides.
    Default,
    /// A placeholder number with no decision behind it.
    Example,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Decided => "decided",
            Self::Default => "default",
            Self::Example => "example",
        })
    }
}

/// One rule with a value the software reads.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule<T> {
    /// The rule itself.
    pub value: T,
    /// How firm it is.
    pub status: Status,
    /// Where it comes from: rule of the diary, question of the interview,
    /// or a design decision of this repository.
    pub source: String,
    /// What the value means.
    pub note: String,
}

/// Name and version of a rule set.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetInfo {
    /// Short name; equals the name of the file without `.toml`.
    pub name: String,
    /// Name for people.
    pub title: String,
    /// The version of the owner's rules diary the file mirrors.
    pub diary: String,
    /// Version of the monetary rules, as a host counts them.
    pub params_version: u32,
}

/// One step of the ladder.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Stable number of the step.
    pub step: u16,
    /// Name for people.
    pub name: String,
    /// Verification level needed to stand on the step.
    pub min_level: u8,
    /// Amount issued …
    pub amount: Money,
    /// … every this long.
    pub per: Span,
    /// How firm the step is.
    pub status: Status,
    /// Where it comes from.
    pub source: String,
    /// What it means.
    pub note: String,
}

/// How money comes into being.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Issuance {
    /// One-time grant for a new account.
    pub start_grant: Rule<Money>,
    /// How far back a claim reaches.
    pub claim_window: Rule<Span>,
    /// Delay before issued money is usable.
    pub ripening: Rule<Span>,
    /// The steps, lowest first.
    pub ladder: Vec<Step>,
}

/// The cap of one country.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CountryRule {
    /// Two capital letters (ISO 3166-1).
    pub country: String,
    /// Gross national income per person and year, in guide units.
    pub gni_per_capita: Money,
    /// Share of it issuance may reach.
    pub share: Percent,
    /// How firm the entry is.
    pub status: Status,
    /// Where it comes from.
    pub source: String,
    /// What it means.
    pub note: String,
}

/// Upper limits of issuance.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caps {
    /// The time span the caps refer to.
    pub period: Rule<Span>,
    /// Cap for everybody.
    pub global_cap: Rule<Money>,
    /// Seniority ramp towards the cap of the country of residence.
    pub ramp: Rule<Span>,
    /// Caps per country; none = global cap only.
    #[serde(default)]
    pub country: Vec<CountryRule>,
}

/// Interest-free lending.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lending {
    /// Highest credit line of an account.
    pub max_credit_line: Rule<Money>,
    /// Highest recorded debt of an account.
    pub max_outstanding: Rule<Money>,
    /// Part of each claim withheld for repayment.
    pub repayment_share: Rule<Percent>,
}

/// The holding fee.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Demurrage {
    /// Share of a balance charged per period.
    pub rate: Rule<Percent>,
    /// Period of the charge.
    pub period: Rule<Span>,
    /// Part of a balance never charged.
    pub free_allowance: Rule<Money>,
}

/// What happens to the balance of a closed account.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClosedBalanceRule {
    /// It stays.
    Keep,
    /// It is burnt.
    Burn,
}

/// Life cycle of accounts.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accounts {
    /// Treatment of the balance of a closed account.
    pub closed_balance: Rule<ClosedBalanceRule>,
}

/// What sponsors may target.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sponsoring {
    /// Numbers of the attributes sponsors may target.
    pub keys: Rule<Vec<u16>>,
}

/// Who is a person, and for how long.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    /// How long a verification holds.
    pub membership_period: Rule<Span>,
    /// What the person key – the one identifier a person has – is made
    /// from.
    pub person_key_basis: Rule<PersonKeyBasis>,
    /// Youngest age at which a person is verified, in years; 0 = none.
    pub minimum_age: Rule<u8>,
    /// How long the move of an account to a new key waits, so that the old
    /// key can stop it. Off: at once.
    pub key_move_delay: Rule<Span>,
}

/// What the person key is made from (`docs/adr/0013-person-key.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PersonKeyBasis {
    /// Name and date of birth, as an identity document states them.
    NameBirthdate,
    /// Name and postal address, proven by a letter with a code.
    NameAddress,
    /// Country, kind and number of an identity document.
    Document,
}

impl PersonKeyBasis {
    /// The word a rules file uses for it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NameBirthdate => "name-birthdate",
            Self::NameAddress => "name-address",
            Self::Document => "document",
        }
    }
}

/// The verification of a person.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verification {
    /// Price of one verification in euro.
    pub price_eur: Rule<Money>,
}

/// The euro door: buying Capita with euro (diary 3), read by door services
/// (`services/door`) and by the genesis of a chain, not by the core.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DoorRules {
    /// Capita one euro buys.
    pub capita_per_euro: Rule<Money>,
    /// Most euro one payment may bring in.
    pub max_payment_eur: Rule<Money>,
}

/// A rule that is behaviour, with the place it lives in.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterEntry {
    /// Short stable name.
    pub id: String,
    /// The rule, in one or two sentences.
    pub rule: String,
    /// How firm it is.
    pub status: Status,
    /// Where it comes from.
    pub source: String,
    /// Files of this repository that hold the rule.
    #[serde(default)]
    pub code: Vec<String>,
    /// For a rule not built yet: the part that will hold it.
    #[serde(default)]
    pub planned: Option<String>,
}

/// A complete rules file.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSet {
    /// Name and version.
    pub set: SetInfo,
    /// How money comes into being.
    pub issuance: Issuance,
    /// Upper limits of issuance.
    pub caps: Caps,
    /// Interest-free lending.
    pub lending: Lending,
    /// The holding fee.
    pub demurrage: Demurrage,
    /// Life cycle of accounts.
    pub accounts: Accounts,
    /// What sponsors may target.
    pub sponsoring: Sponsoring,
    /// Who is a person, and for how long.
    pub identity: Identity,
    /// The verification of a person.
    pub verification: Verification,
    /// Buying Capita with euro.
    pub door: DoorRules,
    /// Rules that are behaviour and live in code.
    #[serde(default)]
    pub register: Vec<RegisterEntry>,
}

/// One line of the overview of a rule set: any rule, value or behaviour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// Where the rule stands in the file, for example `issuance.start_grant`.
    pub path: String,
    /// The value as written, or the wording of a rule of the register.
    pub value: String,
    /// How firm it is.
    pub status: Status,
    /// Where it comes from.
    pub source: String,
}

impl RuleSet {
    /// Reads a rules file and checks it ([`RuleSet::check`]).
    pub fn from_toml(text: &str) -> Result<Self, Error> {
        let set: Self = toml::from_str(text).map_err(|e| Error(e.to_string()))?;
        set.check()?;
        Ok(set)
    }

    /// The monetary rules as the parameters of the core, in force from the
    /// start of time. A host that enacts them later sets the start itself.
    pub fn params(&self) -> Result<Params, Error> {
        let ladder = self
            .issuance
            .ladder
            .iter()
            .map(|s| {
                let rate = Rate::new(s.amount.0, s.per.0).map_err(|_| {
                    Error::at(
                        &format!("issuance.ladder, step {}", s.step),
                        "\"per\" must be a span of time, not \"off\"",
                    )
                })?;
                Ok(LadderStep {
                    id: TierId(s.step),
                    min_level: VerificationLevel(s.min_level),
                    rate,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let by_country = self
            .caps
            .country
            .iter()
            .map(|c| {
                let code: [u8; 2] = c.country.as_bytes().try_into().map_err(|_| {
                    Error::at(
                        &format!("caps.country \"{}\"", c.country),
                        "a country is two capital letters, like \"BI\"",
                    )
                })?;
                Ok(CountryCap {
                    country: CountryCode(code),
                    gni_per_capita: c.gni_per_capita.0,
                    pct: c.share.0,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let params = Params {
            version: ParamsVersion(self.set.params_version),
            valid_from: Timestamp::from_secs(0),
            start_grant: self.issuance.start_grant.value.0,
            ladder,
            claim_window: self.issuance.claim_window.value.0,
            caps: CapParams {
                period: self.caps.period.value.0,
                global_cap: self.caps.global_cap.value.0,
                by_country,
                ramp: self.caps.ramp.value.0,
            },
            loans: LoanParams {
                max_credit_line: self.lending.max_credit_line.value.0,
                max_outstanding: self.lending.max_outstanding.value.0,
                repayment_share: self.lending.repayment_share.value.0,
            },
            demurrage: DemurrageParams {
                rate: self.demurrage.rate.value.0,
                period: self.demurrage.period.value.0,
                free_allowance: self.demurrage.free_allowance.value.0,
            },
            ripening: self.issuance.ripening.value.0,
            sponsor_keys: self
                .sponsoring
                .keys
                .value
                .iter()
                .map(|k| AttrKey(*k))
                .collect(),
            closed_balance: match self.accounts.closed_balance.value {
                ClosedBalanceRule::Keep => ClosedBalance::Keep,
                ClosedBalanceRule::Burn => ClosedBalance::Burn,
            },
        };
        params
            .validate()
            .map_err(|e| Error(format!("the rules do not fit together: {e}")))?;
        Ok(params)
    }

    /// The highest verification level a step of the ladder asks for.
    #[must_use]
    pub fn top_level(&self) -> u8 {
        self.issuance
            .ladder
            .iter()
            .map(|s| s.min_level)
            .max()
            .unwrap_or(0)
    }

    /// Checks everything a rules file must obey beyond its form: the
    /// monetary rules pass the validation of the core, a new account can
    /// stand on the first step, a verification holds for some time, and
    /// every entry says where it comes from.
    pub fn check(&self) -> Result<(), Error> {
        let name = &self.set.name;
        let plain = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
        if !name.starts_with(|c: char| c.is_ascii_lowercase()) || !name.chars().all(plain) {
            return Err(Error::at(
                "set.name",
                "small letters, digits and \"-\" only, starting with a letter",
            ));
        }
        if self.set.params_version == 0 {
            return Err(Error::at("set.params_version", "starts at 1"));
        }
        self.params()?;
        if self.issuance.ladder.first().map(|s| s.min_level) != Some(0) {
            return Err(Error::at(
                "issuance.ladder",
                "the first step must have min_level 0: a new account stands on it",
            ));
        }
        if self.door.capita_per_euro.value.0.is_zero() {
            return Err(Error::at("door.capita_per_euro", "must be above zero"));
        }
        if self.door.max_payment_eur.value.0.is_zero() {
            return Err(Error::at(
                "door.max_payment_eur",
                "must be above zero; to close the door, run no door service",
            ));
        }
        if self.identity.membership_period.value.0.is_zero() {
            return Err(Error::at(
                "identity.membership_period",
                "a verification must hold for some time, not \"off\"",
            ));
        }
        for line in self.lines() {
            if line.source.trim().is_empty() {
                return Err(Error::at(&line.path, "\"source\" is empty"));
            }
        }
        for (i, entry) in self.register.iter().enumerate() {
            let place = format!("register \"{}\"", entry.id);
            if entry.id.is_empty() || entry.rule.trim().is_empty() {
                return Err(Error::at(&place, "\"id\" and \"rule\" must not be empty"));
            }
            if self
                .register
                .iter()
                .skip(i.saturating_add(1))
                .any(|e| e.id == entry.id)
            {
                return Err(Error::at(&place, "this id is used twice"));
            }
            if entry.code.is_empty() == entry.planned.is_none() {
                return Err(Error::at(
                    &place,
                    "name either \"code\" (files that hold the rule) or \"planned\" (not built yet)",
                ));
            }
        }
        Ok(())
    }

    /// Every rule of the set as one line, in the order of the file.
    #[must_use]
    pub fn lines(&self) -> Vec<Line> {
        fn value<T>(path: &str, rule: &Rule<T>, written: String) -> Line {
            Line {
                path: path.to_owned(),
                value: written,
                status: rule.status,
                source: rule.source.clone(),
            }
        }
        let i = &self.issuance;
        let mut lines = vec![
            value(
                "issuance.start_grant",
                &i.start_grant,
                i.start_grant.value.to_string(),
            ),
            value(
                "issuance.claim_window",
                &i.claim_window,
                i.claim_window.value.to_string(),
            ),
            value(
                "issuance.ripening",
                &i.ripening,
                i.ripening.value.to_string(),
            ),
        ];
        lines.extend(i.ladder.iter().map(|s| Line {
            path: format!("issuance.ladder, step {} ({})", s.step, s.name),
            value: format!("{} per {}, from level {}", s.amount, s.per, s.min_level),
            status: s.status,
            source: s.source.clone(),
        }));
        let c = &self.caps;
        lines.push(value("caps.period", &c.period, c.period.value.to_string()));
        lines.push(value(
            "caps.global_cap",
            &c.global_cap,
            c.global_cap.value.to_string(),
        ));
        lines.push(value("caps.ramp", &c.ramp, c.ramp.value.to_string()));
        lines.extend(c.country.iter().map(|k| Line {
            path: format!("caps.country {}", k.country),
            value: format!("{} of {} per year", k.share, k.gni_per_capita),
            status: k.status,
            source: k.source.clone(),
        }));
        let l = &self.lending;
        lines.push(value(
            "lending.max_credit_line",
            &l.max_credit_line,
            l.max_credit_line.value.to_string(),
        ));
        lines.push(value(
            "lending.max_outstanding",
            &l.max_outstanding,
            l.max_outstanding.value.to_string(),
        ));
        lines.push(value(
            "lending.repayment_share",
            &l.repayment_share,
            l.repayment_share.value.to_string(),
        ));
        let d = &self.demurrage;
        lines.push(value("demurrage.rate", &d.rate, d.rate.value.to_string()));
        lines.push(value(
            "demurrage.period",
            &d.period,
            d.period.value.to_string(),
        ));
        lines.push(value(
            "demurrage.free_allowance",
            &d.free_allowance,
            d.free_allowance.value.to_string(),
        ));
        let closed = &self.accounts.closed_balance;
        let written = match closed.value {
            ClosedBalanceRule::Keep => "keep",
            ClosedBalanceRule::Burn => "burn",
        };
        lines.push(value("accounts.closed_balance", closed, written.to_owned()));
        let keys = &self.sponsoring.keys;
        lines.push(value("sponsoring.keys", keys, format!("{:?}", keys.value)));
        let period = &self.identity.membership_period;
        lines.push(value(
            "identity.membership_period",
            period,
            period.value.to_string(),
        ));
        let basis = &self.identity.person_key_basis;
        lines.push(value(
            "identity.person_key_basis",
            basis,
            basis.value.as_str().to_owned(),
        ));
        let age = &self.identity.minimum_age;
        lines.push(value("identity.minimum_age", age, age.value.to_string()));
        let delay = &self.identity.key_move_delay;
        lines.push(value(
            "identity.key_move_delay",
            delay,
            delay.value.to_string(),
        ));
        let price = &self.verification.price_eur;
        lines.push(value(
            "verification.price_eur",
            price,
            price.value.to_string(),
        ));
        let rate = &self.door.capita_per_euro;
        lines.push(value("door.capita_per_euro", rate, rate.value.to_string()));
        let max = &self.door.max_payment_eur;
        lines.push(value("door.max_payment_eur", max, max.value.to_string()));
        lines.extend(self.register.iter().map(|e| Line {
            path: format!("register {}", e.id),
            value: e.rule.clone(),
            status: e.status,
            source: e.source.clone(),
        }));
        lines
    }
}
