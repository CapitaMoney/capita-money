// SPDX-License-Identifier: AGPL-3.0-only
//! The sanction ladder (rules diary 27 and 28, owner 2026-11-16).
//!
//! | Position | Step | What it costs (one calendar month each) |
//! |---|---|---|
//! | 1 | [`Step::Warning`] | a quick warning; the seller is compensated |
//! | 2 | [`Step::SellingBan`] | no selling: illegal products are taken down, the own sales frozen, legal products stay in the shop but cannot be sold |
//! | 3 and on | [`Step::FullBan`] | also no buying, no Capita movements (sending, receiving, paying in person, the euro door), no chat, video chat or social network |
//!
//! Each new ban includes the earlier ones and starts when the running one
//! ends ("all punishments add up"). Through every ban a member still sees
//! everything, receives the Capita of their age (frozen with the rest of
//! the account during a full ban), votes in glassroots, objects, verifies
//! again – nobody is shut out of the realm, and the yearly verification
//! never lapses because of a ban.
//!
//! A leaked copy enters the ladder at the next position; the seller of a
//! stolen work skips the warning and enters at position 2 at least. Two
//! years without a violation ([`GoodsParams::reset_after`]) and the ladder
//! starts again. Sellers see only the [`Trust`] of a member, never the
//! record itself.

use crate::{calendar::add_months, GoodsError, GoodsParams};
use capita_core::types::Timestamp;

/// What a member did, proven by the decider.
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
pub enum Offence {
    /// A bought personal copy was shared (one leaked file = one violation).
    LeakedCopy,
    /// A work of somebody else was sold on purpose (rule 28).
    StolenWork,
}

/// The step of the ladder a violation lands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
pub enum Step {
    /// A quick warning, nothing banned.
    Warning,
    /// One month without selling.
    SellingBan,
    /// One month without selling, buying, Capita movements and social
    /// interaction.
    FullBan,
}

impl Step {
    /// The step of a position on the ladder (1 = first violation).
    #[must_use]
    pub const fn at(position: u8) -> Self {
        match position {
            0 | 1 => Self::Warning,
            2 => Self::SellingBan,
            _ => Self::FullBan,
        }
    }
}

/// Everything a member can do in the app, sorted by what a ban touches.
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
pub enum Action {
    /// Look at anything: shop, articles, statistics, profiles, the own
    /// account. Never banned.
    View,
    /// Object to a decision. Never banned.
    Object,
    /// Verify again (the yearly verification). Never banned.
    Reverify,
    /// Receive the Capita of the own age (creation). Never banned.
    ClaimIssuance,
    /// Vote in glassroots. Never banned (owner 2026-11-16).
    Vote,
    /// Sell in the webshop. Banned from step 2.
    Sell,
    /// Buy in the webshop (watching stays). Banned from step 3.
    Buy,
    /// Send Capita. Banned from step 3.
    Send,
    /// Receive Capita from others. Banned from step 3.
    Receive,
    /// Pay in person with the Capita receiver. Banned from step 3.
    PayInPerson,
    /// Buy Capita with euro at the euro door. Banned from step 3.
    EuroDoor,
    /// Write in the verified chat. Banned from step 3.
    Chat,
    /// Video chat. Banned from step 3.
    VideoChat,
    /// Post in the social network. Banned from step 3.
    Social,
}

impl Action {
    /// The first step that bans it; `None` = never banned.
    #[must_use]
    pub const fn banned_from(self) -> Option<Step> {
        match self {
            Self::View | Self::Object | Self::Reverify | Self::ClaimIssuance | Self::Vote => None,
            Self::Sell => Some(Step::SellingBan),
            Self::Buy
            | Self::Send
            | Self::Receive
            | Self::PayInPerson
            | Self::EuroDoor
            | Self::Chat
            | Self::VideoChat
            | Self::Social => Some(Step::FullBan),
        }
    }
}

/// What sellers see of a member (rule 27: no public pillory).
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
pub enum Trust {
    /// No violation within the reset time.
    Ok,
    /// At least one violation within the reset time.
    Reduced,
}

/// The outcome of one proven violation, for the notice to the member
/// ("why banned", with the button to object).
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
pub struct Sanction {
    /// Position on the ladder after this violation.
    pub position: u8,
    /// The step it landed on.
    pub step: Step,
    /// Selling banned until then (`None` = not banned).
    pub selling_until: Option<Timestamp>,
    /// Everything of a full ban banned until then (`None` = not banned).
    pub full_until: Option<Timestamp>,
}

/// The record of one member (one person: the record follows the person,
/// not a key).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
pub struct Record {
    /// Position on the ladder; 0 = clean.
    pub position: u8,
    /// Time of the last proven violation.
    pub last: Option<Timestamp>,
    /// Selling banned until then.
    pub selling_until: Option<Timestamp>,
    /// Everything of a full ban banned until then.
    pub full_until: Option<Timestamp>,
}

/// The later of a running ban's end and now.
fn later(until: Option<Timestamp>, now: Timestamp) -> Timestamp {
    until.map_or(now, |u| u.max(now))
}

impl Record {
    /// A member without any violation.
    pub const CLEAN: Self = Self {
        position: 0,
        last: None,
        selling_until: None,
        full_until: None,
    };

    /// The position that counts at `now`: 0 once the reset time has passed
    /// since the last violation.
    pub fn position_at(&self, params: &GoodsParams, now: Timestamp) -> Result<u8, GoodsError> {
        let Some(last) = self.last else {
            return Ok(0);
        };
        let since = now.duration_since(last).map_err(|_| GoodsError::Clock)?;
        Ok(if since >= params.reset_after {
            0
        } else {
            self.position
        })
    }

    /// Records one proven violation at `now` and returns the sanction.
    ///
    /// The ban starts when the running ban of the same kind ends, so bans
    /// add up; a full ban also covers selling until it ends.
    pub fn apply(
        &mut self,
        params: &GoodsParams,
        offence: Offence,
        now: Timestamp,
    ) -> Result<Sanction, GoodsError> {
        let current = self.position_at(params, now)?;
        let next = current.saturating_add(1);
        let position = match offence {
            Offence::LeakedCopy => next,
            Offence::StolenWork => next.max(2),
        };
        let step = Step::at(position);
        let mut selling_until = self.selling_until;
        let mut full_until = self.full_until;
        match step {
            Step::Warning => {}
            Step::SellingBan => {
                let start = later(selling_until, now);
                selling_until = Some(add_months(start, params.ban_months)?);
            }
            Step::FullBan => {
                let start = later(full_until, now);
                let end = add_months(start, params.ban_months)?;
                full_until = Some(end);
                selling_until = Some(later(selling_until, end));
            }
        }
        self.position = position;
        self.last = Some(now);
        self.selling_until = selling_until;
        self.full_until = full_until;
        Ok(Sanction {
            position,
            step,
            selling_until,
            full_until,
        })
    }

    /// Whether the member may do `action` at `now`.
    #[must_use]
    pub fn allows(&self, action: Action, now: Timestamp) -> bool {
        let running = |until: Option<Timestamp>| until.is_some_and(|u| now < u);
        match action.banned_from() {
            None => true,
            Some(Step::Warning) => true,
            Some(Step::SellingBan) => !running(self.selling_until) && !running(self.full_until),
            Some(Step::FullBan) => !running(self.full_until),
        }
    }

    /// What sellers see.
    pub fn trust(&self, params: &GoodsParams, now: Timestamp) -> Result<Trust, GoodsError> {
        Ok(if self.position_at(params, now)? == 0 {
            Trust::Ok
        } else {
            Trust::Reduced
        })
    }
}
