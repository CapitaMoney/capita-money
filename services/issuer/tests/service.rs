// Capita Money – capita-issuer
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

//! The identity service, from a checked person to a claim the chain
//! accepts: new persons, the same person again, a lost key, a new secret, a
//! new name, namesakes, age, tickets – and the same through its interface,
//! with a Challenger service played by the test.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::collections::BTreeSet;

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use capita_core::attributes::VerificationLevel;
use capita_core::types::{Duration, Timestamp};
use capita_credential::check::{check_claim, person_id, Crypto};
use capita_credential::types::{
    Audience, Issuer, IssuerId, IssuerStatus, PublicKey, Realm, Signature, SubjectRef,
};
use capita_issuer::checks::{full_name, Challenger, ChallengerConfig, Stated};
use capita_issuer::config::Config;
use capita_issuer::http::App;
use capita_issuer::issue::{
    Action, Issued, Payment, Person, Refusal, Request, Rules, Service, Settings,
};
use capita_issuer::store::{FileStore, MemoryStore, Store};
use capita_issuer::{hex, unhex_n};
use capita_person_key::canonical::{Basis, Date, Name};
use capita_person_key::LocalKey;
use serde_json::{json, Value};

const NOW: u64 = 1_794_614_400; // 2026-11-14
const YEAR: u64 = 365 * 86_400;
const AUDIENCE: Audience = Audience([7; 32]);

/// The host side: what a chain does with a signature and a hash.
struct Host;

impl Crypto for Host {
    fn verify(key: &PublicKey, message: &[u8], signature: &Signature) -> bool {
        match (key, signature) {
            (PublicKey::Ed25519(key), Signature::Ed25519(signature)) => {
                ed25519_dalek::VerifyingKey::from_bytes(key).is_ok_and(|key| {
                    key.verify_strict(message, &ed25519_dalek::Signature::from_bytes(signature))
                        .is_ok()
                })
            }
            _ => false,
        }
    }

    fn hash(data: &[u8]) -> [u8; 32] {
        Blake2b::<U32>::digest(data).into()
    }
}

fn rules(basis: Basis) -> Rules {
    Rules {
        basis,
        minimum_age: 0,
        validity: YEAR,
        level: 1,
    }
}

fn settings(basis: Basis) -> Settings {
    Settings {
        audience: AUDIENCE,
        issuer: IssuerId(0),
        rules: rules(basis),
        payment: Payment::Off,
        allow_name_change: true,
    }
}

fn service(settings: Settings) -> Service<LocalKey, MemoryStore> {
    let key = LocalKey::from_secret(&[1; 32]).unwrap();
    Service::new(settings, key, vec![], &[2; 32], MemoryStore::default())
}

fn person(family: &str, given: &str, born: &str) -> Person {
    Person::NameBirthdate {
        name: Name {
            family: family.into(),
            given: given.into(),
        },
        born: Date::parse(born).unwrap(),
        place_of_birth: None,
    }
}

fn born_in(family: &str, given: &str, born: &str, place: &str) -> Person {
    match person(family, given, born) {
        Person::NameBirthdate { name, born, .. } => Person::NameBirthdate {
            name,
            born,
            place_of_birth: Some(place.into()),
        },
        other => other,
    }
}

fn request(account: u8, person: Person) -> Request {
    Request {
        account: [account; 32],
        person,
        proof: "test".into(),
        former: None,
        ticket: None,
    }
}

/// The entry the chain lists for the service, and the check it runs.
fn accepted(public_key: [u8; 32], issued: &Issued, now: u64) -> bool {
    let issuer = Issuer {
        did: b"did:web:id.example.org".to_vec(),
        keys: vec![PublicKey::Ed25519(public_key)],
        realm: Realm(1),
        max_level: VerificationLevel(1),
        residence: false,
        attributes: vec![],
        max_validity: Some(Duration::from_secs(YEAR)),
        status: IssuerStatus::Active,
    };
    check_claim::<Host>(
        &issued.claim,
        &Signature::Ed25519(issued.signature),
        IssuerId(0),
        &issuer,
        &AUDIENCE,
        Timestamp::from_secs(now),
    )
    .is_ok()
}

