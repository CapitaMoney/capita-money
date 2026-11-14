// Capita Money – capita-issuer
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The document check's reading of a chip: name and date of birth, and the
//! refusal of an expired document. Test chip of the fictional state Utopia
//! (`crates/capita-mrtd/tests/fixtures`), valid until 2012-04-15.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use capita_issuer::document::{person_of, DocumentRefusal};
use capita_issuer::issue::Person;
use capita_mrtd::pa::TrustList;
use capita_mrtd::read_chip;
use capita_person_key::canonical::Date;

const SOD: &[u8] = include_bytes!("../../../crates/capita-mrtd/tests/fixtures/sod.bin");
const DG1: &[u8] = include_bytes!("../../../crates/capita-mrtd/tests/fixtures/dg1.bin");
const DG2: &[u8] = include_bytes!("../../../crates/capita-mrtd/tests/fixtures/dg2.bin");

/// 2011-06-01 and 2026-11-15, seconds since 1970.
const IN_2011: u64 = 1_306_886_400;
const IN_2026: u64 = 1_794_700_800;

fn chip() -> capita_mrtd::Chip {
    let trust = TrustList::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/capita-mrtd/tests/fixtures/csca.pem")).unwrap();
    read_chip(SOD, DG1, DG2, &trust).unwrap()
}

#[test]
fn a_valid_document_names_its_holder() {
    let Person::NameBirthdate { name, born, place_of_birth } = person_of(&chip(), IN_2011).unwrap() else {
        panic!("name and date of birth");
    };
    assert_eq!(name.family, "ERIKSSON");
    assert_eq!(name.given, "ANNA MARIA");
    assert_eq!(born, Date::new(1974, 8, 12).unwrap());
    assert_eq!(place_of_birth, None);
}

#[test]
fn an_expired_document_is_refused() {
    assert_eq!(person_of(&chip(), IN_2026), Err(DocumentRefusal::Expired));
}
