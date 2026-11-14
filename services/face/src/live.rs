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

//! Liveness by challenge: the service names a random order of head
//! movements; the app films the person doing them; the service checks the
//! frames. A printed photo or a still on a screen cannot turn its head.
//!
//! Pose is read from the five landmarks alone, no extra model:
//! - turn (yaw) = sideways offset of the nose tip from the middle of the
//!   eyes, in eye distances;
//! - nod (pitch) = height of the nose between eyes and mouth, compared with
//!   the first frame that looks straight ahead.
//!
//! Frames are taken as the camera sees them, not mirrored: when the person
//! turns to their own left, the nose moves to the right of the picture.

use crate::detect::Face;
use crate::embed::Print;
use crate::Point;

/// A head position asked for or seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pose {
    /// Looking at the camera.
    Straight,
    /// Head turned to the person's own left.
    Left,
    /// Head turned to the person's own right.
    Right,
    /// Chin up.
    Up,
    /// Chin down.
    Down,
}

impl Pose {
    /// Word used in challenges and on the command line.
    pub fn word(self) -> &'static str {
        match self {
            Pose::Straight => "straight",
            Pose::Left => "left",
            Pose::Right => "right",
            Pose::Up => "up",
            Pose::Down => "down",
        }
    }
    /// Pose from its word.
    pub fn from_word(w: &str) -> Option<Pose> {
        [
            Pose::Straight,
            Pose::Left,
            Pose::Right,
            Pose::Up,
            Pose::Down,
        ]
        .into_iter()
        .find(|p| p.word() == w)
    }
}

/// The movements that can be asked for.
pub const MOVES: [Pose; 4] = [Pose::Left, Pose::Right, Pose::Up, Pose::Down];

/// Limits of the challenge. Technical values (ADR 0015), not rules.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Turn counted as left/right from this many eye distances.
    pub turn: f32,
    /// Straight only below this turn.
    pub straight_turn: f32,
    /// Nod counted as up/down from this change of the nose height.
    pub nod: f32,
    /// Straight only below this change of the nose height.
    pub straight_nod: f32,
    /// Smallest eye distance in pixels (face too far or too small below).
    pub min_eye_px: f32,
    /// Largest move of the face between two frames, in eye distances.
    pub max_jump: f32,
    /// Largest change of the face size between two frames (factor).
    pub max_zoom: f32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            turn: 0.25,
            straight_turn: 0.12,
            nod: 0.12,
            straight_nod: 0.07,
            min_eye_px: 30.0,
            max_jump: 1.5,
            max_zoom: 1.5,
        }
    }
}

/// Why frames were not accepted.
#[derive(Clone, Debug, PartialEq)]
pub enum Fault {
    /// Frame `n` (from 0) shows no face.
    NoFace(usize),
    /// Frame `n` shows more than one face.
    SeveralFaces(usize),
    /// Frame `n`: face too small.
    TooSmall(usize),
    /// Between frame `n − 1` and `n` the face jumped or changed size: not one take.
    Jump(usize),
    /// The movements asked for were not seen, in this order, between two straight looks.
    WrongMoves,
}

/// Turn and nose height of a face.
pub fn angles(f: &Face) -> (f32, f32) {
    let [el, er, nose, ml, mr] = f.landmarks;
    let eyes = Point {
        x: (el.x + er.x) / 2.0,
        y: (el.y + er.y) / 2.0,
    };
    let mouth_y = (ml.y + mr.y) / 2.0;
    let d = f.eye_distance().max(f32::EPSILON);
    let turn = (nose.x - eyes.x) / d;
    let span = mouth_y - eyes.y;
    let height = if span.abs() <= f32::EPSILON {
        0.0
    } else {
        (nose.y - eyes.y) / span
    };
    (turn, height)
}

/// Pose of a face, given the nose height of the person looking straight;
/// `None` = between poses.
pub fn pose(f: &Face, straight_height: f32, l: &Limits) -> Option<Pose> {
    let (turn, height) = angles(f);
    let nod = height - straight_height;
    if turn.abs() < l.straight_turn && nod.abs() < l.straight_nod {
        return Some(Pose::Straight);
    }
    if turn.abs() >= l.turn && turn.abs() >= nod.abs() {
        // nose to the right of the picture = the person's left
        return Some(if turn > 0.0 { Pose::Left } else { Pose::Right });
    }
    if nod.abs() >= l.nod && turn.abs() < l.turn {
        return Some(if nod < 0.0 { Pose::Up } else { Pose::Down });
    }
    None
}

/// What the frames showed, once accepted.
#[derive(Clone, Debug, PartialEq)]
pub struct Take {
    /// Frame (from 0) of the first straight look.
    pub first: usize,
    /// Frame of the last straight look, after all movements.
    pub last: usize,
}

