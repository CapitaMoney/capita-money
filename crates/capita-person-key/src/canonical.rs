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

//! The canonical form: what was checked about a person, as one fixed
//! sequence of bytes.
//!
//! What a person key is made from is a rule (`identity.person_key_basis` of
//! the rules file). Every basis has its own form and its own first byte,
//! so that keys of different bases can never coincide:
//!
//! ```text
//! all             "capita-person-data-v1"  0x00
//! name-birthdate  0x01  FAMILY 0x00 GIVEN 0x00 YYYYMMDD
//! name-address    0x02  FAMILY 0x00 GIVEN 0x00 CC 0x00 POSTCODE 0x00 STREET
//! document        0x03  STATE 0x00 KIND 0x00 NUMBER
//! ```
//!
//! - `FAMILY`: every word of the family name, transliterated and folded
//!   ([`crate::translit`]), joined without spaces, at most
//!   [`MAX_FAMILY`] letters. `GIVEN`: the first word of the given names,
//!   the same way, at most [`MAX_GIVEN`] letters. Cutting keeps the form
//!   equal across documents with name fields of different length; further
//!   given names differ too often between documents to be used.
//! - `YYYYMMDD`: the date of birth in digits.
//! - `CC`: country of the address, two letters. `POSTCODE`, `STREET`,
//!   `NUMBER`: letters and digits only, upper case; `STREET` is the line
//!   with street and house number, words folded, `STRASSE` written `STR`.
//! - `STATE`: issuing state as the document prints it, three letters;
//!   `KIND`: its kind, as the first field of the machine-readable zone
//!   (`P`, `ID` …).
//!
//! Vermutung (Claude, ungeprüft): 24 and 12 letters are no longer than any
//! name field of an identity document leaves uncut – check against real
//! documents before real persons are verified.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::translit::{fold, transliterate};

/// First bytes of every canonical form.
pub const TAG: &[u8] = b"capita-person-data-v1\0";
/// Most letters of a family name that count.
pub const MAX_FAMILY: usize = 24;
/// Most letters of the first given name that count.
pub const MAX_GIVEN: usize = 12;

/// What a person key is made from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Basis {
    /// Name and date of birth, as an identity document states them.
    NameBirthdate,
    /// Name and postal address, proven by a letter with a code.
    NameAddress,
    /// Issuing state, kind and number of an identity document.
    Document,
}

impl Basis {
    /// The byte that marks the basis in a canonical form.
    #[must_use]
    pub const fn byte(self) -> u8 {
        match self {
            Self::NameBirthdate => 1,
            Self::NameAddress => 2,
            Self::Document => 3,
        }
    }

    /// The word a rules file uses for the basis.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NameBirthdate => "name-birthdate",
            Self::NameAddress => "name-address",
            Self::Document => "document",
        }
    }

    /// The basis a rules file names, if it is one.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        [Self::NameBirthdate, Self::NameAddress, Self::Document]
            .into_iter()
            .find(|basis| basis.as_str() == word)
    }
}

/// Why data about a person has no canonical form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataError {
    /// No letter A–Z is left of the family name.
    NoFamilyName,
    /// No letter A–Z is left of the given name.
    NoGivenName,
    /// Not a date of the calendar.
    InvalidDate,
    /// Not a country code of two letters, or an issuing state of three.
    InvalidCountry,
    /// A field that must not be empty is.
    Empty(&'static str),
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFamilyName => f.write_str("family name without letters"),
            Self::NoGivenName => f.write_str("given name without letters"),
            Self::InvalidDate => f.write_str("not a date"),
            Self::InvalidCountry => f.write_str("not a country code"),
            Self::Empty(what) => write!(f, "{what} is empty"),
        }
    }
}

impl core::error::Error for DataError {}

/// A day of the calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

impl Date {
    /// A date, if the calendar has it (years 1 to 9999).
    pub const fn new(year: u16, month: u8, day: u8) -> Result<Self, DataError> {
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return Err(DataError::InvalidDate),
        };
        if year == 0 || year > 9999 || day == 0 || day > days {
            return Err(DataError::InvalidDate);
        }
        Ok(Self { year, month, day })
    }

    /// Reads `YYYY-MM-DD`.
    pub fn parse(text: &str) -> Result<Self, DataError> {
        let mut parts = text.trim().split('-');
        let mut number = |digits: usize| {
            parts
                .next()
                .filter(|part| part.len() == digits && part.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|part| part.parse::<u16>().ok())
                .ok_or(DataError::InvalidDate)
        };
        let (year, month, day) = (number(4)?, number(2)?, number(2)?);
        if parts.next().is_some() {
            return Err(DataError::InvalidDate);
        }
        Self::new(
            year,
            u8::try_from(month).map_err(|_| DataError::InvalidDate)?,
            u8::try_from(day).map_err(|_| DataError::InvalidDate)?,
        )
    }

    /// The year.
    #[must_use]
    pub const fn year(self) -> u16 {
        self.year
    }

    /// Whole years from this date to `today`: the age of a person born on
    /// this date. Zero if `today` is earlier.
    #[must_use]
    pub fn years_until(self, today: Self) -> u16 {
        let years = today.year.saturating_sub(self.year);
        if (today.month, today.day) < (self.month, self.day) {
            years.saturating_sub(1)
        } else {
            years
        }
    }

    /// The date of the day that begins `days` days after 1 January 1970.
    #[must_use]
    pub fn from_days_since_epoch(days: u64) -> Option<Self> {
        // Civil-from-days (H. Hinnant), for days at or after the epoch.
        let z = days.checked_add(719_468)?;
        let era = z / 146_097;
        let doe = z % 146_097;
        let yoe = (doe
            .checked_sub(doe / 1_460)?
            .checked_add(doe / 36_524)?
            .checked_sub(doe / 146_096)?)
            / 365;
        let doy = doe.checked_sub(
            (365u64.checked_mul(yoe)?)
                .checked_add(yoe / 4)?
                .checked_sub(yoe / 100)?,
        )?;
        let mp = (5u64.checked_mul(doy)?.checked_add(2)?) / 153;
        let day = doy
            .checked_sub((153u64.checked_mul(mp)?.checked_add(2)?) / 5)?
            .checked_add(1)?;
        let month = if mp < 10 {
            mp.checked_add(3)?
        } else {
            mp.checked_sub(9)?
        };
        let year = yoe
            .checked_add(era.checked_mul(400)?)?
            .checked_add(u64::from(month <= 2))?;
        Self::new(
            u16::try_from(year).ok()?,
            u8::try_from(month).ok()?,
            u8::try_from(day).ok()?,
        )
        .ok()
    }

    fn digits(self) -> String {
        alloc::format!("{:04}{:02}{:02}", self.year, self.month, self.day)
    }
}

