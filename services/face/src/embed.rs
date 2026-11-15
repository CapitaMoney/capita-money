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

//! Telling faces apart: the SFace recogniser of OpenCV Zoo (Apache 2.0).
//!
//! Input: the 112 × 112 cut-out (RGB, 0–255). Output: 128 numbers. Two cut-outs
//! of the same person point in nearly the same direction (cosine close to 1).

use crate::Error;
use image::RgbImage;
use tract_onnx::prelude::*;

/// Length of a face print.
pub const DIM: usize = 128;

/// A face print, scaled to length 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Print(pub [f32; DIM]);

impl Print {
    /// Cosine of the angle between two prints: 1 = same direction.
    pub fn cosine(&self, other: &Print) -> f32 {
        self.0.iter().zip(other.0.iter()).map(|(a, b)| a * b).sum()
    }
}

type Plan = std::sync::Arc<TypedRunnableModel>;

/// The loaded recogniser.
pub struct Recogniser {
    plan: Plan,
}

impl Recogniser {
    /// Load `face_recognition_sface_2021dec.onnx`.
    pub fn load(path: &std::path::Path) -> Result<Self, Error> {
        let side = crate::align::SIDE as usize;
        let plan = tract_onnx::onnx()
            .model_for_path(path)
            .and_then(|m| m.with_input_fact(0, f32::fact([1, 3, side, side]).into()))
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(|e| Error::Model(format!("recogniser: {e}")))?;
        Ok(Self { plan })
    }

    /// The print of a 112 × 112 cut-out.
    pub fn print(&self, crop: &RgbImage) -> Result<Print, Error> {
        let side = crate::align::SIDE as usize;
        if crop.dimensions() != (crate::align::SIDE, crate::align::SIDE) {
            return Err(Error::Picture("cut-out is not 112 × 112".into()));
        }
        let mut data = vec![0f32; 3 * side * side];
        for (x, y, p) in crop.enumerate_pixels() {
            let i = y as usize * side + x as usize;
            for c in 0..3 {
                if let Some(slot) = data.get_mut(c * side * side + i) {
                    *slot = f32::from(p[c]);
                }
            }
        }
        let input: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, side, side), data)
            .map_err(|e| Error::Model(e.to_string()))?
            .into();
        let out = self
            .plan
            .run(tvec!(input.into()))
            .map_err(|e| Error::Model(format!("recogniser: {e}")))?;
        let v = out
            .first()
            .ok_or_else(|| Error::Model("recogniser: no output".into()))?
            .try_as_plain_ram()
            .and_then(|v| v.as_slice::<f32>())
            .map_err(|e| Error::Model(e.to_string()))?;
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if v.len() != DIM || !norm.is_finite() || norm <= f32::EPSILON {
            return Err(Error::Model("recogniser: unusable output".into()));
        }
        let mut p = [0f32; DIM];
        for (slot, x) in p.iter_mut().zip(v) {
            *slot = x / norm;
        }
        Ok(Print(p))
    }
}
