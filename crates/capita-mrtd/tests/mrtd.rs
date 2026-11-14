// Capita Money – capita-mrtd
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The chip check against a test chip of the fictional state Utopia
//! (`tests/fixtures/make.sh`) and the zones of ICAO 9303.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]

use capita_mrtd::groups::{face_image, mrz_text, ImageFormat};
use capita_mrtd::mrz::{check_digit, parse, MrzDate};
use capita_mrtd::pa::{passive_authentication, TrustList};
use capita_mrtd::{read_chip, MrtdError};
use openssl::x509::X509;

const SOD: &[u8] = include_bytes!("fixtures/sod.bin");
const DG1: &[u8] = include_bytes!("fixtures/dg1.bin");
const DG2: &[u8] = include_bytes!("fixtures/dg2.bin");
const CSCA: &[u8] = include_bytes!("fixtures/csca.pem");
const OTHER: &[u8] = include_bytes!("fixtures/other-csca.pem");

fn trust(pem: &[u8]) -> TrustList {
    TrustList::new(X509::stack_from_pem(pem).unwrap()).unwrap()
}

#[test]
fn check_digits_of_icao_9303() {
    assert_eq!(check_digit("L898902C3"), Some(6));
    assert_eq!(check_digit("740812"), Some(2));
    assert_eq!(check_digit("120415"), Some(9));
    assert_eq!(check_digit("D23145890"), Some(7));
    assert_eq!(check_digit("<<<<"), Some(0));
}

#[test]
fn a_passport_zone() {
    let zone = "P<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<\nL898902C36UTO7408122F1204159ZE184226B<<<<<10";
    let mrz = parse(zone).unwrap();
    assert_eq!(mrz.document_code, "P");
    assert_eq!(mrz.issuing_state, "UTO");
    assert_eq!(mrz.family_name, "ERIKSSON");
    assert_eq!(mrz.given_names, "ANNA MARIA");
    assert_eq!(mrz.number, "L898902C3");
    assert_eq!(mrz.nationality, "UTO");
    assert_eq!(mrz.birth, MrzDate { yy: 74, mm: 8, dd: 12 });
    assert_eq!(mrz.birth.birth_year(2026), 1974);
    assert_eq!(mrz.sex, 'F');
    assert_eq!(mrz.expiry.expiry_year(), 2012);
}

#[test]
fn an_identity_card_zone() {
    let zone = "I<UTOD231458907<<<<<<<<<<<<<<<7408122F1204159UTO<<<<<<<<<<<6ERIKSSON<<ANNA<MARIA<<<<<<<<<<";
    let mrz = parse(zone).unwrap();
    assert_eq!(mrz.document_code, "I");
    assert_eq!(mrz.number, "D23145890");
    assert_eq!(mrz.family_name, "ERIKSSON");
    assert_eq!(mrz.given_names, "ANNA MARIA");
    assert_eq!(mrz.birth, MrzDate { yy: 74, mm: 8, dd: 12 });
}

#[test]
fn a_changed_zone_fails_its_check_digits() {
    let born_later = "P<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<L898902C36UTO7508122F1204159ZE184226B<<<<<10";
    assert_eq!(parse(born_later), Err(MrtdError::CheckDigit("date of birth")));
    let other_number = "P<UTOERIKSSON<<ANNA<MARIA<<<<<<<<<<<<<<<<<<<L898902C46UTO7408122F1204159ZE184226B<<<<<10";
    assert_eq!(parse(other_number), Err(MrtdError::CheckDigit("document number")));
    assert!(matches!(parse("P<UTO"), Err(MrtdError::Malformed(_))));
}

#[test]
fn birth_years_take_the_latest_century_not_in_the_future() {
    let d = MrzDate { yy: 10, mm: 1, dd: 1 };
    assert_eq!(d.birth_year(2026), 2010);
    let d = MrzDate { yy: 30, mm: 1, dd: 1 };
    assert_eq!(d.birth_year(2026), 1930);
}

#[test]
fn the_test_chip_is_genuine() {
    let chip = read_chip(SOD, DG1, DG2, &trust(CSCA)).unwrap();
    assert_eq!(chip.mrz.family_name, "ERIKSSON");
    assert_eq!(chip.authenticated.algorithm, "SHA-256");
    assert_eq!(chip.authenticated.groups, vec![1, 2]);
    assert_eq!(chip.photo_format, ImageFormat::Jpeg);
    assert_eq!(&chip.photo[..3], &[0xff, 0xd8, 0xff]);
}

#[test]
fn another_country_does_not_vouch_for_it() {
    assert_eq!(
        passive_authentication(SOD, &[(1, DG1)], &trust(OTHER)),
        Err(MrtdError::Untrusted)
    );
    let empty = TrustList::new(Vec::new()).unwrap();
    assert!(empty.is_empty());
    assert_eq!(
        passive_authentication(SOD, &[(1, DG1)], &empty),
        Err(MrtdError::Untrusted)
    );
}

#[test]
fn changed_data_groups_are_found() {
    let mut dg1 = DG1.to_vec();
    let at = dg1.len() - 50;
    dg1[at] = b'X';
    assert_eq!(
        passive_authentication(SOD, &[(1, &dg1), (2, DG2)], &trust(CSCA)),
        Err(MrtdError::HashMismatch(1))
    );
    assert_eq!(
        passive_authentication(SOD, &[(3, DG2)], &trust(CSCA)),
        Err(MrtdError::MissingGroup(3))
    );
}

#[test]
fn a_changed_security_object_is_refused() {
    let mut sod = SOD.to_vec();
    // a byte inside the hash of DG1, in the signed content
    let pos = sod
        .windows(3)
        .position(|w| w == [0x02, 0x01, 0x01])
        .unwrap();
    sod[pos + 10] ^= 1;
    let result = passive_authentication(&sod, &[(1, DG1)], &trust(CSCA));
    assert!(matches!(result, Err(MrtdError::BadSignature)), "{result:?}");
}

#[test]
fn the_zone_and_photo_are_found_in_their_groups() {
    assert_eq!(mrz_text(DG1).unwrap().len(), 88);
    let (format, image) = face_image(DG2).unwrap();
    assert_eq!(format, ImageFormat::Jpeg);
    assert_eq!(image.last(), Some(&0xd9));
    assert!(matches!(face_image(DG1), Err(MrtdError::Malformed(_))));
}

#[test]
fn a_trust_list_is_read_from_a_folder() {
    let dir = std::env::temp_dir().join(format!("capita-mrtd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("csca.pem"), CSCA).unwrap();
    std::fs::write(dir.join("other.pem"), OTHER).unwrap();
    std::fs::write(dir.join("readme.txt"), b"not a certificate").unwrap();
    let list = TrustList::load(&dir).unwrap();
    assert_eq!(list.len(), 2);
    assert!(passive_authentication(SOD, &[(1, DG1)], &list).is_ok());
    std::fs::remove_dir_all(&dir).unwrap();
}
