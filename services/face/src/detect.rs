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

//! Finding faces: the YuNet detector of OpenCV Zoo (MIT licence).
//!
//! The model takes one 640 × 640 picture (BGR, 0–255, no normalisation) and
//! answers, for three grids (strides 8, 16, 32), a score, a box and five
//! landmarks per cell. The picture is scaled to fit and padded at the right
//! and bottom; positions are scaled back to the original picture.

use crate::{Error, Point};
use image::{imageops::FilterType, RgbImage};
use tract_onnx::prelude::*;

/// Side of the square the detector looks at.
pub const SIDE: u32 = 640;
const STRIDES: [usize; 3] = [8, 16, 32];

/// One face found in a picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// Top-left corner of the box.
    pub x: f32,
    /// Top of the box.
    pub y: f32,
    /// Width of the box.
    pub w: f32,
    /// Height of the box.
    pub h: f32,
    /// Confidence 0–1.
    pub score: f32,
    /// Eye on the left of the picture, eye on the right, nose tip, mouth
    /// corner on the left, mouth corner on the right.
    pub landmarks: [Point; 5],
}

impl Face {
    /// Distance between the eyes in pixels.
    pub fn eye_distance(&self) -> f32 {
        let [a, b, ..] = &self.landmarks;
        ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt()
    }
    /// Centre of the box.
    pub fn centre(&self) -> Point {
        Point {
            x: self.x + self.w / 2.0,
            y: self.y + self.h / 2.0,
        }
    }
    fn iou(&self, o: &Face) -> f32 {
        let x1 = self.x.max(o.x);
        let y1 = self.y.max(o.y);
        let x2 = (self.x + self.w).min(o.x + o.w);
        let y2 = (self.y + self.h).min(o.y + o.h);
        let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
        let union = self.w * self.h + o.w * o.h - inter;
        if union <= 0.0 {
            0.0
        } else {
            inter / union
        }
    }
}

type Plan = std::sync::Arc<TypedRunnableModel>;

/// The loaded detector.
pub struct Detector {
    plan: Plan,
    /// Faces below this score are dropped.
    pub min_score: f32,
    /// Overlap above which the weaker of two boxes is dropped.
    pub nms: f32,
}

impl Detector {
    /// Load `face_detection_yunet_2023mar.onnx`.
    pub fn load(path: &std::path::Path) -> Result<Self, Error> {
        let side = SIDE as usize;
        let plan = tract_onnx::onnx()
            .model_for_path(path)
            .and_then(|m| m.with_input_fact(0, f32::fact([1, 3, side, side]).into()))
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(|e| Error::Model(format!("detector: {e}")))?;
        Ok(Self {
            plan,
            min_score: 0.8,
            nms: 0.3,
        })
    }

    /// All faces in `img`, strongest first.
    pub fn detect(&self, img: &RgbImage) -> Result<Vec<Face>, Error> {
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return Err(Error::Picture("empty picture".into()));
        }
        let scale = (SIDE as f32 / w as f32).min(SIDE as f32 / h as f32);
        let nw = ((w as f32 * scale).round() as u32).clamp(1, SIDE);
        let nh = ((h as f32 * scale).round() as u32).clamp(1, SIDE);
        let small = image::imageops::resize(img, nw, nh, FilterType::Triangle);
        let side = SIDE as usize;
        let mut data = vec![0f32; 3 * side * side];
        for (x, y, p) in small.enumerate_pixels() {
            let i = y as usize * side + x as usize;
            // BGR order
            for (c, v) in [p[2], p[1], p[0]].into_iter().enumerate() {
                if let Some(slot) = data.get_mut(c * side * side + i) {
                    *slot = f32::from(v);
                }
            }
        }
        let input: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, side, side), data)
            .map_err(|e| Error::Model(e.to_string()))?
            .into();
        let out = self
            .plan
            .run(tvec!(input.into()))
            .map_err(|e| Error::Model(format!("detector: {e}")))?;
        let mut faces = decode(&out, self.min_score)?;
        for f in &mut faces {
            f.x /= scale;
            f.y /= scale;
            f.w /= scale;
            f.h /= scale;
            for p in &mut f.landmarks {
                p.x /= scale;
                p.y /= scale;
            }
        }
        Ok(suppress(faces, self.nms))
    }
}

fn view(out: &TVec<TValue>, i: usize) -> Result<&[f32], Error> {
    out.get(i)
        .ok_or_else(|| Error::Model("detector: missing output".into()))?
        .try_as_plain_ram()
        .and_then(|v| v.as_slice::<f32>())
        .map_err(|e| Error::Model(e.to_string()))
}

/// Outputs in model order: cls ×3, obj ×3, bbox ×3, kps ×3.
fn decode(out: &TVec<TValue>, min_score: f32) -> Result<Vec<Face>, Error> {
    let mut faces = Vec::new();
    for (k, &stride) in STRIDES.iter().enumerate() {
        let cols = SIDE as usize / stride;
        let cls = view(out, k)?;
        let obj = view(out, 3 + k)?;
        let bbox = view(out, 6 + k)?;
        let kps = view(out, 9 + k)?;
        if cls.len() != cols * cols
            || bbox.len() != 4 * cols * cols
            || kps.len() != 10 * cols * cols
        {
            return Err(Error::Model("detector: unexpected output shape".into()));
        }
        let s = stride as f32;
        for (i, (c, o)) in cls.iter().zip(obj).enumerate() {
            let score = (c.clamp(0.0, 1.0) * o.clamp(0.0, 1.0)).sqrt();
            if score < min_score {
                continue;
            }
            let (r, col) = ((i / cols) as f32, (i % cols) as f32);
            let b = bbox.get(4 * i..4 * i + 4).unwrap_or(&[0.0; 4]);
            let (cx, cy) = ((col + b[0]) * s, (r + b[1]) * s);
            let (w, h) = (b[2].exp() * s, b[3].exp() * s);
            let kp = kps.get(10 * i..10 * i + 10).unwrap_or(&[0.0; 10]);
            let lm = core::array::from_fn(|n| Point {
                x: (kp[2 * n] + col) * s,
                y: (kp[2 * n + 1] + r) * s,
            });
            faces.push(Face {
                x: cx - w / 2.0,
                y: cy - h / 2.0,
                w,
                h,
                score,
                landmarks: lm,
            });
        }
    }
    Ok(faces)
}

fn suppress(mut faces: Vec<Face>, nms: f32) -> Vec<Face> {
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Face> = Vec::new();
    for f in faces {
        if kept.iter().all(|k| k.iou(&f) <= nms) {
            kept.push(f);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    fn face(x: f32, score: f32) -> Face {
        Face {
            x,
            y: 0.0,
            w: 10.0,
            h: 10.0,
            score,
            landmarks: [Point { x: 0.0, y: 0.0 }; 5],
        }
    }
    #[test]
    fn overlapping_boxes_keep_the_strongest() {
        let kept = suppress(vec![face(0.0, 0.9), face(1.0, 0.95), face(50.0, 0.85)], 0.3);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].score, 0.95);
        assert_eq!(kept[1].x, 50.0);
    }
}
