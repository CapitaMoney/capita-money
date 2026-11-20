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

//! Face dedup (rule 33, ADR 0030): one person with two documents is held
//! until the board decides; twins pass after the board; nothing is kept
//! while the rules switch it off; a forgotten print is gone from the disk.
//!
//! No face of a real person is part of these tests: the face prints are
//! random directions, and "another photo of the same face" is the same
//! direction with a little noise.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use capita_credential::types::{Audience, IssuerId};
use capita_face::embed::{Print, DIM};
use capita_issuer::dedup::{
    Dedup, DedupKey, FaceStore, FileFaceStore, MemoryFaceStore, Outcome, Verdict,
};
use capita_issuer::hex;
use capita_issuer::issue::{Action, Payment, Person, Refusal, Request, Rules, Service, Settings};
use capita_issuer::store::{MemoryStore, Store};
use capita_person_key::canonical::{Basis, Date, Name};
use capita_person_key::LocalKey;

const NOW: u64 = 1_794_614_400; // 2026-11-14
const YEAR: u64 = 365 * 86_400;
const SECRET: [u8; 32] = [9; 32];
const SAME: f32 = 0.6;
const BOARD: &str = "board@example.org";

/// A face print no face made: a random direction, the same for the same
/// seed. Two of them are nearly at right angles (likeness near 0).
fn face(seed: u64) -> Print {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut v = [0f32; DIM];
    for x in &mut v {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *x = (state >> 40) as f32 / (1u64 << 24) as f32 - 0.5;
    }
    unit(v)
}

fn unit(mut v: [f32; DIM]) -> Print {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    for x in &mut v {
        *x /= norm;
    }
    Print(v)
}

/// Another photo of the same face: the same direction, a little noise
/// (likeness about 0.96).
fn again(print: &Print, seed: u64) -> Print {
    let noise = face(seed.wrapping_add(1_000));
    let mut v = print.0;
    for (x, n) in v.iter_mut().zip(noise.0.iter()) {
        *x += 0.3 * n;
    }
    unit(v)
}

fn date(text: &str) -> Date {
    Date::parse(text).unwrap()
}

fn person(family: &str, given: &str, born: &str) -> Person {
    Person::NameBirthdate {
        name: Name {
            family: family.into(),
            given: given.into(),
        },
        born: date(born),
        place_of_birth: None,
    }
}

fn settings(face_dedup: bool, payment: Payment) -> Settings {
    Settings {
        audience: Audience([7; 32]),
        issuer: IssuerId(0),
        rules: Rules {
            basis: Basis::NameBirthdate,
            minimum_age: 0,
            validity: YEAR,
            level: 1,
            face_dedup,
        },
        payment,
        allow_name_change: true,
    }
}

fn dedup(store: Box<dyn FaceStore>) -> Dedup {
    Dedup::new(
        DedupKey::from_secret(&SECRET).unwrap(),
        store,
        SAME,
        BOARD.into(),
    )
    .unwrap()
}

fn bare(face_dedup: bool, payment: Payment) -> Service<LocalKey, MemoryStore> {
    let key = LocalKey::from_secret(&[1; 32]).unwrap();
    Service::new(
        settings(face_dedup, payment),
        key,
        vec![],
        &[2; 32],
        MemoryStore::default(),
    )
}

fn service(face_dedup: bool) -> Service<LocalKey, MemoryStore> {
    bare(face_dedup, Payment::Off).with_dedup(dedup(Box::new(MemoryFaceStore::default())))
}

fn document(account: u8, person: Person, print: Print) -> Request {
    Request {
        account: [account; 32],
        person,
        proof: "document-chip".into(),
        former: None,
        ticket: None,
        helper: None,
        face: Some(print),
    }
}

/// The people the face store keeps in the group of a date of birth.
fn group_of(service: &Service<LocalKey, MemoryStore>, born: &str) -> Vec<String> {
    let key = DedupKey::from_secret(&SECRET).unwrap();
    let dedup = service.dedup().unwrap();
    dedup
        .store()
        .group(&key.group(date(born)).unwrap())
        .unwrap()
        .into_iter()
        .map(|e| e.person)
        .collect()
}

/// The board finds the held case by the data of the document it is shown.
fn case_of(service: &Service<LocalKey, MemoryStore>, person: &Person) -> String {
    let key = hex(&service.person_key_of(person).unwrap());
    let cases = service.dedup().unwrap().store().cases().unwrap();
    cases.into_iter().find(|c| c.person == key).unwrap().id
}

