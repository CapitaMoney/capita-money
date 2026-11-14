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

//! Letters of names, reduced to A–Z.
//!
//! Identity documents print a name twice: as the holder writes it, and in a
//! machine-readable zone that knows the letters A to Z only (ICAO Doc 9303,
//! part 3, section 6). States transliterate differently – `Ü` becomes `UE`
//! in a German passport and `U` in a Turkish one – and a person with two
//! passports may be spelled both ways. [`fold`] therefore goes one step
//! further than the standard and maps every such variant to the same
//! letters. The result is for hashing only: it is no way to write a name.

use alloc::string::String;

/// The letters A–Z a character stands for in a machine-readable zone, in
/// the longer of the forms ICAO allows (`Ü` → `UE`). Characters without
/// letters (space, hyphen, apostrophe, the filler `<`, digits, other
/// scripts) give `None`.
#[must_use]
pub const fn letters(c: char) -> Option<&'static str> {
    Some(match c {
        'A' | 'a' => "A",
        'B' | 'b' => "B",
        'C' | 'c' => "C",
        'D' | 'd' => "D",
        'E' | 'e' => "E",
        'F' | 'f' => "F",
        'G' | 'g' => "G",
        'H' | 'h' => "H",
        'I' | 'i' => "I",
        'J' | 'j' => "J",
        'K' | 'k' => "K",
        'L' | 'l' => "L",
        'M' | 'm' => "M",
        'N' | 'n' => "N",
        'O' | 'o' => "O",
        'P' | 'p' => "P",
        'Q' | 'q' => "Q",
        'R' | 'r' => "R",
        'S' | 's' => "S",
        'T' | 't' => "T",
        'U' | 'u' => "U",
        'V' | 'v' => "V",
        'W' | 'w' => "W",
        'X' | 'x' => "X",
        'Y' | 'y' => "Y",
        'Z' | 'z' => "Z",
        'Á' | 'À' | 'Â' | 'Ã' | 'Ă' | 'Ā' | 'Ą' | 'á' | 'à' | 'â' | 'ã' | 'ă' | 'ā' | 'ą' => {
            "A"
        }
        'Ä' | 'ä' | 'Æ' | 'æ' => "AE",
        'Å' | 'å' => "AA",
        'Ç' | 'Ć' | 'Ĉ' | 'Č' | 'Ċ' | 'ç' | 'ć' | 'ĉ' | 'č' | 'ċ' => "C",
        'Ð' | 'Ď' | 'Đ' | 'ð' | 'ď' | 'đ' => "D",
        'É' | 'È' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ę' | 'Ě' | 'Ė' | 'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ĕ'
        | 'ę' | 'ě' | 'ė' => "E",
        'Ĝ' | 'Ğ' | 'Ġ' | 'Ģ' | 'ĝ' | 'ğ' | 'ġ' | 'ģ' => "G",
        'Ĥ' | 'Ħ' | 'ĥ' | 'ħ' => "H",
        'Í' | 'Ì' | 'Î' | 'Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' | 'İ' | 'í' | 'ì' | 'î' | 'ï' | 'ĩ' | 'ī'
        | 'ĭ' | 'į' | 'ı' => "I",
        'Ĳ' | 'ĳ' => "IJ",
        'Ĵ' | 'ĵ' => "J",
        'Ķ' | 'ķ' => "K",
        'Ĺ' | 'Ļ' | 'Ľ' | 'Ŀ' | 'Ł' | 'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "L",
        'Ñ' | 'Ń' | 'Ņ' | 'Ň' | 'Ŋ' | 'ñ' | 'ń' | 'ņ' | 'ň' | 'ŋ' => "N",
        'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ō' | 'Ŏ' | 'Ő' | 'ó' | 'ò' | 'ô' | 'õ' | 'ō' | 'ŏ' | 'ő' => {
            "O"
        }
        'Ö' | 'ö' | 'Ø' | 'ø' | 'Œ' | 'œ' => "OE",
        'Ŕ' | 'Ŗ' | 'Ř' | 'ŕ' | 'ŗ' | 'ř' => "R",
        'Ś' | 'Ŝ' | 'Š' | 'Ş' | 'Ș' | 'ś' | 'ŝ' | 'š' | 'ş' | 'ș' => "S",
        'ß' | 'ẞ' => "SS",
        'Ţ' | 'Ť' | 'Ŧ' | 'Ț' | 'ţ' | 'ť' | 'ŧ' | 'ț' => "T",
        'Þ' | 'þ' => "TH",
        'Ú' | 'Ù' | 'Û' | 'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' | 'ú' | 'ù' | 'û' | 'ũ' | 'ū' | 'ŭ'
        | 'ů' | 'ű' | 'ų' => "U",
        'Ü' | 'ü' => "UE",
        'Ŵ' | 'ŵ' => "W",
        'Ý' | 'Ŷ' | 'Ÿ' | 'ý' | 'ŷ' | 'ÿ' => "Y",
        'Ź' | 'Ż' | 'Ž' | 'ź' | 'ż' | 'ž' => "Z",
        _ => return None,
    })
}

/// A name as a machine-readable zone prints it, in the longer of the
/// allowed forms: A–Z, with one space wherever the source has anything
/// else between letters. `Müller-Lüdenscheidt` → `MUELLER LUEDENSCHEIDT`.
#[must_use]
pub fn transliterate(text: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in text.chars() {
        match letters(c) {
            Some(s) => {
                if gap && !out.is_empty() {
                    out.push(' ');
                }
                gap = false;
                out.push_str(s);
            }
            None => gap = true,
        }
    }
    out
}

/// One word of a transliterated name, reduced so that the forms different
/// states print for the same letter coincide:
/// `AE`, `AA` → `A` · `OE` → `O` · `UE`, `UXX` → `U` · `NXX` → `N` ·
/// `SS` → `S` · `TH` → `T` · `IJ` → `I` · any letter doubled → once.
/// `MUELLER`, `MULLER` and `MUXXLLER` all become `MULER`.
///
/// For hashing only. Different names can fall together (`MICHAEL`,
/// `MICHAL`); a person key has a procedure for people who share name and
/// birthday (ADR 0013, "Namesakes").
#[must_use]
pub fn fold(word: &str) -> String {
    let letters: alloc::vec::Vec<char> = word.chars().filter(char::is_ascii_uppercase).collect();
    let mut out = String::new();
    let mut i = 0usize;
    while let Some(&c) = letters.get(i) {
        let next = letters.get(i.saturating_add(1)).copied();
        let after = letters.get(i.saturating_add(2)).copied();
        let (keep, used) = match (c, next, after) {
            ('U' | 'N', Some('X'), Some('X')) => (c, 3usize),
            ('A' | 'O' | 'U', Some('E'), _) | ('A', Some('A'), _) => (c, 2),
            ('T', Some('H'), _) => ('T', 2),
            ('I', Some('J'), _) => ('I', 2),
            _ => (c, 1),
        };
        if !out.ends_with(keep) {
            out.push(keep);
        }
        i = i.saturating_add(used);
    }
    out
}
