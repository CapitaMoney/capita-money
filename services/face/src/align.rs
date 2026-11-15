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

//! Cutting a face out of a picture the way the recogniser expects it: five
//! landmarks moved onto fixed places of a 112 × 112 square (the ArcFace
//! reference), by the similarity transform (turn, scale, shift) that fits
//! them best in the least-squares sense (Umeyama).

use crate::Point;
use image::{Rgb, RgbImage};

/// Side of the cut-out.
pub const SIDE: u32 = 112;

/// Where the five landmarks go (eye left, eye right, nose, mouth left, mouth right).
pub const REFERENCE: [Point; 5] = [
    Point {
        x: 38.2946,
        y: 51.6963,
    },
    Point {
        x: 73.5318,
        y: 51.5014,
    },
    Point {
        x: 56.0252,
        y: 71.7366,
    },
    Point {
        x: 41.5493,
        y: 92.3655,
    },
    Point {
        x: 70.7299,
        y: 92.2041,
    },
];

/// `[a, b, tx, ty]`: `x' = a·x − b·y + tx`, `y' = b·x + a·y + ty`.
pub type Similarity = [f32; 4];

/// Best similarity transform taking `src` onto `dst`; `None` if `src` is degenerate.
pub fn similarity(src: &[Point], dst: &[Point]) -> Option<Similarity> {
    let n = src.len().min(dst.len());
    if n < 2 {
        return None;
    }
    let mean = |ps: &[Point]| {
        let (sx, sy) = ps
            .iter()
            .take(n)
            .fold((0.0, 0.0), |(x, y), p| (x + p.x, y + p.y));
        Point {
            x: sx / n as f32,
            y: sy / n as f32,
        }
    };
    let (ms, md) = (mean(src), mean(dst));
    let (mut dot, mut cross, mut norm) = (0.0f32, 0.0f32, 0.0f32);
    for (s, d) in src.iter().zip(dst).take(n) {
        let (sx, sy, dx, dy) = (s.x - ms.x, s.y - ms.y, d.x - md.x, d.y - md.y);
        dot += sx * dx + sy * dy;
        cross += sx * dy - sy * dx;
        norm += sx * sx + sy * sy;
    }
    if norm <= f32::EPSILON {
        return None;
    }
    let (a, b) = (dot / norm, cross / norm);
    Some([
        a,
        b,
        md.x - (a * ms.x - b * ms.y),
        md.y - (b * ms.x + a * ms.y),
    ])
}

/// Apply `t` to `p`.
pub fn apply(t: &Similarity, p: Point) -> Point {
    let [a, b, tx, ty] = *t;
    Point {
        x: a * p.x - b * p.y + tx,
        y: b * p.x + a * p.y + ty,
    }
}

/// The 112 × 112 cut-out of the face with `landmarks`; `None` if they are degenerate.
pub fn crop(img: &RgbImage, landmarks: &[Point; 5]) -> Option<RgbImage> {
    let t = similarity(landmarks, &REFERENCE)?;
    let [a, b, tx, ty] = t;
    let s2 = a * a + b * b;
    if s2 <= f32::EPSILON {
        return None;
    }
    let mut out = RgbImage::new(SIDE, SIDE);
    for (u, v, px) in out.enumerate_pixels_mut() {
        // inverse: p = R⁻¹ (q − t), R⁻¹ = [a b; −b a] / s²
        let (qx, qy) = (u as f32 - tx, v as f32 - ty);
        let (x, y) = ((a * qx + b * qy) / s2, (-b * qx + a * qy) / s2);
        *px = bilinear(img, x, y);
    }
    Some(out)
}

fn bilinear(img: &RgbImage, x: f32, y: f32) -> Rgb<u8> {
    let (w, h) = img.dimensions();
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let get = |xi: f32, yi: f32| -> [f32; 3] {
        if xi < 0.0 || yi < 0.0 || xi >= w as f32 || yi >= h as f32 {
            return [0.0; 3];
        }
        let p = img.get_pixel(xi as u32, yi as u32);
        [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])]
    };
    let (p00, p10, p01, p11) = (
        get(x0, y0),
        get(x0 + 1.0, y0),
        get(x0, y0 + 1.0),
        get(x0 + 1.0, y0 + 1.0),
    );
    let mut o = [0u8; 3];
    for (c, slot) in o.iter_mut().enumerate() {
        let top = p00[c] * (1.0 - fx) + p10[c] * fx;
        let bot = p01[c] * (1.0 - fx) + p11[c] * fx;
        *slot = (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0) as u8;
    }
    Rgb(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_reference_maps_onto_itself() {
        let t = similarity(&REFERENCE, &REFERENCE).unwrap_or([0.0; 4]);
        for (got, want) in [t[0], t[1], t[2], t[3]].iter().zip([1.0, 0.0, 0.0, 0.0]) {
            assert!((got - want).abs() < 1e-4, "{t:?}");
        }
    }
    #[test]
    fn a_turned_scaled_shifted_face_is_found_again() {
        let (a, b, tx, ty) = (0.8f32 * 0.3f32.cos(), 0.8f32 * 0.3f32.sin(), 12.0, -7.0);
        let moved: Vec<Point> = REFERENCE
            .iter()
            .map(|p| apply(&[a, b, tx, ty], *p))
            .collect();
        let back = similarity(&moved, &REFERENCE).unwrap_or([0.0; 4]);
        for (p, q) in moved.iter().zip(REFERENCE) {
            let r = apply(&back, *p);
            assert!((r.x - q.x).abs() < 1e-3 && (r.y - q.y).abs() < 1e-3);
        }
    }
    #[test]
    fn identical_landmarks_are_refused() {
        assert!(similarity(&[Point { x: 1.0, y: 1.0 }; 5], &REFERENCE).is_none());
    }
}
