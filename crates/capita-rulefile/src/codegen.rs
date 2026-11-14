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

//! Rule sets as Rust source: what the build of `capita-rules` compiles.
//!
//! One module per rule set, named after it, with the values as constants and
//! the monetary rules as a function that builds the parameters of the core.
//! The source uses full paths and no arithmetic, so it compiles `no_std`
//! and under the lints of this workspace.

use core::fmt::Write as _;

use capita_core::params::{ClosedBalance, Params};

use crate::{Error, RuleSet};

/// Name of the module of a rule set: its name with `_` for `-`.
#[must_use]
pub fn module_name(set_name: &str) -> String {
    set_name.replace('-', "_")
}

fn params_source(p: &Params) -> String {
    let amount = |cents: u64| format!("capita_core::types::Amount::from_cents({cents})");
    let span = |secs: u64| format!("capita_core::types::Duration::from_secs({secs})");
    let share = |ppm: u32| format!("capita_core::types::Share::from_ppm({ppm}).ok()?");
    let list = |items: Vec<String>| {
        if items.is_empty() {
            String::from("alloc::vec::Vec::new()")
        } else {
            format!("alloc::vec![\n{}            ]", items.concat())
        }
    };
    let ladder = list(
        p.ladder
            .iter()
            .map(|s| {
                format!(
                    "                capita_core::params::LadderStep {{\n\
                     \x20                   id: capita_core::types::TierId({}),\n\
                     \x20                   min_level: capita_core::attributes::VerificationLevel({}),\n\
                     \x20                   rate: capita_core::types::Rate::new({}, {}).ok()?,\n\
                     \x20               }},\n",
                    s.id.0,
                    s.min_level.0,
                    amount(s.rate.amount().cents()),
                    span(s.rate.per().secs()),
                )
            })
            .collect(),
    );
    let by_country = list(
        p.caps
            .by_country
            .iter()
            .map(|c| {
                let [first, second] = c.country.0;
                format!(
                    "                capita_core::params::CountryCap {{\n\
                     \x20                   country: capita_core::types::CountryCode([{}, {}]),\n\
                     \x20                   gni_per_capita: {},\n\
                     \x20                   pct: {},\n\
                     \x20               }},\n",
                    first,
                    second,
                    amount(c.gni_per_capita.cents()),
                    share(c.pct.ppm()),
                )
            })
            .collect(),
    );
    let sponsor_keys = list(
        p.sponsor_keys
            .iter()
            .map(|k| {
                format!(
                    "                capita_core::attributes::AttrKey({}),\n",
                    k.0
                )
            })
            .collect(),
    );
    let closed_balance = match p.closed_balance {
        ClosedBalance::Keep => "Keep",
        ClosedBalance::Burn => "Burn",
    };
    format!(
        "        Some(capita_core::params::Params {{\n\
         \x20           version: capita_core::types::ParamsVersion({version}),\n\
         \x20           valid_from: capita_core::types::Timestamp::from_secs({valid_from}),\n\
         \x20           start_grant: {start_grant},\n\
         \x20           ladder: {ladder},\n\
         \x20           claim_window: {claim_window},\n\
         \x20           caps: capita_core::params::CapParams {{\n\
         \x20               period: {cap_period},\n\
         \x20               global_cap: {global_cap},\n\
         \x20               by_country: {by_country},\n\
         \x20               ramp: {ramp},\n\
         \x20           }},\n\
         \x20           loans: capita_core::params::LoanParams {{\n\
         \x20               max_credit_line: {max_credit_line},\n\
         \x20               max_outstanding: {max_outstanding},\n\
         \x20               repayment_share: {repayment_share},\n\
         \x20           }},\n\
         \x20           demurrage: capita_core::params::DemurrageParams {{\n\
         \x20               rate: {demurrage_rate},\n\
         \x20               period: {demurrage_period},\n\
         \x20               free_allowance: {free_allowance},\n\
         \x20           }},\n\
         \x20           ripening: {ripening},\n\
         \x20           sponsor_keys: {sponsor_keys},\n\
         \x20           closed_balance: capita_core::params::ClosedBalance::{closed_balance},\n\
         \x20       }})\n",
        version = p.version.0,
        valid_from = p.valid_from.secs(),
        start_grant = amount(p.start_grant.cents()),
        claim_window = span(p.claim_window.secs()),
        cap_period = span(p.caps.period.secs()),
        global_cap = amount(p.caps.global_cap.cents()),
        ramp = span(p.caps.ramp.secs()),
        max_credit_line = amount(p.loans.max_credit_line.cents()),
        max_outstanding = amount(p.loans.max_outstanding.cents()),
        repayment_share = share(p.loans.repayment_share.ppm()),
        demurrage_rate = share(p.demurrage.rate.ppm()),
        demurrage_period = span(p.demurrage.period.secs()),
        free_allowance = amount(p.demurrage.free_allowance.cents()),
        ripening = span(p.ripening.secs()),
    )
}

