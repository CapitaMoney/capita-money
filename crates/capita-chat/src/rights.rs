// SPDX-License-Identifier: AGPL-3.0-only
//! Who may write. Reading public forums needs nothing (rule 5: unverified
//! members read, writing is greyed out). Writing – chat messages, posts,
//! comments, joining private chats – needs all of:
//!
//! 1. the account exists and is active (rules 9, 20);
//! 2. it belongs to a verified person (rule 30);
//! 3. it is the first account of that person (rules 21, 29): a double never
//!    writes, so a banned person cannot start again under a new account;
//! 4. no ban runs (rule 27 ladder; chat ladder default of the concept).

use capita_keys::types::AccountBytes;

use crate::chain::{ChainView, Person};

/// Why an account may not write. The app shows the matching greyed-out
/// state; none of these is shown as an id or code (rule 16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denied {
    /// No such account on the chain.
    UnknownAccount,
    /// Not verified: „Verify yourself first“.
    NotVerified,
    /// Inactive: re-verification overdue or account closed.
    Inactive,
    /// A younger account of a person who already has one.
    SecondAccount,
    /// Posting ban until this day (exclusive); reading stays.
    PostingBan {
        /// First day writing is allowed again.
        until: u32,
    },
    /// Ban from all social interaction until this day (exclusive).
    SocialBan {
        /// First day the chat opens again.
        until: u32,
    },
}

/// Whether `account` may write on `today`; the person on success.
///
/// # Errors
/// [`Denied`]: why not.
pub fn may_post(
    chain: &impl ChainView,
    account: &AccountBytes,
    today: u32,
) -> Result<Person, Denied> {
    let info = chain.account(account).ok_or(Denied::UnknownAccount)?;
    if !info.active {
        return Err(Denied::Inactive);
    }
    let person = info.person.ok_or(Denied::NotVerified)?;
    if chain.first_account_of(&person).as_ref() != Some(account) {
        return Err(Denied::SecondAccount);
    }
    if let Some(until) = info.social_ban_until.filter(|u| today < *u) {
        return Err(Denied::SocialBan { until });
    }
    if let Some(until) = info.posting_ban_until.filter(|u| today < *u) {
        return Err(Denied::PostingBan { until });
    }
    Ok(person)
}
