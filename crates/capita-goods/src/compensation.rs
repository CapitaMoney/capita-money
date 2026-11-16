// SPDX-License-Identifier: AGPL-3.0-only
//! Compensation for the seller and the reward for a finder.
//!
//! Rule 27: the seller of a leaked work receives 1 × the price from the
//! leaker – at every violation, the warning included. Rule 10 (no debt): it
//! is taken from the balance only, never below zero; the rest lapses and is
//! never taken from future Capita (that would be a hidden debt).

use crate::{GoodsError, GoodsParams};
use capita_core::types::Amount;

/// How a compensation is settled.
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
pub struct Settlement {
    /// Moves from the leaker to the seller.
    pub paid: Amount,
    /// Owed but not there: lapses.
    pub lapsed: Amount,
}

/// `price` × `factor`.
fn times(price: Amount, factor: u8) -> Result<Amount, GoodsError> {
    price
        .cents()
        .checked_mul(u64::from(factor))
        .map(Amount::from_cents)
        .ok_or(GoodsError::Arithmetic)
}

/// The compensation for one leaked copy sold at `price`, out of `balance`.
pub fn compensation(
    params: &GoodsParams,
    price: Amount,
    balance: Amount,
) -> Result<Settlement, GoodsError> {
    let due = times(price, params.compensation_factor)?;
    let paid = due.min(balance);
    Ok(Settlement {
        paid,
        lapsed: due.checked_sub(paid)?,
    })
}

/// The reward for whoever finds a stolen work sold at `price` (rule 28).
/// Who pays it is still open (rules diary 28, `Offene Frage:`).
pub fn finder_reward(params: &GoodsParams, price: Amount) -> Result<Amount, GoodsError> {
    times(price, params.finder_reward_factor)
}