/// The name of a person as a document states it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    /// Family name (primary identifier of a document).
    pub family: String,
    /// Given names (secondary identifier of a document).
    pub given: String,
}

impl Name {
    /// The name field of a machine-readable zone:
    /// `MUELLER<<ROLF<THILO<<<<<<`.
    #[must_use]
    pub fn from_mrz(field: &str) -> Self {
        let (family, given) = field.split_once("<<").unwrap_or((field, ""));
        Self {
            family: family.replace('<', " "),
            given: given.replace('<', " "),
        }
    }

    fn family_form(&self) -> Result<String, DataError> {
        let joined: String = transliterate(&self.family).split(' ').map(fold).collect();
        let cut: String = joined.chars().take(MAX_FAMILY).collect();
        if cut.is_empty() {
            Err(DataError::NoFamilyName)
        } else {
            Ok(cut)
        }
    }

    fn given_form(&self) -> Result<String, DataError> {
        let first = transliterate(&self.given)
            .split(' ')
            .map(fold)
            .find(|word| !word.is_empty())
            .ok_or(DataError::NoGivenName)?;
        Ok(first.chars().take(MAX_GIVEN).collect())
    }

    fn write(&self, out: &mut Vec<u8>) -> Result<(), DataError> {
        out.extend_from_slice(self.family_form()?.as_bytes());
        out.push(0);
        out.extend_from_slice(self.given_form()?.as_bytes());
        out.push(0);
        Ok(())
    }
}

/// Letters and digits of `text`, letters transliterated to upper-case A–Z.
fn plain(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            out.push(c);
        } else if let Some(letters) = crate::translit::letters(c) {
            out.push_str(letters);
        }
    }
    out
}

fn start(basis: Basis) -> Vec<u8> {
    let mut out = TAG.to_vec();
    out.push(basis.byte());
    out
}

fn letters_only(text: &str, count: usize) -> Result<String, DataError> {
    let code = plain(text);
    if code.len() == count && code.bytes().all(|b| b.is_ascii_uppercase()) {
        Ok(code)
    } else {
        Err(DataError::InvalidCountry)
    }
}

/// Canonical form of name and date of birth.
pub fn name_birthdate(name: &Name, born: Date) -> Result<Vec<u8>, DataError> {
    let mut out = start(Basis::NameBirthdate);
    name.write(&mut out)?;
    out.extend_from_slice(born.digits().as_bytes());
    Ok(out)
}

/// Canonical form of name and postal address: country (two letters),
/// postcode, and the line with street and house number.
pub fn name_address(
    name: &Name,
    country: &str,
    postcode: &str,
    street: &str,
) -> Result<Vec<u8>, DataError> {
    let mut out = start(Basis::NameAddress);
    name.write(&mut out)?;
    out.extend_from_slice(letters_only(country, 2)?.as_bytes());
    out.push(0);
    let postcode = plain(postcode);
    if postcode.is_empty() {
        return Err(DataError::Empty("postcode"));
    }
    out.extend_from_slice(postcode.as_bytes());
    out.push(0);
    // Words folded like names; digits kept; the commonest abbreviation of
    // German addresses made one.
    let mut line = String::new();
    for word in street.split(|c: char| !c.is_alphanumeric()) {
        let word = plain(word);
        if word.bytes().any(|b| b.is_ascii_digit()) {
            line.push_str(&word);
        } else {
            line.push_str(&fold(&word.replace("STRASSE", "STR")));
        }
    }
    if line.is_empty() {
        return Err(DataError::Empty("street"));
    }
    out.extend_from_slice(line.as_bytes());
    Ok(out)
}

/// Canonical form of an identity document: issuing state (three letters,
/// as the document prints it), kind (`P`, `ID` …) and number.
pub fn document(state: &str, kind: &str, number: &str) -> Result<Vec<u8>, DataError> {
    let mut out = start(Basis::Document);
    out.extend_from_slice(letters_only(state, 3)?.as_bytes());
    out.push(0);
    let kind = plain(kind);
    if kind.is_empty() {
        return Err(DataError::Empty("kind of document"));
    }
    out.extend_from_slice(kind.as_bytes());
    out.push(0);
    let number = plain(number);
    if number.is_empty() {
        return Err(DataError::Empty("document number"));
    }
    out.extend_from_slice(number.as_bytes());
    Ok(out)
}

/// A place of birth, or another word that tells two people of the same
/// name and birthday apart, in the form it is hashed in.
#[must_use]
pub fn discriminator(text: &str) -> String {
    transliterate(text).split(' ').map(fold).collect()
}
