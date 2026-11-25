// SPDX-License-Identifier: AGPL-3.0-only
//! What a chat message holds (inside the encryption): the text and,
//! for a comment, the message it comments on (owner: „comments under each
//! chat“). Threads are built by the app from these parents.

use capita_keys::encoding::blake2_256;

use crate::codec::{option, put_long, put_option, Reader};
use crate::ChatError;

/// Domain tag of message ids.
pub const MESSAGE_ID_TAG: &[u8] = b"capita/chat-message/v1";
/// Longest text, in bytes of UTF-8.
pub const MAX_TEXT: usize = 16 * 1024;

/// One chat message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatMessage {
    /// The message this one comments on, if any ([`message_id`]).
    pub parent: Option<[u8; 32]>,
    /// The text.
    pub text: String,
}

impl ChatMessage {
    /// A message that starts a thread.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self {
            parent: None,
            text: text.to_owned(),
        }
    }

    /// A comment under `parent`.
    #[must_use]
    pub fn comment(parent: [u8; 32], text: &str) -> Self {
        Self {
            parent: Some(parent),
            text: text.to_owned(),
        }
    }

    /// The message as bytes: version, parent, text.
    ///
    /// # Errors
    /// Empty text or text longer than [`MAX_TEXT`].
    pub fn to_bytes(&self) -> Result<Vec<u8>, ChatError> {
        check_text(&self.text)?;
        let mut out = Vec::with_capacity(self.text.len().saturating_add(40));
        out.push(1);
        put_option(&mut out, self.parent.as_ref());
        put_long(&mut out, self.text.as_bytes(), "text")?;
        Ok(out)
    }

    /// Reads what [`ChatMessage::to_bytes`] wrote.
    ///
    /// # Errors
    /// Bytes that are no message.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ChatError> {
        let mut r = Reader::new(bytes);
        if r.u8("message version")? != 1 {
            return Err(ChatError::Malformed("message version"));
        }
        let parent = option(&mut r, "parent")?;
        let text = String::from_utf8(r.long("text")?.to_vec())
            .map_err(|_| ChatError::Malformed("text"))?;
        r.end("message")?;
        check_text(&text)?;
        Ok(Self { parent, text })
    }
}

pub(crate) fn check_text(text: &str) -> Result<(), ChatError> {
    if text.trim().is_empty() || text.len() > MAX_TEXT {
        Err(ChatError::Malformed("text"))
    } else {
        Ok(())
    }
}

/// The id of a message: BLAKE2b-256 of `MESSAGE_ID_TAG ‖` the encrypted
/// message as sent. Sender and receivers hold the same bytes.
#[must_use]
pub fn message_id(wire: &[u8]) -> [u8; 32] {
    let mut m = Vec::with_capacity(wire.len().saturating_add(32));
    m.extend_from_slice(MESSAGE_ID_TAG);
    m.extend_from_slice(wire);
    blake2_256(&m)
}