#[test]
fn a_checked_person_gets_a_claim_the_chain_accepts_and_the_service_forgets() {
    let mut service = service(settings(Basis::NameBirthdate));
    let zoe = person("Müller", "Zoë Anna", "1990-07-04");
    let issued = service.issue(request(1, zoe.clone()), NOW).unwrap();
    assert_eq!(issued.action, Action::Present);
    assert_eq!(issued.claim.subject.0, [1; 32]);
    assert_eq!(issued.claim.level, VerificationLevel(1));
    assert_eq!(issued.claim.replaces, None);
    assert_eq!(
        issued.claim.expires_at,
        Some(Timestamp::from_secs(NOW + YEAR))
    );
    assert!(accepted(service.public_key(), &issued, NOW + 60));
    // signed for this chain and no other
    assert!(!{
        let mut elsewhere = issued.clone();
        elsewhere.claim.subject.0 = [9; 32];
        accepted(service.public_key(), &elsewhere, NOW + 60)
    });
    // what is kept: key, account, number, day, kind of proof – no name
    let record = service.store().by_account(&hex(&[1; 32])).unwrap();
    assert_eq!(record.person, hex(&issued.claim.subject_ref.0));
    assert_eq!((record.issued_at, record.proof.as_str()), (NOW, "test"));
    let kept = serde_json::to_string(&record).unwrap().to_lowercase();
    for word in ["müller", "muller", "muler", "zo", "1990", "0704"] {
        assert!(!kept.contains(word), "{word} in {kept}");
    }
    // the same person a year later, spelled as another document spells
    // them: the same key, a new number
    let again = service
        .issue(
            request(1, person("MUELLER", "ZOE", "1990-07-04")),
            NOW + YEAR,
        )
        .unwrap();
    assert_eq!(again.action, Action::Present);
    assert_eq!(again.claim.subject_ref, issued.claim.subject_ref);
    assert_ne!(again.claim.serial, issued.claim.serial);
    // somebody else: another key
    let other = service
        .issue(request(2, person("Müller", "Zoë", "1990-07-05")), NOW)
        .unwrap();
    assert_ne!(other.claim.subject_ref, issued.claim.subject_ref);
    assert_eq!(service.store().count, 3);
}

#[test]
fn a_person_with_a_new_key_is_told_to_move_the_account() {
    let mut service = service(settings(Basis::NameBirthdate));
    let zoe = || person("Müller", "Zoë", "1990-07-04");
    let first = service.issue(request(1, zoe()), NOW).unwrap();
    // the key of account 1 is lost; verified again for account 2
    let second = service.issue(request(2, zoe()), NOW + 86_400).unwrap();
    assert_eq!(second.action, Action::Move { from: [1; 32] });
    assert_eq!(second.claim.subject.0, [2; 32]);
    assert_eq!(second.claim.subject_ref, first.claim.subject_ref);
    assert!(accepted(service.public_key(), &second, NOW + 86_400));
    // from now on account 2 is the account of the person
    let third = service.issue(request(2, zoe()), NOW + YEAR).unwrap();
    assert_eq!(third.action, Action::Present);
    // an account of one person is never verified for another
    assert_eq!(
        service.issue(request(2, person("Kim", "Min", "1980-01-01")), NOW + YEAR),
        Err(Refusal::AccountOfAnotherPerson)
    );
}