fn decide(service: &mut Service<LocalKey, MemoryStore>, case: &str, verdict: Verdict) {
    let store = service.dedup_mut().unwrap().store_mut();
    assert!(store.decide(case, verdict, NOW).unwrap().is_some());
}

fn held() -> Refusal {
    Refusal::Held {
        contact: BOARD.into(),
    }
}

#[test]
fn the_keys_name_groups_and_seal_prints() {
    let key = DedupKey::from_secret(&SECRET).unwrap();
    let may = key.group(date("1990-05-17")).unwrap();
    assert_eq!(may, key.group(date("1990-05-17")).unwrap());
    assert_ne!(may, key.group(date("1990-05-18")).unwrap());
    assert_eq!(may.len(), 32);
    assert!(!may.contains("1990"));
    let other = DedupKey::from_secret(&[8; 32]).unwrap();
    assert_ne!(may, other.group(date("1990-05-17")).unwrap());
    assert!(DedupKey::from_secret(&[1; 31]).is_err());

    let print = face(1);
    let person = hex(&[3; 32]);
    let sealed = key.seal(&person, &may, &print).unwrap();
    assert_eq!(key.open(&person, &may, &sealed).unwrap(), print);
    // a fresh nonce every time
    assert_ne!(sealed, key.seal(&person, &may, &print).unwrap());
    // moved to another line, another group, changed, another secret: refused
    assert!(key.open(&hex(&[4; 32]), &may, &sealed).is_err());
    let june = key.group(date("1990-06-17")).unwrap();
    assert!(key.open(&person, &june, &sealed).is_err());
    let mut changed = sealed.clone().into_bytes();
    changed[20] = if changed[20] == b'A' { b'B' } else { b'A' };
    assert!(key
        .open(&person, &may, &String::from_utf8(changed).unwrap())
        .is_err());
    assert!(other.open(&person, &may, &sealed).is_err());
    assert!(key.open(&person, &may, "bm90IGEgcHJpbnQ=").is_err());
    // no number of a print in a log
    assert_eq!(format!("{print:?}"), "Print(..)");
}

#[test]
fn the_synthetic_faces_behave_like_faces() {
    let one = face(1);
    assert!(one.cosine(&again(&one, 1)) > 0.9);
    for seed in 2..50 {
        assert!(one.cosine(&face(seed)) < 0.4, "seed {seed}");
    }
}

#[test]
fn off_by_the_rules_nothing_is_kept_and_nobody_is_held() {
    let mut s = service(false);
    let anna = face(1);
    let issued = s
        .issue(
            document(1, person("Kovalenko", "Anna", "1990-05-17"), anna.clone()),
            NOW,
        )
        .unwrap();
    assert_eq!(issued.action, Action::Present);
    assert!(group_of(&s, "1990-05-17").is_empty());
    // the same face under another spelling opens a second account: the gap
    // face dedup closes when it is switched on
    assert!(s
        .issue(
            document(
                2,
                person("Kowalenko", "Anna", "1990-05-17"),
                again(&anna, 1)
            ),
            NOW
        )
        .is_ok());
    assert!(s.dedup().unwrap().store().cases().unwrap().is_empty());
}

#[test]
fn one_person_with_two_documents_waits_for_the_board_and_is_refused() {
    let mut s = service(true);
    let anna = face(1);
    let first = person("Kovalenko", "Anna", "1990-05-17");
    s.issue(document(1, first.clone(), anna.clone()), NOW)
        .unwrap();
    let key = hex(&s.person_key_of(&first).unwrap());
    assert_eq!(group_of(&s, "1990-05-17"), vec![key.clone()]);

    // verified again a year later, a new photo: the print is replaced
    s.issue(document(1, first, again(&anna, 2)), NOW + YEAR)
        .unwrap();
    assert_eq!(group_of(&s, "1990-05-17"), vec![key]);

    // a second document of the same person, the name written otherwise
    let second = person("Kowalenko", "Anna", "1990-05-17");
    assert_eq!(
        s.issue(document(2, second.clone(), again(&anna, 3)), NOW + YEAR),
        Err(held())
    );
    // nothing issued, nothing remembered for that account
    assert!(s.store().by_account(&hex(&[2; 32])).is_none());
    assert_eq!(s.store().count, 2);
    // asking again changes nothing: one case
    assert_eq!(
        s.issue(document(3, second.clone(), again(&anna, 4)), NOW + YEAR),
        Err(held())
    );
    assert_eq!(s.dedup().unwrap().store().cases().unwrap().len(), 1);

    // the board: video call, both documents – one person
    let case = case_of(&s, &second);
    decide(&mut s, &case, Verdict::Same);
    assert!(s.dedup().unwrap().store().cases().unwrap().is_empty());
    assert_eq!(
        s.issue(document(2, second, again(&anna, 5)), NOW + YEAR),
        Err(Refusal::DoubleAccount)
    );
    // the account that exists stays
    assert!(s.store().by_account(&hex(&[1; 32])).is_some());
}

