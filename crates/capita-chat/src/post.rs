// SPDX-License-Identifier: AGPL-3.0-only
//! Public forum posts and their comments (diaspora*-style). Every app user
//! may read them, unverified ones too (rule 5), so they are signed, not
//! encrypted: encrypting for everybody protects nothing. A post carries the
//! writer's identity, so any reader checks it without asking a server.

use capita_keys::encoding::blake2_256;
use ed25519_dalek::{Signature, VerifyingKey};

use crate::chain::ChainView;
use crate::check::{Checked, Context, Identity};
use crate::codec::{option, put_long, put_option, Reader};
use crate::group::ChatDevice;
use crate::message::check_text;
use crate::ChatError;

/// Domain tag of what the chat key signs.
pub const POST_TAG: &[u8] = b"capita/forum-post/v1";

/// A post, or a comment when it has a parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Post {
    /// The forum (topic), by its id.
    pub forum: [u8; 32],
    /// The post this one comments on ([`SignedPost::id`]).
    pub parent: Option<[u8; 32]>,
    /// Day written (days since 1970-01-01, UTC).
    pub day: u32,
    /// The text.
    pub text: String,
}

impl Post {
    fn bytes(&self) -> Result<Vec<u8>, ChatError> {
        check_text(&self.text)?;
        let mut out = Vec::with_capacity(self.text.len().saturating_add(80));
        out.extend_from_slice(&self.forum);
        put_option(&mut out, self.parent.as_ref());
        out.extend_from_slice(&self.day.to_le_bytes());
        put_long(&mut out, self.text.as_bytes(), "text")?;
        Ok(out)
    }
}

/// A post with its writer's identity and the chat key's signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedPost {
    /// The post.
    pub post: Post,
    /// The writer's identity (certificate + name card).
    pub identity: Identity,
    /// Ed25519 signature of the chat key over `POST_TAG ‖ identity ‖ post`.
    pub signature: [u8; 64],
}

fn signed_message(identity: &Identity, post: &Post) -> Result<Vec<u8>, ChatError> {
    let mut m = Vec::with_capacity(512);
    m.extend_from_slice(POST_TAG);
    put_long(&mut m, &identity.to_bytes()?, "identity")?;
    m.extend_from_slice(&post.bytes()?);
    Ok(m)
}

impl SignedPost {
    /// Writes a post on `device`. Refused when the account may not write.
    ///
    /// # Errors
    /// No identity, no right to write, a text that is empty or too long.
    pub fn write<C: ChainView>(
        device: &ChatDevice,
        ctx: &Context<'_, C>,
        post: Post,
    ) -> Result<Self, ChatError> {
        let identity = device.identity().ok_or(ChatError::NoIdentity)?.clone();
        let me = ctx.identity(&identity, &device.chat_key()?)?;
        ctx.writer(&me)?;
        let signature = device.sign(&signed_message(&identity, &post)?)?;
        Ok(Self {
            post,
            identity,
            signature,
        })
    }

    /// The id of the post, for comments: BLAKE2b-256 of the signed bytes
    /// and the signature.
    ///
    /// # Errors
    /// A post that cannot be encoded.
    pub fn id(&self) -> Result<[u8; 32], ChatError> {
        let mut m = signed_message(&self.identity, &self.post)?;
        m.extend_from_slice(&self.signature);
        Ok(blake2_256(&m))
    }

    /// Checks a post as a reader does: identity, signature, and that the
    /// writer may write today. A post that fails is not shown.
    ///
    /// # Errors
    /// Why the post does not count.
    pub fn verify<C: ChainView>(&self, ctx: &Context<'_, C>) -> Result<Checked, ChatError> {
        let key = self.identity.cert.cert.chat_key;
        let writer = ctx.identity(&self.identity, &key)?;
        let vk = VerifyingKey::from_bytes(&key).map_err(|_| ChatError::BadSignature)?;
        vk.verify_strict(
            &signed_message(&self.identity, &self.post)?,
            &Signature::from_bytes(&self.signature),
        )
        .map_err(|_| ChatError::BadSignature)?;
        ctx.writer(&writer)?;
        Ok(writer)
    }

    /// The post as bytes, to spread to readers.
    ///
    /// # Errors
    /// A post that cannot be encoded.
    pub fn to_bytes(&self) -> Result<Vec<u8>, ChatError> {
        let mut out = Vec::with_capacity(512);
        out.push(1);
        put_long(&mut out, &self.identity.to_bytes()?, "identity")?;
        out.extend_from_slice(&self.post.bytes()?);
        out.extend_from_slice(&self.signature);
        Ok(out)
    }

    /// Reads what [`SignedPost::to_bytes`] wrote.
    ///
    /// # Errors
    /// Bytes that are no post.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ChatError> {
        let mut r = Reader::new(bytes);
        if r.u8("post version")? != 1 {
            return Err(ChatError::Malformed("post version"));
        }
        let identity = Identity::from_bytes(r.long("identity")?)?;
        let forum = r.array("forum")?;
        let parent = option(&mut r, "parent")?;
        let day = r.u32("day")?;
        let text = String::from_utf8(r.long("text")?.to_vec())
            .map_err(|_| ChatError::Malformed("text"))?;
        let signature = r.array("signature")?;
        r.end("post")?;
        Ok(Self {
            post: Post {
                forum,
                parent,
                day,
                text,
            },
            identity,
            signature,
        })
    }
}