#[test]
fn a_new_secret_or_a_new_name_keeps_the_account() {
    // The secret of the person keys was replaced: [1; 32] is the old one.
    let mut old = service(settings(Basis::NameBirthdate));
    let zoe = || person("Müller", "Zoë", "1990-07-04");
    let before = old.issue(request(1, zoe()), NOW).unwrap();
    let mut store = MemoryStore::default();
    store
        .record(old.store().by_account(&hex(&[1; 32])).unwrap())
        .unwrap();
    let mut service = Service::new(
        settings(Basis::NameBirthdate),
        LocalKey::from_secret(&[5; 32]).unwrap(),
        vec![LocalKey::from_secret(&[1; 32]).unwrap()],
        &[2; 32],
        store,
    );
    let after = service.issue(request(1, zoe()), NOW + YEAR).unwrap();
    assert_eq!(after.action, Action::Present);
    assert_ne!(after.claim.subject_ref, before.claim.subject_ref);
    assert_eq!(after.claim.replaces, Some(before.claim.subject_ref));
    // on the chain the two keys are one person from now on
    assert_ne!(
        person_id::<Host>(Realm(1), &after.claim.subject_ref),
        person_id::<Host>(Realm(1), &before.claim.subject_ref)
    );
    // next year nothing is replaced any more
    let later = service.issue(request(1, zoe()), NOW + 2 * YEAR).unwrap();
    assert_eq!(
        (later.claim.subject_ref, later.claim.replaces),
        (after.claim.subject_ref, None)
    );
    // verified under the old secret, key lost, back under the new one:
    // a move, and the claim names the key the old account is bound to
    let kim = || person("Kim", "Min", "1980-01-01");
    let mut old = self::service(settings(Basis::NameBirthdate));
    let kim_before = old.issue(request(3, kim()), NOW).unwrap();
    let mut store = MemoryStore::default();
    store
        .record(old.store().by_account(&hex(&[3; 32])).unwrap())
        .unwrap();
    let mut service = Service::new(
        settings(Basis::NameBirthdate),
        LocalKey::from_secret(&[5; 32]).unwrap(),
        vec![LocalKey::from_secret(&[1; 32]).unwrap()],
        &[2; 32],
        store,
    );
    let moved = service.issue(request(4, kim()), NOW + YEAR).unwrap();
    assert_eq!(moved.action, Action::Move { from: [3; 32] });
    assert_eq!(moved.claim.replaces, Some(kim_before.claim.subject_ref));

    // A new name: the person states the former one.
    let mut service = self::service(settings(Basis::NameBirthdate));
    let maiden = service.issue(request(1, zoe()), NOW).unwrap();
    let married = || person("Schmidt", "Zoë", "1990-07-04");
    let mut ask = request(1, married());
    ask.former = Some(zoe());
    let renamed = service.issue(ask, NOW + YEAR).unwrap();
    assert_eq!(renamed.action, Action::Present);
    assert_eq!(renamed.claim.replaces, Some(maiden.claim.subject_ref));
    // a former name that is not what this account is bound to, another
    // birthday, or a service that does not take former names: refused
    let mut wrong = request(1, person("Meier", "Zoë", "1990-07-04"));
    wrong.former = Some(person("Huber", "Zoë", "1990-07-04"));
    assert_eq!(
        service.issue(wrong, NOW + YEAR),
        Err(Refusal::FormerDataUnknown)
    );
    let mut other_day = request(1, person("Meier", "Zoë", "1991-07-04"));
    other_day.former = Some(married());
    assert_eq!(
        service.issue(other_day, NOW + YEAR),
        Err(Refusal::FormerDataUnknown)
    );
    let mut strict = self::service(Settings {
        allow_name_change: false,
        ..settings(Basis::NameBirthdate)
    });
    strict.issue(request(1, zoe()), NOW).unwrap();
    let mut ask = request(1, married());
    ask.former = Some(zoe());
    assert_eq!(
        strict.issue(ask, NOW + YEAR),
        Err(Refusal::FormerDataUnknown)
    );
    // without stating the former name the account is somebody else's
    assert_eq!(
        strict.issue(request(1, married()), NOW + YEAR),
        Err(Refusal::AccountOfAnotherPerson)
    );
}

#[test]
fn namesakes_born_the_same_day_are_two_people_if_a_further_fact_differs() {
    let mut service = service(settings(Basis::NameBirthdate));
    let seoul = || born_in("Kim", "Min", "1990-07-04", "Seoul");
    let busan = || born_in("Kim", "Min", "1990-07-04", "Busan");
    let first = service.issue(request(1, seoul()), NOW).unwrap();
    let second = service.issue(request(2, busan()), NOW).unwrap();
    assert_eq!(second.action, Action::Present);
    assert_ne!(second.claim.subject_ref, first.claim.subject_ref);
    // each of them is found again
    let again = service.issue(request(2, busan()), NOW + YEAR).unwrap();
    assert_eq!(
        (again.action, again.claim.subject_ref),
        (Action::Present, second.claim.subject_ref)
    );
    let again = service.issue(request(1, seoul()), NOW + YEAR).unwrap();
    assert_eq!(
        (again.action, again.claim.subject_ref),
        (Action::Present, first.claim.subject_ref)
    );
    // without the further fact a namesake is the same person: a move
    let unknown = service
        .issue(request(3, person("Kim", "Min", "1990-07-04")), NOW)
        .unwrap();
    assert_eq!(unknown.action, Action::Move { from: [1; 32] });
    // the service tells four apart, not more
    for (account, place) in [(4, "Daegu"), (5, "Incheon")] {
        service
            .issue(
                request(account, born_in("Kim", "Min", "1990-07-04", place)),
                NOW,
            )
            .unwrap();
    }
    assert_eq!(
        service.issue(
            request(6, born_in("Kim", "Min", "1990-07-04", "Ulsan")),
            NOW
        ),
        Err(Refusal::TooManyNamesakes)
    );
}

