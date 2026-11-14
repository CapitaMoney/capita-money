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

//! What the reader makes of good and bad input, and whether the rules files
//! of this repository are valid.
//!
//! Values are expected only from `tests/fixture.toml`, a file with fixed
//! numbers: a change of the rules must not break a test.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use capita_core::types::{Amount, Duration, Share};
use capita_rulefile::units::{Money, Percent, Span};
use capita_rulefile::{codegen, RuleSet, Status};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn file(name: &str) -> String {
    std::fs::read_to_string(root().join("rules").join(name)).unwrap()
}

fn alpha() -> RuleSet {
    RuleSet::from_toml(&file("alpha.toml")).unwrap()
}

const FIXTURE: &str = include_str!("fixture.toml");

fn fixture() -> RuleSet {
    RuleSet::from_toml(FIXTURE).unwrap()
}

/// The fixture with `from` replaced by `to` (which must occur).
fn fixture_with(from: &str, to: &str) -> Result<RuleSet, String> {
    assert!(FIXTURE.contains(from), "the fixture has no {from:?}");
    RuleSet::from_toml(&FIXTURE.replacen(from, to, 1)).map_err(|e| e.0)
}

#[test]
fn money_is_read_to_the_cent() {
    let cents = |text: &str| text.parse::<Money>().map(|m| m.0.cents());
    assert_eq!(cents("1.00"), Ok(100));
    assert_eq!(cents("10"), Ok(1_000));
    assert_eq!(cents("0.5"), Ok(50));
    assert_eq!(cents("1_000_000.00"), Ok(100_000_000));
    for wrong in [
        "", "1.005", "-1.00", "1,00", "1.", ".5", "_1", "1 Capita", "ten",
    ] {
        assert!(cents(wrong).is_err(), "{wrong:?} was read as money");
    }
    assert_eq!(Money(Amount::from_cents(1_005)).to_string(), "10.05");
}

#[test]
fn time_is_read_in_the_units_of_the_core() {
    let secs = |text: &str| text.parse::<Span>().map(|s| s.0.secs());
    assert_eq!(secs("off"), Ok(0));
    assert_eq!(secs("90 seconds"), Ok(90));
    assert_eq!(secs("1 minute"), Ok(60));
    assert_eq!(secs("14 days"), Ok(14 * 86_400));
    assert_eq!(secs("1 month"), Ok(Duration::MONTH.secs()));
    assert_eq!(secs("2 years"), Ok(2 * Duration::YEAR.secs()));
    for wrong in [
        "",
        "14",
        "days",
        "1.5 days",
        "14 days ago",
        "3 fortnights",
        "0",
    ] {
        assert!(secs(wrong).is_err(), "{wrong:?} was read as time");
    }
    assert!(secs("999999999999999 years").is_err());
    for text in [
        "off",
        "90 seconds",
        "1 minute",
        "3 days",
        "2 weeks",
        "365 days",
        "1 month",
        "2 years",
    ] {
        assert_eq!(text.parse::<Span>().unwrap().to_string(), text);
    }
}

#[test]
fn shares_are_read_to_parts_per_million() {
    let ppm = |text: &str| text.parse::<Percent>().map(|p| p.0.ppm());
    assert_eq!(ppm("off"), Ok(0));
    assert_eq!(ppm("50 %"), Ok(500_000));
    assert_eq!(ppm("50%"), Ok(500_000));
    assert_eq!(ppm("0.5 %"), Ok(5_000));
    assert_eq!(ppm("0.0001 %"), Ok(1));
    assert_eq!(ppm("100 %"), Ok(Share::ONE.ppm()));
    for wrong in ["", "50", "101 %", "0.00001 %", "-5 %", "half"] {
        assert!(ppm(wrong).is_err(), "{wrong:?} was read as a share");
    }
    for text in ["off", "50 %", "0.5 %", "0.0001 %", "100 %"] {
        assert_eq!(text.parse::<Percent>().unwrap().to_string(), text);
    }
}

#[test]
fn every_rules_file_of_the_repository_is_valid_and_named_after_its_set() {
    let mut seen = 0;
    for entry in std::fs::read_dir(root().join("rules")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            let set = RuleSet::from_toml(&std::fs::read_to_string(&path).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(
                path.file_stem().unwrap().to_str(),
                Some(set.set.name.as_str())
            );
            set.params().unwrap().validate().unwrap();
            seen += 1;
        }
    }
    assert!(seen >= 2);
}

#[test]
fn a_file_becomes_the_parameters_of_the_core() {
    let set = fixture();
    let p = set.params().unwrap();
    assert_eq!(p.version.0, 1);
    assert_eq!(p.start_grant, Amount::from_cents(100));
    assert_eq!(p.ladder.len(), 2);
    assert!(p.ladder[0].rate.is_zero());
    assert_eq!(p.ladder[1].rate.amount(), Amount::from_cents(1_000));
    assert_eq!(p.ladder[1].rate.per(), Duration::MONTH);
    assert_eq!(p.claim_window.secs(), 14 * 86_400);
    assert_eq!(p.caps.global_cap, Amount::from_cents(100_000_000));
    assert_eq!(p.loans.repayment_share.ppm(), 500_000);
    assert!(p.demurrage.rate.is_zero() && p.ripening.is_zero() && p.caps.ramp.is_zero());
    assert!(p.caps.by_country.is_empty() && p.sponsor_keys.is_empty());
    assert_eq!(set.top_level(), 1);
    assert_eq!(set.identity.membership_period.value.0.secs(), 365 * 86_400);
    assert_eq!(set.verification.price_eur.value.0.cents(), 100);
    assert_eq!(
        set.identity.person_key_basis.value,
        capita_rulefile::PersonKeyBasis::NameBirthdate
    );
    assert_eq!(set.identity.minimum_age.value, 0);
    assert_eq!(set.identity.key_move_delay.value.0.secs(), 7 * 86_400);
    assert_eq!(set.issuance.start_grant.status, Status::Decided);
}

