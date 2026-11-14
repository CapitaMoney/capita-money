// Capita Money – capita-mrtd
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The machine-readable zone (ICAO 9303 parts 4–6): TD1 (identity cards,
//! 3 × 30), TD2 (2 × 36), TD3 (passports, 2 × 44), with check digits.

use crate::MrtdError;

/// A date of the zone: two-digit year, month, day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MrzDate {
    /// Year within its century.
    pub yy: u8,
    /// Month, 1–12.
    pub mm: u8,
    /// Day, 1–31.
    pub dd: u8,
}

impl MrzDate {
    /// Full year of a date of birth: the latest century that does not put
    /// it after `this_year`.
    #[must_use]
    pub fn birth_year(&self, this_year: u16) -> u16 {
        let modern = 2000u16.saturating_add(u16::from(self.yy));
        if modern <= this_year {
            modern
        } else {
            1900u16.saturating_add(u16::from(self.yy))
        }
    }

    /// Full year of an expiry date (documents expire in this century).
    #[must_use]
    pub fn expiry_year(&self) -> u16 {
        2000u16.saturating_add(u16::from(self.yy))
    }
}

/// What the zone says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mrz {
    /// Kind of document: `P` passport, `I`, `A`, `C` identity cards …
    pub document_code: String,
    /// Issuing state or organisation, three letters.
    pub issuing_state: String,
    /// Primary identifier: the family name, in A–Z and spaces.
    pub family_name: String,
    /// Secondary identifier: the given names, in A–Z and spaces.
    pub given_names: String,
    /// Document number, without filler.
    pub number: String,
    /// Nationality, three letters.
    pub nationality: String,
    /// Date of birth.
    pub birth: MrzDate,
    /// `M`, `F` or `<` (unspecified).
    pub sex: char,
    /// Date of expiry.
    pub expiry: MrzDate,
}

/// The check digit of a field: weights 7, 3, 1; `<` = 0, `0`–`9`, `A`–`Z` =
/// 10–35.
#[must_use]
pub fn check_digit(field: &str) -> Option<u8> {
    let mut sum: u32 = 0;
    for (c, w) in field.chars().zip([7u32, 3, 1].iter().cycle()) {
        let v = if c == '<' { 0 } else { c.to_digit(36)? };
        sum = sum.wrapping_add(v.wrapping_mul(*w));
    }
    u8::try_from(sum.checked_rem(10)?).ok()
}

fn part(line: &str, from: usize, to: usize) -> Result<&str, MrtdError> {
    line.get(from..to).ok_or(MrtdError::Malformed("zone too short"))
}

fn checked(field: &str, digit: &str, name: &'static str) -> Result<(), MrtdError> {
    let want = digit.chars().next().and_then(|c| c.to_digit(10));
    match (check_digit(field), want) {
        (Some(got), Some(want)) if u32::from(got) == want => Ok(()),
        _ => Err(MrtdError::CheckDigit(name)),
    }
}

fn date(field: &str) -> Result<MrzDate, MrtdError> {
    let digits: Vec<u8> = field
        .chars()
        .map(|c| c.to_digit(10).and_then(|d| u8::try_from(d).ok()))
        .collect::<Option<Vec<u8>>>()
        .ok_or(MrtdError::Malformed("date"))?;
    let [y1, y2, m1, m2, d1, d2] = digits.as_slice() else {
        return Err(MrtdError::Malformed("date"));
    };
    let two = |a: u8, b: u8| a.saturating_mul(10).saturating_add(b);
    let out = MrzDate {
        yy: two(*y1, *y2),
        mm: two(*m1, *m2),
        dd: two(*d1, *d2),
    };
    if !(1..=12).contains(&out.mm) || !(1..=31).contains(&out.dd) {
        return Err(MrtdError::Malformed("date"));
    }
    Ok(out)
}

fn plain(field: &str) -> String {
    field.replace('<', " ").trim().to_string()
}

fn names(field: &str) -> (String, String) {
    match field.split_once("<<") {
        Some((family, given)) => (plain(family), plain(given)),
        None => (plain(field), String::new()),
    }
}

fn first_char(field: &str) -> char {
    field.chars().next().unwrap_or('<')
}

