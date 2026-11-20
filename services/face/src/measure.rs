// Capita Money – capita-face
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

//! How alike the prints of one person are, and those of different people:
//! the numbers from which face dedup chooses its `same_person` value
//! (rule 33, ADR 0030 of the repository).
//!
//! Face dedup compares a new chip photo with everybody of the same date of
//! birth. Two kinds of error:
//! - a **false hold**: an honest newcomer looks like somebody of the group
//!   and waits for the board. Per verification: 1 − (1 − FMR)^n, n = size of
//!   the group, FMR = share of pairs of different people at or above the
//!   value.
//! - a **missed double**: the same person with a second document is not
//!   found. FNMR = share of pairs of one person below the value.
//!
//! The pictures of one person should be what face dedup sees: document
//! photos taken years apart, not holiday photos of one afternoon.

use crate::embed::Print;

/// Likenesses of all pairs of prints: of one person, and of different
/// people.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Likenesses {
    /// Pairs of prints of one person.
    pub same: Vec<f32>,
    /// Pairs of prints of two different people.
    pub different: Vec<f32>,
}

/// Every pair: `people` holds the prints of each person.
#[must_use]
pub fn likenesses(people: &[Vec<Print>]) -> Likenesses {
    let mut out = Likenesses::default();
    for (i, mine) in people.iter().enumerate() {
        for (j, a) in mine.iter().enumerate() {
            for b in mine.iter().skip(j + 1) {
                out.same.push(a.cosine(b));
            }
            for theirs in people.iter().skip(i + 1) {
                for b in theirs {
                    out.different.push(a.cosine(b));
                }
            }
        }
    }
    out
}

/// Share of `values` at or above `value`; 0 for no values.
#[must_use]
pub fn share_from(values: &[f32], value: f32) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let hits = values.iter().filter(|&&v| v >= value).count();
    hits as f64 / values.len() as f64
}

/// Chance that an honest newcomer is held, in a group of `group` people,
/// when a pair of different people reaches the value with chance `fmr`.
#[must_use]
pub fn false_hold(fmr: f64, group: u32) -> f64 {
    1.0 - (1.0 - fmr).powi(i32::try_from(group).unwrap_or(i32::MAX))
}

/// Group sizes the report shows: about 1 000, 1 million and 10 million
/// accounts spread over the dates of birth of 100 years (36 525 days).
pub const GROUPS: [u32; 3] = [1, 27, 274];

/// The report as lines of text.
#[must_use]
pub fn report(people: usize, pictures: usize, without_face: usize, l: &Likenesses) -> Vec<String> {
    let mut lines = vec![
        format!(
            "{people} people, {pictures} pictures ({without_face} without a face), {} pairs of one person, {} pairs of different people",
            l.same.len(),
            l.different.len()
        ),
        format!(
            "highest likeness of different people {:.3}, lowest of one person {:.3}",
            l.different.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            l.same.iter().copied().fold(f32::INFINITY, f32::min),
        ),
        format!(
            "value   missed doubles   pairs of different people   false holds per verification, group of {} / {} / {}",
            GROUPS[0], GROUPS[1], GROUPS[2]
        ),
    ];
    for step in 6..=16u8 {
        let value = f32::from(step) * 0.05;
        let missed = 1.0 - share_from(&l.same, value);
        let fmr = share_from(&l.different, value);
        let holds: Vec<String> = GROUPS
            .iter()
            .map(|&n| format!("{:.4} %", 100.0 * false_hold(fmr, n)))
            .collect();
        lines.push(format!(
            "{value:.2}    {:>8.3} %       {:>10.5} %              {}",
            100.0 * missed,
            100.0 * fmr,
            holds.join(" / ")
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::DIM;

    fn axis(n: usize) -> Print {
        let mut v = [0f32; DIM];
        if let Some(x) = v.get_mut(n) {
            *x = 1.0;
        }
        Print(v)
    }

    #[test]
    fn pairs_are_counted_once() {
        let people = vec![vec![axis(0), axis(0)], vec![axis(1), axis(2)]];
        let l = likenesses(&people);
        assert_eq!(l.same, vec![1.0, 0.0]);
        assert_eq!(l.different, vec![0.0, 0.0, 0.0, 0.0]);
        assert!((share_from(&l.same, 0.5) - 0.5).abs() < 1e-9);
        assert!(share_from(&[], 0.5).abs() < 1e-9);
    }

    #[test]
    fn false_holds_grow_with_the_group() {
        assert!(false_hold(0.0, 274).abs() < 1e-12);
        assert!((false_hold(0.01, 1) - 0.01).abs() < 1e-12);
        let many = false_hold(0.01, 274);
        assert!(many > 0.93 && many < 0.94);
        let lines = report(2, 4, 0, &likenesses(&[vec![axis(0)], vec![axis(1)]]));
        assert_eq!(lines.len(), 3 + 11);
    }
}