#[test]
fn rules_from_the_rules_file_decide_basis_and_age() {
    // the rules file of this repository
    let set =
        capita_rulefile::RuleSet::from_toml(include_str!("../../../rules/alpha.toml")).unwrap();
    let from_file = Rules::from_set(&set, 1).unwrap();
    assert_eq!(
        from_file.validity,
        set.identity.membership_period.value.0.secs()
    );
    assert!(Rules::from_set(&set, 0).is_err() && Rules::from_set(&set, 200).is_err());

    // another basis than the check established
    let mut by_address = service(settings(Basis::NameAddress));
    assert_eq!(
        by_address.issue(request(1, person("Müller", "Zoë", "1990-07-04")), NOW),
        Err(Refusal::WrongBasis)
    );
    let at = |street: &str| Person::NameAddress {
        name: full_name("Zoë Anna Müller"),
        country: "DE".into(),
        postcode: "79106".into(),
        street: street.into(),
    };
    let first = by_address
        .issue(request(1, at("Fendrichstraße 12")), NOW)
        .unwrap();
    let again = by_address
        .issue(request(1, at("Fendrichstr. 12")), NOW + YEAR)
        .unwrap();
    assert_eq!(again.claim.subject_ref, first.claim.subject_ref);
    // … which is where this basis ends: after a move the person is new
    let moved = by_address
        .issue(request(2, at("Hauptstraße 1")), NOW + YEAR)
        .unwrap();
    assert_eq!(moved.action, Action::Present);

    // a minimum age
    let mut adults = service(Settings {
        rules: Rules {
            minimum_age: 18,
            ..rules(Basis::NameBirthdate)
        },
        ..settings(Basis::NameBirthdate)
    });
    assert_eq!(
        adults.issue(request(1, person("Kim", "Min", "2008-11-15")), NOW),
        Err(Refusal::TooYoung)
    );
    assert!(adults
        .issue(request(1, person("Kim", "Min", "2008-11-14")), NOW)
        .is_ok());
    let mut by_document = service(Settings {
        rules: Rules {
            minimum_age: 18,
            ..rules(Basis::Document)
        },
        ..settings(Basis::Document)
    });
    let passport = |born| Person::Document {
        state: "DEU".into(),
        kind: "P".into(),
        number: "C01X00T47".into(),
        born,
    };
    assert_eq!(
        by_document.issue(request(1, passport(None)), NOW),
        Err(Refusal::AgeUnknown)
    );
    assert!(by_document
        .issue(
            request(1, passport(Some(Date::parse("1990-07-04").unwrap()))),
            NOW
        )
        .is_ok());
    // nothing to make a key from
    assert!(matches!(
        service(settings(Basis::NameBirthdate))
            .issue(request(1, person("李", "Zoë", "1990-07-04")), NOW),
        Err(Refusal::Data(_))
    ));
}

#[test]
fn a_ticket_pays_one_verification() {
    let tickets: BTreeSet<String> = ["T-1", "T-2"].into_iter().map(String::from).collect();
    let mut service = service(Settings {
        payment: Payment::Tickets(tickets),
        ..settings(Basis::NameBirthdate)
    });
    let with = |account, ticket: Option<&str>| Request {
        ticket: ticket.map(String::from),
        ..request(account, person("Kim", "Min", "1980-01-01"))
    };
    assert_eq!(service.issue(with(1, None), NOW), Err(Refusal::NotPaid));
    assert_eq!(
        service.issue(with(1, Some("T-9")), NOW),
        Err(Refusal::NotPaid)
    );
    assert_eq!(service.store().count, 0);
    assert!(service.issue(with(1, Some("T-1")), NOW).is_ok());
    assert_eq!(
        service.issue(with(1, Some("T-1")), NOW + YEAR),
        Err(Refusal::NotPaid)
    );
    assert!(service.issue(with(1, Some("T-2")), NOW + YEAR).is_ok());
}

