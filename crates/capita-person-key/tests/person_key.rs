// Capita Money – capita-person-key
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

//! The person key: canonical forms with test vectors, the keyed function,
//! namesakes.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use capita_person_key::canonical::{
    discriminator, document, name_address, name_birthdate, Basis, DataError, Date, Name,
};
use capita_person_key::translit::{fold, transliterate};
use capita_person_key::{namesake_tag, person_key, KeyError, KeyService, LocalKey};

fn name(family: &str, given: &str) -> Name {
    Name {
        family: family.into(),
        given: given.into(),
    }
}

fn born(text: &str) -> Date {
    Date::parse(text).unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn names_are_reduced_to_the_letters_of_a_machine_readable_zone() {
    assert_eq!(
        transliterate("Müller-Lüdenscheidt"),
        "MUELLER LUEDENSCHEIDT"
    );
    assert_eq!(
        transliterate("  d'Été  Øster Ångström"),
        "D ETE OESTER AANGSTROEM"
    );
    assert_eq!(
        transliterate("Łukasz Žižek Þór Straße"),
        "LUKASZ ZIZEK THOR STRASSE"
    );
    assert_eq!(transliterate("ROLF<THILO"), "ROLF THILO");
    assert_eq!(transliterate("李 4"), "");
    // the forms different states print for one letter fall together
    for spelled in ["MUELLER", "MULLER", "MUXXLLER", "MUELER"] {
        assert_eq!(fold(spelled), "MULER");
    }
    assert_eq!(fold("NUXXNXXEZ"), fold("NUNEZ"));
    assert_eq!(fold("AANGSTROEM"), "ANGSTROM");
    assert_eq!(fold("STRASSE"), "STRASE");
    assert_eq!(fold("THOR"), "TOR");
}

#[test]
fn the_canonical_form_of_name_and_birthday_is_fixed() {
    // A test vector for identity services written in other languages.
    assert_eq!(Date::parse("1966-02-29"), Err(DataError::InvalidDate));
    let form = name_birthdate(
        &name("Schlüter-Åberg", "Rolf-Thilo Maria"),
        born("1968-02-29"),
    );
    let mut expected = b"capita-person-data-v1\0".to_vec();
    expected.push(1);
    expected.extend_from_slice(b"SCHLUTERABERG\0ROLF\x0019680229");
    assert_eq!(form.unwrap(), expected);
}

#[test]
fn one_person_has_one_form_whatever_the_source_spells() {
    let day = born("1990-07-04");
    let reference = name_birthdate(&name("Müller", "Zoë Anna"), day).unwrap();
    for (family, given) in [
        ("MUELLER", "ZOE ANNA"),      // passport, German transliteration
        ("MULLER", "ZOE"),            // another state: U for Ü, one given name
        ("müller", "Zoë-Anna"),       // typed by hand
        ("MUXXLLER", "ZOE<ANNA"),     // the third form ICAO allows
        ("Mueller ", " Zoe  Anna B"), // spaces, a further given name
    ] {
        assert_eq!(
            name_birthdate(&name(family, given), day).unwrap(),
            reference
        );
    }
    assert_eq!(
        name_birthdate(&Name::from_mrz("MUELLER<<ZOE<ANNA<<<<<<<<<<"), day).unwrap(),
        reference
    );
    // another person: another form
    for (family, given, date) in [
        ("Möller", "Zoë", "1990-07-04"),
        ("Müller", "Anna", "1990-07-04"),
        ("Müller", "Zoë", "1990-07-05"),
    ] {
        assert_ne!(
            name_birthdate(&name(family, given), born(date)).unwrap(),
            reference
        );
    }
    // long names are cut where every document still prints them
    let long = name(
        "Wolfeschlegelsteinhausenbergerdorff",
        "Maximiliane-Alexandra",
    );
    let form = name_birthdate(&long, day).unwrap();
    let cut = name("Wolfeschlegelsteinhausenbe", "Maximiliane-A");
    assert_eq!(name_birthdate(&cut, day).unwrap(), form);
    // nothing to hash
    assert_eq!(
        name_birthdate(&name("李", "Zoë"), day),
        Err(DataError::NoFamilyName)
    );
    assert_eq!(
        name_birthdate(&name("Müller", "-"), day),
        Err(DataError::NoGivenName)
    );
}

#[test]
fn the_other_bases_have_forms_of_their_own() {
    let n = name("Müller", "Zoë");
    let a = name_address(&n, "de", "79106", "Fendrichstraße 12 a").unwrap();
    let mut expected = b"capita-person-data-v1\0".to_vec();
    expected.push(2);
    expected.extend_from_slice(b"MULER\0ZO\0DE\x0079106\0FENDRICHSTR12A");
    assert_eq!(a, expected);
    assert_eq!(
        name_address(&n, "DE", "79 106", "Fendrichstr. 12a").unwrap(),
        a
    );
    assert_ne!(
        name_address(&n, "DE", "79106", "Fendrichstr. 14").unwrap(),
        a
    );
    assert_eq!(
        name_address(&n, "DEU", "79106", "x 1"),
        Err(DataError::InvalidCountry)
    );
    assert_eq!(
        name_address(&n, "DE", " ", "x 1"),
        Err(DataError::Empty("postcode"))
    );

    let d = document("D", "p", "c01x00t47").unwrap_err();
    assert_eq!(d, DataError::InvalidCountry);
    let d = document("deu", "p", "C01X-00T47").unwrap();
    let mut expected = b"capita-person-data-v1\0".to_vec();
    expected.push(3);
    expected.extend_from_slice(b"DEU\0P\0C01X00T47");
    assert_eq!(d, expected);

    for basis in [Basis::NameBirthdate, Basis::NameAddress, Basis::Document] {
        assert_eq!(Basis::from_word(basis.as_str()), Some(basis));
    }
    assert_eq!(Basis::from_word("e-mail"), None);
}

#[test]
fn dates_and_ages() {
    assert_eq!(Date::parse("1966-11-31"), Err(DataError::InvalidDate));
    assert_eq!(Date::parse("66-11-30"), Err(DataError::InvalidDate));
    assert_eq!(Date::parse("1966-11-30-1"), Err(DataError::InvalidDate));
    assert!(Date::parse("2000-02-29").is_ok() && Date::parse("1900-02-29").is_err());
    let b = born("2008-11-14");
    assert_eq!(b.years_until(born("2026-11-13")), 17);
    assert_eq!(b.years_until(born("2026-11-14")), 18);
    assert_eq!(b.years_until(born("2001-01-01")), 0);
    // days since 1970-01-01
    assert_eq!(Date::from_days_since_epoch(0), Some(born("1970-01-01")));
    assert_eq!(
        Date::from_days_since_epoch(20_771),
        Some(born("2026-11-14"))
    );
    assert_eq!(
        Date::from_days_since_epoch(11_016),
        Some(born("2000-02-29"))
    );
}

#[test]
fn the_person_key_needs_the_secret_and_is_fixed_by_it() {
    let service = LocalKey::from_secret(&[7; 32]).unwrap();
    let data = name_birthdate(&name("Müller", "Zoë"), born("1990-07-04")).unwrap();
    let key = person_key(&service, &data, 0).unwrap();
    // the same data, the same key – at every service that holds the secret
    let again = LocalKey::from_secret(&[7; 32]).unwrap();
    assert_eq!(person_key(&again, &data, 0).unwrap(), key);
    // A test vector: RFC 9497, OPRF(ristretto255, SHA-512), key derived
    // from the secret with the info "capita-person-key-v1"; input =
    // canonical form, 0x00, counter; the first 32 bytes of the output.
    assert_eq!(hex(&key), VECTOR);
    // another secret, another person, a namesake: other keys
    let other = LocalKey::from_secret(&[8; 32]).unwrap();
    assert_ne!(person_key(&other, &data, 0).unwrap(), key);
    let twin = name_birthdate(&name("Müller", "Zoé"), born("1990-07-05")).unwrap();
    assert_ne!(person_key(&service, &twin, 0).unwrap(), key);
    assert_ne!(person_key(&service, &data, 1).unwrap(), key);
    // a secret must be long enough to be one
    assert_eq!(
        LocalKey::from_secret(&[7; 31]).unwrap_err(),
        KeyError::Secret
    );
    assert_eq!(format!("{service:?}"), "LocalKey(secret)");
}

#[test]
fn namesakes_are_told_apart_by_a_further_fact() {
    let service = LocalKey::from_secret(&[7; 32]).unwrap();
    let data = name_birthdate(&name("Kim", "Min"), born("1990-07-04")).unwrap();
    let seoul = namesake_tag(&service, &data, &discriminator("Seoul")).unwrap();
    assert_eq!(
        namesake_tag(&service, &data, &discriminator(" SEOUL ")).unwrap(),
        seoul
    );
    assert_ne!(
        namesake_tag(&service, &data, &discriminator("Busan")).unwrap(),
        seoul
    );
    // the tag is no person key, whatever the counter
    for counter in 0..4 {
        assert_ne!(person_key(&service, &data, counter).unwrap(), seoul);
    }
}

/// A key service that is not this program: the function is the same.
struct Elsewhere(LocalKey);

impl KeyService for Elsewhere {
    fn evaluate(&self, input: &[u8]) -> Result<[u8; 64], KeyError> {
        self.0.evaluate(input)
    }
}

#[test]
fn where_the_secret_is_held_does_not_change_a_key() {
    let data = document("DEU", "P", "C01X00T47").unwrap();
    let here = LocalKey::from_secret(&[9; 32]).unwrap();
    let there = Elsewhere(LocalKey::from_secret(&[9; 32]).unwrap());
    assert_eq!(person_key(&here, &data, 0), person_key(&there, &data, 0));
}

const VECTOR: &str = "ab957fab7c21879c3d7d7929688cb0b238c6c3a79f8825bdea3836834c0e5933";
