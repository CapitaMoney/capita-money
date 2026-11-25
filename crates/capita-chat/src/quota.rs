// SPDX-License-Identifier: AGPL-3.0-only
//! The free daily quota (rules diary 32: no fees). Receiving apps and
//! mailboxes count per person and day and drop what goes beyond. The
//! numbers come from the rules file (`[chat]` of `rules/alpha.toml`).

use std::collections::BTreeMap;

use crate::chain::Person;
use crate::ChatError;

/// The limits of the rules file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Chat messages per person and day (`chat.messages_per_day`).
    pub messages_per_day: u32,
    /// Forum posts and comments per person and day (`chat.posts_per_day`).
    pub posts_per_day: u32,
}

/// What is counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum What {
    /// A chat message.
    Message,
    /// A forum post or comment.
    Post,
}

/// The counts of one app or mailbox.
#[derive(Clone, Debug, Default)]
pub struct Quota {
    counts: BTreeMap<(Person, u32, What), u32>,
}

impl Quota {
    /// No counts yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts one item of `person` on `day`; refused beyond the limit.
    /// Counts of earlier days are dropped.
    ///
    /// # Errors
    /// [`ChatError::QuotaReached`].
    pub fn count(
        &mut self,
        limits: &Limits,
        person: &Person,
        day: u32,
        what: What,
    ) -> Result<(), ChatError> {
        self.counts.retain(|(_, d, _), _| *d >= day);
        let limit = match what {
            What::Message => limits.messages_per_day,
            What::Post => limits.posts_per_day,
        };
        let n = self.counts.entry((*person, day, what)).or_insert(0);
        if *n >= limit {
            return Err(ChatError::QuotaReached);
        }
        *n = n.saturating_add(1);
        Ok(())
    }
}
