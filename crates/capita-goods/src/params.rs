// SPDX-License-Identifier: AGPL-3.0-only
//! The values of the goods rules (`[goods]` of a rules file).

use crate::GoodsError;
use capita_core::types::{Duration, Share};

/// The values of the goods rules, as the rules file states them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "scale",
    derive(
        codec::Encode,
        codec::Decode,
        codec::DecodeWithMemTracking,
        codec::MaxEncodedLen,
        scale_info::TypeInfo
    )
)]
pub struct GoodsParams {
    /// Length of one ban, in calendar months (the 17th to the 17th).
    pub ban_months: u8,
    /// Time without a violation after which the ladder starts again.
    pub reset_after: Duration,
    /// Compensation for the seller, as a multiple of the price.
    pub compensation_factor: u8,
    /// Reward for whoever finds a stolen work, as a multiple of the price.
    pub finder_reward_factor: u8,
    /// Downloads one sale gives; each one is the same personal copy.
    pub max_downloads: u8,
    /// Time an accused member has to answer before the decision.
    pub answer_time: Duration,
    /// Time the earnings of a newly registered work are held, so that a
    /// compensation is still possible.
    pub earnings_hold: Duration,
    /// Similarity of the text print from which an upload counts as the
    /// same work as a registered one.
    pub match_threshold: Share,
    /// A buyer may give the licence away.
    pub gift: bool,
    /// A buyer may sell the licence on.
    pub resale: bool,
}

impl GoodsParams {
    /// Checks that the values fit together.
    pub fn validate(&self) -> Result<(), GoodsError> {
        if !(1..=12).contains(&self.ban_months) {
            return Err(GoodsError::InvalidParams("ban_months: 1 to 12"));
        }
        if self.reset_after.is_zero() {
            return Err(GoodsError::InvalidParams(
                "reset_after: some time, not \"off\"",
            ));
        }
        if self.max_downloads == 0 {
            return Err(GoodsError::InvalidParams("max_downloads: at least 1"));
        }
        if self.match_threshold.is_zero() {
            return Err(GoodsError::InvalidParams(
                "match_threshold: above 0 %, else every upload matches",
            ));
        }
        if self.resale && !self.gift {
            return Err(GoodsError::InvalidParams(
                "resale needs gift: a resale moves the licence like a gift",
            ));
        }
        Ok(())
    }
}