/// Reads a zone of 88 (TD3), 72 (TD2) or 90 (TD1) characters, line breaks
/// removed, and checks every check digit.
///
/// # Errors
/// [`MrtdError::Malformed`] for a zone of another form,
/// [`MrtdError::CheckDigit`] for a wrong check digit.
pub fn parse(zone: &str) -> Result<Mrz, MrtdError> {
    let zone: String = zone.chars().filter(|c| !c.is_whitespace()).collect();
    if !zone.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '<') {
        return Err(MrtdError::Malformed("zone characters"));
    }
    match zone.len() {
        88 => two_lines(&zone, 44),
        72 => two_lines(&zone, 36),
        90 => td1(&zone),
        _ => Err(MrtdError::Malformed("zone length")),
    }
}

/// TD3 and TD2: line 1 = code, state, name; line 2 = number, nationality,
/// birth, sex, expiry, optional data, composite.
fn two_lines(zone: &str, width: usize) -> Result<Mrz, MrtdError> {
    let (one, two) = zone.split_at_checked(width).ok_or(MrtdError::Malformed("zone"))?;
    let last = width.saturating_sub(1);
    let optional_end = if width == 44 { 42 } else { 35 };
    let number = part(two, 0, 9)?;
    checked(number, part(two, 9, 10)?, "document number")?;
    checked(part(two, 13, 19)?, part(two, 19, 20)?, "date of birth")?;
    checked(part(two, 21, 27)?, part(two, 27, 28)?, "date of expiry")?;
    if width == 44 {
        let optional = part(two, 28, 42)?;
        let digit = part(two, 42, 43)?;
        if !(optional.chars().all(|c| c == '<') && digit == "<") {
            checked(optional, digit, "personal number")?;
        }
    }
    let composite = [
        part(two, 0, 10)?,
        part(two, 13, 20)?,
        part(two, 21, optional_end.saturating_add(if width == 44 { 1 } else { 0 }))?,
    ]
    .concat();
    checked(&composite, part(two, last, width)?, "composite")?;
    let (family_name, given_names) = names(part(one, 5, width)?);
    Ok(Mrz {
        document_code: plain(part(one, 0, 2)?),
        issuing_state: plain(part(one, 2, 5)?),
        family_name,
        given_names,
        number: plain(number),
        nationality: plain(part(two, 10, 13)?),
        birth: date(part(two, 13, 19)?)?,
        sex: first_char(part(two, 20, 21)?),
        expiry: date(part(two, 21, 27)?)?,
    })
}

/// TD1: line 1 = code, state, number, optional; line 2 = birth, sex,
/// expiry, nationality, optional, composite; line 3 = name.
fn td1(zone: &str) -> Result<Mrz, MrtdError> {
    let one = part(zone, 0, 30)?;
    let two = part(zone, 30, 60)?;
    let three = part(zone, 60, 90)?;
    let mut number = part(one, 5, 14)?.to_string();
    let digit = part(one, 14, 15)?;
    if digit == "<" {
        // A number longer than nine characters goes on in the optional
        // field; its check digit is the last character before the filler.
        let optional = part(one, 15, 30)?;
        let rest = optional.split('<').next().unwrap_or_default();
        let (more, check) = rest
            .split_at_checked(rest.len().saturating_sub(1))
            .ok_or(MrtdError::Malformed("document number"))?;
        number.push_str(more);
        checked(&number, check, "document number")?;
    } else {
        checked(&number, digit, "document number")?;
    }
    checked(part(two, 0, 6)?, part(two, 6, 7)?, "date of birth")?;
    checked(part(two, 8, 14)?, part(two, 14, 15)?, "date of expiry")?;
    let composite = [part(one, 5, 30)?, part(two, 0, 7)?, part(two, 8, 15)?, part(two, 18, 29)?].concat();
    checked(&composite, part(two, 29, 30)?, "composite")?;
    let (family_name, given_names) = names(three);
    Ok(Mrz {
        document_code: plain(part(one, 0, 2)?),
        issuing_state: plain(part(one, 2, 5)?),
        family_name,
        given_names,
        number: plain(&number),
        nationality: plain(part(two, 15, 18)?),
        birth: date(part(two, 0, 6)?)?,
        sex: first_char(part(two, 7, 8)?),
        expiry: date(part(two, 8, 14)?)?,
    })
}