#[test]
fn tickets_paid_after_the_start_are_taken() {
    let mut service = service(Settings {
        payment: Payment::Tickets(BTreeSet::new()),
        ..settings(Basis::NameBirthdate)
    });
    let ask = Request {
        ticket: Some("NEW-1".into()),
        ..request(1, person("Kim", "Min", "1980-01-01"))
    };
    assert_eq!(service.issue(ask.clone(), NOW), Err(Refusal::NotPaid));
    assert!(!service.knows_ticket("NEW-1"));
    let codes = capita_issuer::config::ticket_codes("# euro door\nNEW-1\n\nNEW-1\n");
    assert_eq!(service.add_tickets(codes), 1);
    assert!(service.knows_ticket("NEW-1"));
    assert!(service.issue(ask, NOW).is_ok());
    // Payment off: codes change nothing.
    let mut free = service_off();
    assert_eq!(free.add_tickets(["X".to_owned()]), 0);
    assert!(!free.knows_ticket("X"));
}

fn service_off() -> Service<LocalKey, MemoryStore> {
    service(settings(Basis::NameBirthdate))
}

#[test]
fn a_revocation_is_signed_for_the_chain() {
    let service = service(settings(Basis::NameBirthdate));
    let (revocation, signature) = service.revoke([9; 16]);
    let issuer = Issuer {
        did: b"did:web:id.example.org".to_vec(),
        keys: vec![PublicKey::Ed25519(service.public_key())],
        realm: Realm(1),
        max_level: VerificationLevel(1),
        residence: false,
        attributes: vec![],
        max_validity: None,
        status: IssuerStatus::Active,
    };
    assert!(capita_credential::check::check_revocation::<Host>(
        &revocation,
        &Signature::Ed25519(signature),
        IssuerId(0),
        &issuer,
        &AUDIENCE
    )
    .is_ok());
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("capita-issuer-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_configured_service_remembers_across_restarts() {
    let dir = temp("config");
    let write = |name: &str, text: &str| {
        std::fs::write(dir.join(name), text).unwrap();
        dir.join(name)
    };
    write("seed.hex", &hex(&[2; 32]));
    write("secret.hex", &format!("0x{}\n", hex(&[1; 32])));
    write("tickets.txt", "# sponsored\nT-1\n\nT-2\n");
    let rules = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules/alpha.toml");
    let config = write(
        "issuer.toml",
        &format!(
            "listen = \"127.0.0.1:0\"\nrules = {rules:?}\nstore = {store:?}\n\
             [host]\naudience = \"0x{audience}\"\nissuer = 0\n\
             [keys]\nsigning_seed_file = {seed:?}\nperson_key_secret_file = {secret:?}\n\
             [policy]\nlevel = 1\n[checks]\ntest = true\n\
             [payment]\ntickets_file = {tickets:?}\n",
            store = dir.join("store.jsonl"),
            audience = hex(&AUDIENCE.0),
            seed = dir.join("seed.hex"),
            secret = dir.join("secret.hex"),
            tickets = dir.join("tickets.txt"),
        ),
    );
    let config = Config::load(&config).unwrap();
    assert!(config.checks.test && config.checks.challenger.is_none());
    let mut service = config.service().unwrap();
    // the same keys as the service of the other tests
    assert_eq!(
        service.public_key(),
        self::service(settings(Basis::NameBirthdate)).public_key()
    );
    let ask = |account, ticket: &str| Request {
        ticket: Some(ticket.into()),
        ..request(account, person("Kim", "Min", "1980-01-01"))
    };
    let first = service.issue(ask(1, "T-1"), NOW).unwrap();
    drop(service);
    // restarted: the person and the used ticket are known
    let mut service = config.service().unwrap();
    assert_eq!(service.issue(ask(2, "T-1"), NOW), Err(Refusal::NotPaid));
    let second = service.issue(ask(2, "T-2"), NOW).unwrap();
    assert_eq!(second.action, Action::Move { from: [1; 32] });
    assert_eq!(second.claim.subject_ref, first.claim.subject_ref);
    let lines = std::fs::read_to_string(dir.join("store.jsonl")).unwrap();
    assert_eq!(lines.lines().count(), 2);
    assert!(!lines.to_lowercase().contains("kim"));
    // a store that is not what the service wrote is refused, not ignored
    std::fs::write(dir.join("store.jsonl"), "{\"person\": 1}\n").unwrap();
    assert!(FileStore::open(&dir.join("store.jsonl")).is_err());
    // the example configuration of the repository is one
    let example: Config = toml_example();
    assert!(!example.checks.test);
    let _ = std::fs::remove_dir_all(&dir);
}

fn toml_example() -> Config {
    let dir = temp("example");
    let path = dir.join("issuer.toml");
    std::fs::write(&path, include_str!("../issuer.example.toml")).unwrap();
    let config = Config::load(&path).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    config
}

fn stated(family: &str, given: &str, born: &str) -> Value {
    json!({ "family_name": family, "given_names": given, "birth_date": born })
}

#[test]
fn the_interface_answers_with_the_call_for_the_chain() {
    let app = App::new(service(settings(Basis::NameBirthdate)), true, None);
    let (status, info) = app.handle("GET", "/v1/info", b"", NOW);
    assert_eq!(status, 200);
    assert_eq!(info["basis"], "name-birthdate");
    assert_eq!(info["checks"], json!(["test"]));
    assert_eq!(info["audience"], format!("0x{}", hex(&AUDIENCE.0)));
    let post = |body: Value| app.handle("POST", "/v1/test-check", body.to_string().as_bytes(), NOW);

    let account = format!("0x{}", hex(&[1; 32]));
    let (status, call) =
        post(json!({ "account": account, "person": stated("Müller", "Zoë", "1990-07-04") }));
    assert_eq!(status, 200, "{call}");
    assert_eq!(call["call"], "credential.present");
    assert_eq!(call["from"], Value::Null);
    assert_eq!(call["claim"]["subject"], account);
    assert_eq!(call["claim"]["level"], 1);
    assert_eq!(call["claim"]["expiresAt"], NOW + YEAR);
    assert_eq!(
        unhex_n::<64>(call["signature"].as_str().unwrap()).map(|_| ()),
        Some(())
    );
    // nothing about the person in the answer
    assert!(!call.to_string().to_lowercase().contains("ller"));
    // a lost key
    let new_account = format!("0x{}", hex(&[2; 32]));
    let (_, moved) =
        post(json!({ "account": new_account, "person": stated("MUELLER", "ZOE", "1990-07-04") }));
    assert_eq!(moved["call"], "credential.requestMove");
    assert_eq!(moved["from"], account);
    assert_eq!(moved["claim"]["subjectRef"], call["claim"]["subjectRef"]);
    // refusals say why, in a word for programs and a sentence
    let (status, no) =
        post(json!({ "account": new_account, "person": stated("Kim", "Min", "1980-01-01") }));
    assert_eq!(
        (status, no["error"].as_str()),
        (422, Some("account-of-another-person"))
    );
    let (status, no) =
        post(json!({ "account": "0x01", "person": stated("Kim", "Min", "1980-01-01") }));
    assert_eq!((status, no["error"].as_str()), (400, Some("request")));
    let (status, no) = post(json!({ "account": account, "person": { "family_name": "Kim" } }));
    assert_eq!(
        (status, no["message"].as_str()),
        (400, Some("birth_date is needed"))
    );
    let (status, _) = post(json!({ "account": account }));
    assert_eq!(status, 400);
    assert_eq!(app.handle("GET", "/v1/nothing", b"", NOW).0, 404);
    assert_eq!(
        app.handle("POST", "/v1/challenger/start", b"{}", NOW).0,
        404
    );
    // a service without the test check has none
    let real = App::new(service(settings(Basis::NameBirthdate)), false, None);
    let body =
        json!({ "account": account, "person": stated("Kim", "Min", "1980-01-01") }).to_string();
    assert_eq!(
        real.handle("POST", "/v1/test-check", body.as_bytes(), NOW)
            .0,
        404
    );
    assert_eq!(
        real.handle("GET", "/v1/info", b"", NOW).1["checks"],
        json!([])
    );
    let _ = Stated::person;
}

/// A Challenger service, played by the test: one person who proves the
/// postal address given.
fn play_challenger(address: Value) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}", server.server_addr().to_ip().unwrap());
    let handle = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for _ in 0..3 {
            let mut request = server.recv().unwrap();
            let mut body = String::new();
            request.as_reader().read_to_string(&mut body).unwrap();
            let auth = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("Authorization"))
                .map(|h| h.value.to_string())
                .unwrap_or_default();
            let path = request.url().to_owned();
            seen.push(format!("{} {path} [{auth}] {body}", request.method()));
            let answer = match path.as_str() {
                "/setup/7" => json!({ "nonce": "N 1" }),
                "/token" => {
                    json!({ "access_token": "TOKEN", "token_type": "Bearer", "expires_in": 3600 })
                }
                "/info" => {
                    json!({ "id": 1, "address": address, "address_type": "postal", "expires": { "t_s": 1 } })
                }
                _ => json!({}),
            };
            request
                .respond(tiny_http::Response::from_string(answer.to_string()))
                .unwrap();
        }
        seen
    });
    (url, handle)
}