#[test]
fn twins_pass_once_the_board_has_seen_them() {
    let mut s = service(true);
    let mia = face(5);
    s.issue(
        document(1, person("Lee", "Mia", "2001-02-03"), mia.clone()),
        NOW,
    )
    .unwrap();
    let noa = person("Lee", "Noa", "2001-02-03");
    assert_eq!(
        s.issue(document(2, noa.clone(), again(&mia, 6)), NOW),
        Err(held())
    );
    let case = case_of(&s, &noa);
    decide(&mut s, &case, Verdict::Different);
    let issued = s.issue(document(2, noa, again(&mia, 7)), NOW).unwrap();
    assert_eq!(issued.action, Action::Present);
    assert_eq!(group_of(&s, "2001-02-03").len(), 2);
    // and neither twin is held for the other again
    assert!(s
        .issue(
            document(1, person("Lee", "Mia", "2001-02-03"), again(&mia, 8)),
            NOW + YEAR
        )
        .is_ok());
}

#[test]
fn only_the_same_birthday_is_compared() {
    let mut s = service(true);
    let face_one = face(1);
    s.issue(
        document(1, person("Ruiz", "Ana", "1990-05-17"), face_one.clone()),
        NOW,
    )
    .unwrap();
    // the same face, another date of birth: another group, not compared
    assert!(s
        .issue(
            document(2, person("Ruis", "Ana", "1990-05-18"), again(&face_one, 1)),
            NOW
        )
        .is_ok());
    // the same date of birth, another face: no match
    assert!(s
        .issue(
            document(3, person("Diaz", "Eva", "1990-05-17"), face(2)),
            NOW
        )
        .is_ok());
    assert_eq!(group_of(&s, "1990-05-17").len(), 2);
    assert!(s.dedup().unwrap().store().cases().unwrap().is_empty());
}

#[test]
fn a_new_name_takes_the_print_along() {
    let mut s = service(true);
    let eva = face(3);
    let maiden = person("Schmidt", "Eva", "1985-01-01");
    s.issue(document(1, maiden.clone(), eva.clone()), NOW)
        .unwrap();
    let old = hex(&s.person_key_of(&maiden).unwrap());
    let married = person("Weber", "Eva", "1985-01-01");
    let issued = s
        .issue(
            Request {
                former: Some(maiden),
                ..document(1, married.clone(), again(&eva, 1))
            },
            NOW,
        )
        .unwrap();
    assert!(issued.claim.replaces.is_some());
    let new = hex(&s.person_key_of(&married).unwrap());
    assert_ne!(old, new);
    // one line for the person, under the new key; the old one is gone
    assert_eq!(group_of(&s, "1985-01-01"), vec![new]);
}

#[test]
fn a_held_verification_uses_no_ticket() {
    let codes: BTreeSet<String> = ["TICKET-1".to_owned(), "TICKET-2".to_owned()].into();
    let mut s = bare(
        true,
        Payment::Tickets {
            codes,
            doors: vec![],
        },
    )
    .with_dedup(dedup(Box::new(MemoryFaceStore::default())));
    let mia = face(5);
    let paid = |account, person, print| Request {
        ticket: Some("TICKET-1".into()),
        ..document(account, person, print)
    };
    let free = |account, person, print| Request {
        ticket: Some("TICKET-2".into()),
        ..document(account, person, print)
    };
    s.issue(
        free(1, person("Lee", "Mia", "2001-02-03"), mia.clone()),
        NOW,
    )
    .unwrap();
    let noa = person("Lee", "Noa", "2001-02-03");
    assert_eq!(
        s.issue(paid(2, noa.clone(), again(&mia, 1)), NOW),
        Err(held())
    );
    assert!(!s.store().ticket_used("TICKET-1"));
    let case = case_of(&s, &noa);
    decide(&mut s, &case, Verdict::Different);
    // the same ticket still pays
    assert!(s.issue(paid(2, noa, again(&mia, 2)), NOW).is_ok());
    assert!(s.store().ticket_used("TICKET-1"));
}