/// Check one take: `faces[n]` = all faces found in frame `n`.
pub fn check(faces: &[Vec<Face>], asked: &[Pose], l: &Limits) -> Result<Take, Fault> {
    let mut one: Vec<&Face> = Vec::with_capacity(faces.len());
    for (n, fs) in faces.iter().enumerate() {
        let f = match fs.as_slice() {
            [] => return Err(Fault::NoFace(n)),
            [f] => f,
            _ => return Err(Fault::SeveralFaces(n)),
        };
        if f.eye_distance() < l.min_eye_px {
            return Err(Fault::TooSmall(n));
        }
        if let Some(prev) = one.last() {
            let (a, b) = (prev.centre(), f.centre());
            let d = prev.eye_distance().max(f32::EPSILON);
            let jump = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt() / d;
            let zoom = f.w.max(f32::EPSILON) / prev.w.max(f32::EPSILON);
            if jump > l.max_jump || zoom > l.max_zoom || zoom < 1.0 / l.max_zoom {
                return Err(Fault::Jump(n));
            }
        }
        one.push(f);
    }
    // the first frame looking straight sets the person's own nose height
    let Some(base) = one
        .iter()
        .map(|f| angles(f))
        .find(|(t, _)| t.abs() < l.straight_turn)
        .map(|(_, h)| h)
    else {
        return Err(Fault::WrongMoves);
    };
    let mut want: Vec<Pose> = Vec::with_capacity(asked.len() + 2);
    want.push(Pose::Straight);
    want.extend(asked.iter().copied().filter(|p| *p != Pose::Straight));
    want.push(Pose::Straight);
    let (mut k, mut first, mut last) = (0usize, None, None);
    for (n, f) in one.iter().enumerate() {
        let Some(&w) = want.get(k) else { break };
        if pose(f, base, l) == Some(w) {
            if k == 0 {
                first = Some(n);
            }
            if k + 1 == want.len() {
                last = Some(n);
            }
            k += 1;
        }
    }
    match (first, last) {
        (Some(first), Some(last)) => Ok(Take { first, last }),
        _ => Err(Fault::WrongMoves),
    }
}

/// Lowest likeness of the two straight looks to the document photo.
pub fn likeness(document: &Print, first: &Print, last: &Print) -> f32 {
    document.cosine(first).min(document.cosine(last))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A face 100 px between the eyes; `turn` and `nod` move the nose.
    fn face(turn: f32, nod: f32, dx: f32) -> Vec<Face> {
        let p = |x: f32, y: f32| Point { x: x + dx, y };
        vec![Face {
            x: 50.0 + dx,
            y: 50.0,
            w: 200.0,
            h: 250.0,
            score: 0.9,
            landmarks: [
                p(100.0, 100.0),
                p(200.0, 100.0),
                p(150.0 + 100.0 * turn, 150.0 + 100.0 * nod),
                p(110.0, 200.0),
                p(190.0, 200.0),
            ],
        }]
    }
    fn s() -> Vec<Face> {
        face(0.0, 0.0, 0.0)
    }
    const L: f32 = 0.35; // person's left = nose to picture right

    #[test]
    fn poses_are_read_from_the_landmarks() {
        let l = Limits::default();
        let base = angles(&s()[0]).1;
        assert_eq!(pose(&s()[0], base, &l), Some(Pose::Straight));
        assert_eq!(pose(&face(L, 0.0, 0.0)[0], base, &l), Some(Pose::Left));
        assert_eq!(pose(&face(-L, 0.0, 0.0)[0], base, &l), Some(Pose::Right));
        assert_eq!(pose(&face(0.0, -0.2, 0.0)[0], base, &l), Some(Pose::Up));
        assert_eq!(pose(&face(0.0, 0.2, 0.0)[0], base, &l), Some(Pose::Down));
        assert_eq!(pose(&face(0.18, 0.0, 0.0)[0], base, &l), None);
    }

    #[test]
    fn the_asked_moves_in_order_pass() {
        let frames = vec![
            s(),
            face(0.1, 0.0, 0.0),
            face(L, 0.0, 0.0),
            s(),
            face(-L, 0.0, 0.0),
            face(0.0, 0.0, 0.0),
        ];
        let t = check(&frames, &[Pose::Left, Pose::Right], &Limits::default());
        assert_eq!(t, Ok(Take { first: 0, last: 5 }));
    }

    #[test]
    fn the_wrong_order_fails() {
        let frames = vec![s(), face(-L, 0.0, 0.0), s(), face(L, 0.0, 0.0), s()];
        assert_eq!(
            check(&frames, &[Pose::Left, Pose::Right], &Limits::default()),
            Err(Fault::WrongMoves)
        );
    }

    #[test]
    fn a_still_photo_fails() {
        let frames = vec![s(); 10];
        assert_eq!(
            check(&frames, &[Pose::Left, Pose::Up], &Limits::default()),
            Err(Fault::WrongMoves)
        );
    }

    #[test]
    fn no_straight_look_at_the_end_fails() {
        let frames = vec![s(), face(L, 0.0, 0.0), face(0.0, 0.2, 0.0)];
        assert_eq!(
            check(&frames, &[Pose::Left, Pose::Down], &Limits::default()),
            Err(Fault::WrongMoves)
        );
    }

    #[test]
    fn faces_missing_doubled_small_or_jumping_fail() {
        let l = Limits::default();
        assert_eq!(
            check(&[s(), vec![]], &[Pose::Left], &l),
            Err(Fault::NoFace(1))
        );
        let mut two = s();
        two.extend(s());
        assert_eq!(
            check(&[two], &[Pose::Left], &l),
            Err(Fault::SeveralFaces(0))
        );
        let mut small = s();
        small[0].landmarks[1].x = 110.0;
        assert_eq!(check(&[small], &[Pose::Left], &l), Err(Fault::TooSmall(0)));
        assert_eq!(
            check(&[s(), face(0.0, 0.0, 400.0)], &[Pose::Left], &l),
            Err(Fault::Jump(1))
        );
    }

    #[test]
    fn words_round_trip() {
        for p in [
            Pose::Straight,
            Pose::Left,
            Pose::Right,
            Pose::Up,
            Pose::Down,
        ] {
            assert_eq!(Pose::from_word(p.word()), Some(p));
        }
        assert_eq!(Pose::from_word("sideways"), None);
    }
}