#[test]
fn caps_per_country_are_read() {
    let with_country = "[[caps.country]]\ncountry = \"BI\"\ngni_per_capita = \"230.00\"\n\
        share = \"20 %\"\nstatus = \"example\"\nsource = \"A3\"\nnote = \"n\"\n\n[lending.max_credit_line]";
    let set = fixture_with("[lending.max_credit_line]", with_country).unwrap();
    let p = set.params().unwrap();
    assert_eq!(p.caps.by_country.len(), 1);
    assert_eq!(p.caps.by_country[0].country.0, *b"BI");
    assert_eq!(p.caps.by_country[0].pct.ppm(), 200_000);
    let source = codegen::module(&set).unwrap();
    assert!(source.contains("CountryCode([66, 73])"));
    let problem = fixture_with(
        "[lending.max_credit_line]",
        &with_country.replace("BI", "Burundi"),
    );
    assert!(problem.unwrap_err().contains("two capital letters"));
}

#[test]
fn the_register_names_files_that_exist() {
    let set = alpha();
    assert!(!set.register.is_empty());
    for entry in &set.register {
        for path in &entry.code {
            assert!(
                root().join(path).is_file(),
                "register \"{}\" names {path}, which is no file",
                entry.id
            );
        }
    }
}

#[test]
fn the_overview_lists_every_rule_with_its_status() {
    let set = fixture();
    let lines = set.lines();
    let values = 21 + set.issuance.ladder.len() + set.caps.country.len();
    assert_eq!(lines.len(), values + set.register.len());
    let line = |path: &str| lines.iter().find(|l| l.path == path).unwrap();
    assert_eq!(line("issuance.start_grant").value, "1.00");
    assert_eq!(line("issuance.claim_window").value, "2 weeks");
    assert_eq!(line("identity.membership_period").value, "365 days");
    assert_eq!(line("demurrage.rate").value, "off");
    assert_eq!(line("door.max_payment_eur").value, "100.00");
    assert_eq!(line("lending.repayment_share").status, Status::Example);
}

#[test]
fn a_wrong_value_is_refused_with_the_name_of_the_rule() {
    let problem = fixture_with("value = \"14 days\"", "value = \"14 dys\"").unwrap_err();
    assert!(problem.contains("not a span of time"), "{problem}");
    assert!(problem.contains("14 dys"), "{problem}");

    let problem = fixture_with("value = \"50 %\"", "value = \"150 %\"").unwrap_err();
    assert!(problem.contains("not a share"), "{problem}");

    let problem = fixture_with("status = \"decided\"", "status = \"maybe\"").unwrap_err();
    assert!(problem.contains("maybe"), "{problem}");

    // a key the reader does not know: a typing error must not pass silently
    let problem = fixture_with("[issuance.ripening]", "[issuance.ripning]").unwrap_err();
    assert!(problem.contains("ripning"), "{problem}");
}

#[test]
fn rules_that_do_not_fit_together_are_refused() {
    // a claim that reaches back no time at all
    let problem = fixture_with("value = \"14 days\"", "value = \"off\"").unwrap_err();
    assert!(problem.contains("claim window"), "{problem}");

    // a step issued "every no time"
    let problem = fixture_with("per = \"1 month\"", "per = \"off\"").unwrap_err();
    assert!(problem.contains("issuance.ladder, step 0"), "{problem}");

    // a new account must be able to stand on the first step
    let problem = fixture_with("min_level = 0", "min_level = 1").unwrap_err();
    assert!(problem.contains("first step"), "{problem}");

    // a verification that holds for no time
    let problem = fixture_with("value = \"365 days\"", "value = \"off\"").unwrap_err();
    assert!(problem.contains("identity.membership_period"), "{problem}");

    // a rule of the register without a place
    let problem = fixture_with("planned = \"market (M5)\"", "").unwrap_err();
    assert!(problem.contains("digital-goods-first"), "{problem}");
}

#[test]
fn the_source_of_a_rule_set_names_its_values() {
    let set = fixture();
    assert_eq!(codegen::module_name("dev-fast"), "dev_fast");
    let source = codegen::module(&set).unwrap();
    assert!(source.contains("pub mod fixture {"));
    assert!(source.contains("Duration::from_secs(31536000)"));
    assert!(source.contains("VERIFICATION_PRICE_EUR_CENTS: u64 = 100"));
    assert!(source.contains("PERSON_KEY_BASIS: &str = \"name-birthdate\""));
    assert!(source.contains("Duration::from_secs(604800)"));
    let both = codegen::library(&[set.clone(), set]).unwrap_err();
    assert!(both.0.contains("two rule sets"), "{}", both.0);
}