/// The module of one rule set.
pub fn module(set: &RuleSet) -> Result<String, Error> {
    set.check()?;
    let params = set.params()?;
    let mut out = String::new();
    let written = write!(
        out,
        "/// Rule set `{name}` – {title}. Mirrors: {diary}.\n\
         pub mod {module} {{\n\
         \x20   /// Name of the rule set.\n\
         \x20   pub const NAME: &str = {name:?};\n\
         \x20   /// Name of the rule set for people.\n\
         \x20   pub const TITLE: &str = {title:?};\n\
         \x20   /// The version of the owner's rules diary the set mirrors.\n\
         \x20   pub const DIARY: &str = {diary:?};\n\
         \x20   /// How long a verification holds (`identity.membership_period`).\n\
         \x20   pub const MEMBERSHIP_PERIOD: capita_core::types::Duration =\n\
         \x20       capita_core::types::Duration::from_secs({membership});\n\
         \x20   /// What the person key is made from (`identity.person_key_basis`).\n\
         \x20   pub const PERSON_KEY_BASIS: &str = {basis:?};\n\
         \x20   /// Youngest age at which a person is verified, in years; 0 = none\n\
         \x20   /// (`identity.minimum_age`).\n\
         \x20   pub const MINIMUM_AGE_YEARS: u8 = {age};\n\
         \x20   /// How long the move of an account to a new key waits\n\
         \x20   /// (`identity.key_move_delay`).\n\
         \x20   pub const KEY_MOVE_DELAY: capita_core::types::Duration =\n\
         \x20       capita_core::types::Duration::from_secs({delay});\n\
         \x20   /// The highest verification level a step of the ladder asks for.\n\
         \x20   pub const TOP_LEVEL: capita_core::attributes::VerificationLevel =\n\
         \x20       capita_core::attributes::VerificationLevel({top_level});\n\
         \x20   /// Price of one verification in euro cents (`verification.price_eur`).\n\
         \x20   pub const VERIFICATION_PRICE_EUR_CENTS: u64 = {price};\n\
         \n\
         \x20   /// The monetary rules as parameters of the core, in force from the\n\
         \x20   /// start of time. Checked when this crate was built: never `None`.\n\
         \x20   #[must_use]\n\
         \x20   pub fn params() -> Option<capita_core::params::Params> {{\n\
         {params}\
         \x20   }}\n\
         }}\n",
        name = set.set.name,
        title = set.set.title,
        diary = set.set.diary,
        module = module_name(&set.set.name),
        membership = set.identity.membership_period.value.0.secs(),
        basis = set.identity.person_key_basis.value.as_str(),
        age = set.identity.minimum_age.value,
        delay = set.identity.key_move_delay.value.0.secs(),
        top_level = set.top_level(),
        price = set.verification.price_eur.value.0.cents(),
        params = params_source(&params),
    );
    written.map_err(|e| Error(e.to_string()))?;
    Ok(out)
}

/// All rule sets as one source file: a module each, the list of their
/// names, and the parameters by name.
pub fn library(sets: &[RuleSet]) -> Result<String, Error> {
    let mut out =
        String::from("// Generated from the files of `rules/`. Do not edit: edit those.\n\n");
    let mut names = String::new();
    let mut arms = String::new();
    for (i, set) in sets.iter().enumerate() {
        let name = &set.set.name;
        if sets
            .iter()
            .skip(i.saturating_add(1))
            .any(|s| &s.set.name == name)
        {
            return Err(Error(format!("two rule sets are named \"{name}\"")));
        }
        out.push_str(&module(set)?);
        out.push('\n');
        names.push_str(&format!("{name:?}, "));
        arms.push_str(&format!(
            "        {name:?} => {}::params(),\n",
            module_name(name)
        ));
    }
    out.push_str(&format!(
        "/// Names of all rule sets.\n\
         pub const SETS: &[&str] = &[{names}];\n\
         \n\
         /// The monetary rules of the rule set `name`.\n\
         #[must_use]\n\
         pub fn params_of(name: &str) -> Option<capita_core::params::Params> {{\n\
         \x20   match name {{\n\
         {arms}\
         \x20       _ => None,\n\
         \x20   }}\n\
         }}\n"
    ));
    Ok(out)
}
