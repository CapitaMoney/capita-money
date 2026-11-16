// SPDX-License-Identifier: AGPL-3.0-only
//! The licence of one sale: who holds it, how many downloads are used, and
//! when the seller's earnings of a new work become spendable.
//!
//! Every download of a sale is the same personal copy (same mark). A gift
//! moves the licence: the receiver gets a new personal copy with a mark of
//! their own, the giver's old copy keeps the giver's mark – so the giver
//! stays traceable and has a reason to delete it.

use crate::{GoodsError, GoodsParams};
use capita_core::types::{AccountId, Timestamp};

/// The licence of one sale.
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
pub struct Licence {
    /// Who may download it now.
    pub holder: AccountId,
    /// Downloads used by the current holder.
    pub downloads: u8,
    /// How often it was given away; part of the mark of each new holder's
    /// copy, so that every holder's copy differs.
    pub generation: u8,
}

impl Licence {
    /// A new licence for the buyer.
    #[must_use]
    pub const fn new(buyer: AccountId) -> Self {
        Self {
            holder: buyer,
            downloads: 0,
            generation: 0,
        }
    }

    /// Counts one download; an error once all are used.
    pub fn download(&mut self, params: &GoodsParams) -> Result<(), GoodsError> {
        if self.downloads >= params.max_downloads {
            return Err(GoodsError::DownloadsUsed);
        }
        self.downloads = self.downloads.saturating_add(1);
        Ok(())
    }

    /// Gives the licence to `to` (a gift; a resale uses the same move).
    pub fn give(&mut self, params: &GoodsParams, to: AccountId) -> Result<(), GoodsError> {
        if !params.gift {
            return Err(GoodsError::GiftOff);
        }
        if to == self.holder {
            return Err(GoodsError::SameHolder);
        }
        self.holder = to;
        self.downloads = 0;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(GoodsError::Arithmetic)?;
        Ok(())
    }
}

/// From when the earnings of a work registered at `registered` may be
/// spent (rule 28: earnings of a new work are held, so that a compensation
/// is still possible). Sales after that time are free at once.
pub fn earnings_free_from(
    params: &GoodsParams,
    registered: Timestamp,
) -> Result<Timestamp, GoodsError> {
    Ok(registered.checked_add(params.earnings_hold)?)
}