#[test]
fn switched_on_without_a_face_store_nobody_passes_a_document_check() {
    let mut s = bare(true, Payment::Off);
    assert!(matches!(
        s.issue(
            document(1, person("Lee", "Mia", "2001-02-03"), face(1)),
            NOW
        ),
        Err(Refusal::System(_))
    ));
    // a check without a face print (the test check of test chains) is not
    // compared
    assert!(s
        .issue(
            Request {
                face: None,
                ..document(1, person("Lee", "Mia", "2001-02-03"), face(1))
            },
            NOW
        )
        .is_ok());
}

fn fresh_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Every byte the store keeps, as text.
fn everything(dir: &std::path::Path) -> String {
    let mut all = String::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            for item in std::fs::read_dir(&path).unwrap() {
                stack.push(item.unwrap().path());
            }
        } else {
            all.push_str(&std::fs::read_to_string(&path).unwrap_or_default());
        }
    }
    all
}

#[test]
fn the_file_store_is_shared_with_the_board_and_forgets_for_real() {
    let dir = fresh_dir("face-store-shared");
    let mut service_side = dedup(Box::new(FileFaceStore::new(&dir)));
    // nothing is created before the first print is kept
    assert!(!dir.exists());
    let born = date("1990-05-17");
    let (anna, other) = (hex(&[1; 32]), hex(&[2; 32]));
    assert_eq!(
        service_side.check(&anna, born, &face(1), &[], NOW).unwrap(),
        Outcome::Clear
    );
    service_side.keep(&anna, born, &face(1), None, NOW).unwrap();
    let group = DedupKey::from_secret(&SECRET).unwrap().group(born).unwrap();
    let file = dir.join("groups").join(format!("{group}.jsonl"));
    let sealed = std::fs::read_to_string(&file).unwrap();
    assert!(sealed.contains(&anna));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(&dir.join("groups")), 0o700);
    }

    let Outcome::Held(case) = service_side
        .check(&other, born, &again(&face(1), 1), &[], NOW)
        .unwrap()
    else {
        panic!("not held");
    };
    // the board's command runs as another process on the same folder
    let mut board = FileFaceStore::new(&dir);
    assert_eq!(board.cases().unwrap(), vec![case.clone()]);
    assert_eq!(
        board
            .decide(&case.id, Verdict::Same, NOW)
            .unwrap()
            .unwrap()
            .id,
        case.id
    );
    assert!(board
        .decide(&case.id, Verdict::Same, NOW)
        .unwrap()
        .is_none());
    assert_eq!(
        service_side
            .check(&other, born, &again(&face(1), 2), &[], NOW)
            .unwrap(),
        Outcome::Double
    );

    // the account of `anna` closes: print and verdicts are gone from the disk
    let print_line = sealed.trim().to_owned();
    assert!(board.forget(&anna).unwrap());
    assert!(!board.forget(&anna).unwrap());
    let kept = everything(&dir);
    assert!(!kept.contains(&print_line));
    assert!(!kept.contains(&anna));
    assert!(!file.exists());
    assert_eq!(board.verdict(&anna, &other).unwrap(), None);
}

#[test]
fn a_changed_print_stops_every_check_of_its_group() {
    let dir = fresh_dir("face-store-changed");
    let mut d = dedup(Box::new(FileFaceStore::new(&dir)));
    let born = date("2001-02-03");
    d.keep(&hex(&[1; 32]), born, &face(1), None, NOW).unwrap();
    let group = DedupKey::from_secret(&SECRET).unwrap().group(born).unwrap();
    let file = dir.join("groups").join(format!("{group}.jsonl"));
    let line = std::fs::read_to_string(&file).unwrap();
    // the line of person 1 now claims to be person 3
    std::fs::write(&file, line.replace(&hex(&[1; 32]), &hex(&[3; 32]))).unwrap();
    assert!(d.check(&hex(&[2; 32]), born, &face(2), &[], NOW).is_err());
}

#[test]
fn a_likeness_outside_zero_to_one_or_no_contact_is_refused() {
    let key = || DedupKey::from_secret(&SECRET).unwrap();
    let store = || Box::new(MemoryFaceStore::default());
    assert!(Dedup::new(key(), store(), 0.0, BOARD.into()).is_err());
    assert!(Dedup::new(key(), store(), 1.5, BOARD.into()).is_err());
    assert!(Dedup::new(key(), store(), f32::NAN, BOARD.into()).is_err());
    assert!(Dedup::new(key(), store(), 0.6, " ".into()).is_err());
    assert!(Dedup::new(key(), store(), 0.6, BOARD.into()).is_ok());
}