#[test]
fn a_letter_with_a_code_through_challenger() {
    let (url, challenger) = play_challenger(json!({
        "CONTACT_NAME": "Zoë Anna Müller",
        "ADDRESS_COUNTRY": "DE",
        "ADDRESS_ZIPCODE": "79106",
        "ADDRESS_STREET": "Fendrichstraße 12"
    }));
    let config: ChallengerConfig = serde_json::from_value(json!({
        "url": url,
        "client_id": "7",
        "client_secret_file": "unused",
        "redirect_uri": "https://id.example.org/v1/challenger/callback"
    }))
    .unwrap();
    let client = Challenger::new(config, "SECRET".into());
    let app = App::new(service(settings(Basis::NameAddress)), false, Some(client));
    let account = format!("0x{}", hex(&[1; 32]));
    let body = json!({ "account": account }).to_string();

    // the app asks; the person is sent to Challenger
    let (status, start) = app.handle("POST", "/v1/challenger/start", body.as_bytes(), NOW);
    assert_eq!(status, 200, "{start}");
    let link = start["url"].as_str().unwrap();
    let (address, query) = link.split_once('?').unwrap();
    assert_eq!(address, format!("{url}/authorize/N%201"));
    assert!(query.contains("response_type=code&client_id=7"));
    assert!(
        query.contains("redirect_uri=https%3A%2F%2Fid.example.org%2Fv1%2Fchallenger%2Fcallback")
    );
    let state = query.split_once("state=").unwrap().1;

    // a callback nobody asked for, or without its parts
    assert_eq!(
        app.handle(
            "GET",
            "/v1/challenger/callback?code=C&state=other",
            b"",
            NOW
        )
        .0,
        410
    );
    assert_eq!(
        app.handle("GET", "/v1/challenger/callback?code=C", b"", NOW)
            .0,
        400
    );
    // the person typed the code of the letter and comes back
    let back = format!("/v1/challenger/callback?code=C%2B1&state={state}");
    let (status, call) = app.handle("GET", &back, b"", NOW + 600);
    assert_eq!(status, 200, "{call}");
    assert_eq!(call["call"], "credential.present");
    assert_eq!(call["claim"]["subject"], account);
    // once
    assert_eq!(app.handle("GET", &back, b"", NOW + 700).0, 410);

    let seen = challenger.join().unwrap();
    assert_eq!(seen[0], "POST /setup/7 [Bearer SECRET] {}");
    assert!(seen[1].starts_with("POST /token [] "), "{}", seen[1]);
    for part in [
        "grant_type=authorization_code",
        "code=C%2B1",
        "client_id=7",
        "client_secret=SECRET",
    ] {
        assert!(seen[1].contains(part), "{part} in {}", seen[1]);
    }
    assert_eq!(seen[2], "GET /info [Bearer TOKEN] ");
    let _ = SubjectRef([0; 32]);
}
