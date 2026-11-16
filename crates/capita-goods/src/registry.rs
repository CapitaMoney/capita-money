// SPDX-License-Identifier: AGPL-3.0-only
//! The work registry: first registration wins (concept L2).
//!
//! A new upload is compared with every registered work. The same file, or
//! a text print at least [`GoodsParams::match_threshold`] alike, of another
//! seller → the listing is held, the seller asked, the earlier registrant
//! told. The same seller uploading a new edition of their own work is no
//! conflict. First registration is evidence, not a verdict: older dated
//! public evidence (publication, ISBN, timestamped drafts) beats it, and in
//! alpha the board of the Verein decides (rules diary 27).

use crate::fingerprint::{ContentHash, Fingerprint, TextPrint};
use crate::GoodsParams;
use capita_core::types::{AccountId, Share, Timestamp};

/// One registered work (the public part: no buyer, no sale).
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
pub struct Work {
    /// SHA-256 of the file as first uploaded.
    pub hash: ContentHash,
    /// Print of its text.
    pub text: Option<TextPrint>,
    /// Account of the seller who registered it.
    pub seller: AccountId,
    /// When it was registered.
    pub registered: Timestamp,
}

/// Why an upload matches a registered work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Match {
    /// The very same file.
    SameFile,
    /// The text prints agree this much.
    Similar(Share),
}

/// What happens to an upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision<'a> {
    /// No conflict: the work is registered.
    Register,
    /// The same seller registered this very file already.
    AlreadyYours(&'a Work),
    /// Another seller registered it earlier: listing held.
    Hold {
        /// The earliest registered work it matches.
        earlier: &'a Work,
        /// Why.
        why: Match,
    },
}

/// How `print` matches `work`, if at all.
fn matches(params: &GoodsParams, print: &Fingerprint, work: &Work) -> Option<Match> {
    if print.hash == work.hash {
        return Some(Match::SameFile);
    }
    let (Some(a), Some(b)) = (print.text.as_ref(), work.text.as_ref()) else {
        return None;
    };
    let s = a.similarity(b);
    (s >= params.match_threshold).then_some(Match::Similar(s))
}

/// The decision for an upload by `seller` with fingerprint `print`.
pub fn judge<'a>(
    params: &GoodsParams,
    print: &Fingerprint,
    seller: &AccountId,
    works: impl IntoIterator<Item = &'a Work>,
) -> Decision<'a> {
    let mut held: Option<(&'a Work, Match)> = None;
    let mut own: Option<&'a Work> = None;
    for work in works {
        let Some(why) = matches(params, print, work) else {
            continue;
        };
        if work.seller == *seller {
            if why == Match::SameFile {
                own = Some(work);
            }
            continue;
        }
        let earlier = held.is_none_or(|(w, _)| work.registered < w.registered);
        if earlier {
            held = Some((work, why));
        }
    }
    match (held, own) {
        (Some((earlier, why)), _) => Decision::Hold { earlier, why },
        (None, Some(w)) => Decision::AlreadyYours(w),
        (None, None) => Decision::Register,
    }
}
