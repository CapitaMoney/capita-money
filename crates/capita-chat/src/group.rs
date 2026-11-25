// SPDX-License-Identifier: AGPL-3.0-only
//! Direct and group chats, end-to-end encrypted with MLS (RFC 9420,
//! OpenMLS). One MLS leaf per device; its credential is the device's
//! [`Identity`] (chat certificate + name card).
//!
//! The checks of the verified chat sit around OpenMLS, in every app:
//! - adding a member: identity holds and the member may write (unverified
//!   members are in no private chat – rule 5);
//! - joining: every member of the group has an identity that holds;
//! - a commit of another member: every new or changed leaf holds, every
//!   added member may write – else the commit is not taken;
//! - a message: the sender's identity holds and the sender may write
//!   *today* – a ban or a closed account takes effect at once, without any
//!   server.

use capita_keys::types::AccountBytes;
use openmls::prelude::tls_codec::{Deserialize as _, Serialize as _};
use openmls::prelude::{
    BasicCredential, Ciphersuite, CredentialWithKey, KeyPackage, KeyPackageIn, LeafNode, MlsGroup,
    MlsGroupCreateConfig, MlsGroupJoinConfig, MlsMessageBodyIn, MlsMessageIn, OpenMlsProvider as _,
    ProcessedMessageContent, ProtocolVersion, Sender, StagedWelcome,
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;

use crate::chain::ChainView;
use crate::check::{Checked, Context, Identity};
use crate::message::{message_id, ChatMessage};
use crate::ChatError;

/// The one MLS cipher suite: X25519, AES-128-GCM, SHA-256, Ed25519 (the
/// mandatory suite of RFC 9420). Vermutung (Claude, ungeprüft): a
/// post-quantum suite later, once OpenMLS ships one as stable.
pub const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519;

fn mls<E: core::fmt::Debug>(e: E) -> ChatError {
    ChatError::Mls(format!("{e:?}"))
}

/// One device of a member: its MLS state, its chat key, its identity.
pub struct ChatDevice {
    provider: OpenMlsRustCrypto,
    signer: SignatureKeyPair,
    identity: Option<Identity>,
}

impl ChatDevice {
    /// A device with a fresh chat key and no identity yet.
    ///
    /// # Errors
    /// The key could not be made or stored.
    pub fn new() -> Result<Self, ChatError> {
        let provider = OpenMlsRustCrypto::default();
        let signer = SignatureKeyPair::new(CIPHERSUITE.signature_algorithm()).map_err(mls)?;
        signer.store(provider.storage()).map_err(mls)?;
        Ok(Self {
            provider,
            signer,
            identity: None,
        })
    }

    /// The chat key the Capita key is to sign.
    ///
    /// # Errors
    /// A key of another length (never with [`CIPHERSUITE`]).
    pub fn chat_key(&self) -> Result<[u8; 32], ChatError> {
        <[u8; 32]>::try_from(self.signer.public()).map_err(|_| ChatError::Malformed("chat key"))
    }

    /// Signs bytes with the chat key (forum posts).
    pub(crate) fn sign(&self, payload: &[u8]) -> Result<[u8; 64], ChatError> {
        use openmls_traits::signatures::Signer as _;
        let sig = self.signer.sign(payload).map_err(mls)?;
        <[u8; 64]>::try_from(sig.as_slice()).map_err(|_| ChatError::Malformed("signature"))
    }

    /// Sets the identity: the certificate signed by the Capita key, and the
    /// name card.
    pub fn set_identity(&mut self, identity: Identity) {
        self.identity = Some(identity);
    }

    /// The identity, if set.
    #[must_use]
    pub fn identity(&self) -> Option<&Identity> {
        self.identity.as_ref()
    }

    fn credential(&self) -> Result<CredentialWithKey, ChatError> {
        let identity = self.identity.as_ref().ok_or(ChatError::NoIdentity)?;
        Ok(CredentialWithKey {
            credential: BasicCredential::new(identity.to_bytes()?).into(),
            signature_key: self.signer.to_public_vec().into(),
        })
    }

    /// Checks the own identity and the right to write (the app greys out
    /// what this refuses).
    fn own_writer<C: ChainView>(&self, ctx: &Context<'_, C>) -> Result<Checked, ChatError> {
        let identity = self.identity.as_ref().ok_or(ChatError::NoIdentity)?;
        let me = ctx.identity(identity, self.signer.public())?;
        ctx.writer(&me)?;
        Ok(me)
    }

    /// A key package: what others need to add this device to a chat.
    ///
    /// # Errors
    /// No identity, or an MLS error.
    pub fn key_package(&self) -> Result<Vec<u8>, ChatError> {
        let bundle = KeyPackage::builder()
            .build(
                CIPHERSUITE,
                &self.provider,
                &self.signer,
                self.credential()?,
            )
            .map_err(mls)?;
        bundle.key_package().tls_serialize_detached().map_err(mls)
    }

    /// Starts a chat with this device as its only member and admin.
    ///
    /// # Errors
    /// The own identity does not hold or may not write; an MLS error.
    pub fn create_group<C: ChainView>(&self, ctx: &Context<'_, C>) -> Result<ChatGroup, ChatError> {
        self.own_writer(ctx)?;
        let config = MlsGroupCreateConfig::builder()
            .ciphersuite(CIPHERSUITE)
            .use_ratchet_tree_extension(true)
            .build();
        let group = MlsGroup::new(&self.provider, &self.signer, &config, self.credential()?)
            .map_err(mls)?;
        Ok(ChatGroup { group })
    }

    /// Joins a chat from the welcome another member sent. Every member of
    /// the group is checked first.
    ///
    /// # Errors
    /// A member whose identity does not hold; the own right to write; an
    /// MLS error.
    pub fn join<C: ChainView>(
        &self,
        ctx: &Context<'_, C>,
        welcome: &[u8],
    ) -> Result<ChatGroup, ChatError> {
        self.own_writer(ctx)?;
        let message = MlsMessageIn::tls_deserialize_exact(welcome).map_err(mls)?;
        let MlsMessageBodyIn::Welcome(welcome) = message.extract() else {
            return Err(ChatError::Malformed("not a welcome"));
        };
        let config = MlsGroupJoinConfig::builder()
            .use_ratchet_tree_extension(true)
            .build();
        let staged =
            StagedWelcome::new_from_welcome(&self.provider, &config, welcome, None).map_err(mls)?;
        for member in staged.members() {
            ctx.identity_bytes(
                member.credential.serialized_content(),
                &member.signature_key,
            )?;
        }
        let group = staged.into_group(&self.provider).map_err(mls)?;
        Ok(ChatGroup { group })
    }
}

/// What a received message turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Received {
    /// A chat message of a member who may write.
    Message(Incoming),
    /// A change of the group (members added, removed, keys renewed), taken.
    Commit,
    /// Something else MLS handles (a proposal); nothing to show.
    Other,
}

/// A chat message as the app shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incoming {
    /// Its id ([`message_id`]), for comments.
    pub id: [u8; 32],
    /// The sender's account.
    pub from: AccountBytes,
    /// The sender's name (name card).
    pub name: Option<String>,
    /// The message.
    pub message: ChatMessage,
}

/// What [`ChatGroup::send`] gives: the bytes to send and their id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    /// The id ([`message_id`]).
    pub id: [u8; 32],
    /// The encrypted message, to every member.
    pub wire: Vec<u8>,
}

/// What [`ChatGroup::add`] gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    /// The commit, to the members already in the chat.
    pub commit: Vec<u8>,
    /// The welcome, to the new member.
    pub welcome: Vec<u8>,
}

/// One chat as one device sees it.
pub struct ChatGroup {
    group: MlsGroup,
}

fn check_leaf<C: ChainView>(ctx: &Context<'_, C>, leaf: &LeafNode) -> Result<Checked, ChatError> {
    ctx.identity_bytes(
        leaf.credential().serialized_content(),
        leaf.signature_key().as_slice(),
    )
}

impl ChatGroup {
    /// The accounts in the chat, one per device.
    #[must_use]
    pub fn members(&self) -> Vec<AccountBytes> {
        self.group
            .members()
            .filter_map(|m| Identity::from_bytes(m.credential.serialized_content()).ok())
            .map(|i| i.cert.cert.account)
            .collect()
    }

    /// Adds a device of a member, from its key package. The member's
    /// identity must hold and the member must be allowed to write.
    ///
    /// # Errors
    /// Why the member may not be added; an MLS error.
    pub fn add<C: ChainView>(
        &mut self,
        device: &ChatDevice,
        ctx: &Context<'_, C>,
        key_package: &[u8],
    ) -> Result<Added, ChatError> {
        device.own_writer(ctx)?;
        let package = KeyPackageIn::tls_deserialize_exact(key_package)
            .map_err(mls)?
            .validate(device.provider.crypto(), ProtocolVersion::Mls10)
            .map_err(mls)?;
        let member = check_leaf(ctx, package.leaf_node())?;
        ctx.writer(&member)?;
        let (commit, welcome, _info) = self
            .group
            .add_members(
                &device.provider,
                &device.signer,
                core::slice::from_ref(&package),
            )
            .map_err(mls)?;
        self.group
            .merge_pending_commit(&device.provider)
            .map_err(mls)?;
        Ok(Added {
            commit: commit.to_bytes().map_err(mls)?,
            welcome: welcome.to_bytes().map_err(mls)?,
        })
    }

    /// Encrypts a message to all members. Refused when the own account may
    /// not write today.
    ///
    /// # Errors
    /// No right to write; a message that is too long; an MLS error.
    pub fn send<C: ChainView>(
        &mut self,
        device: &ChatDevice,
        ctx: &Context<'_, C>,
        message: &ChatMessage,
    ) -> Result<Outgoing, ChatError> {
        device.own_writer(ctx)?;
        let out = self
            .group
            .create_message(&device.provider, &device.signer, &message.to_bytes()?)
            .map_err(mls)?;
        let wire = out.to_bytes().map_err(mls)?;
        Ok(Outgoing {
            id: message_id(&wire),
            wire,
        })
    }

    /// Takes a message from the network: decrypts it and checks the
    /// sender, or checks and takes a commit.
    ///
    /// # Errors
    /// [`ChatError::Denied`] for a sender who may not write (the message is
    /// dropped); an identity that does not hold; an MLS error.
    pub fn receive<C: ChainView>(
        &mut self,
        device: &ChatDevice,
        ctx: &Context<'_, C>,
        wire: &[u8],
    ) -> Result<Received, ChatError> {
        let message = MlsMessageIn::tls_deserialize_exact(wire).map_err(mls)?;
        let protocol = message
            .try_into_protocol_message()
            .map_err(|_| ChatError::Malformed("not a group message"))?;
        let processed = self
            .group
            .process_message(&device.provider, protocol)
            .map_err(mls)?;
        let sender = match processed.sender() {
            Sender::Member(index) => self.group.member_at(*index),
            _ => None,
        };
        match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(app) => {
                let sender = sender.ok_or(ChatError::Malformed("sender"))?;
                let from = ctx.identity_bytes(
                    sender.credential.serialized_content(),
                    &sender.signature_key,
                )?;
                ctx.writer(&from)?;
                Ok(Received::Message(Incoming {
                    id: message_id(wire),
                    from: from.account,
                    name: from.name,
                    message: ChatMessage::from_bytes(&app.into_bytes())?,
                }))
            }
            ProcessedMessageContent::StagedCommitMessage(staged) => {
                for add in staged.add_proposals() {
                    let member = check_leaf(ctx, add.add_proposal().key_package().leaf_node())?;
                    ctx.writer(&member)?;
                }
                for update in staged.update_proposals() {
                    check_leaf(ctx, update.update_proposal().leaf_node())?;
                }
                if let Some(leaf) = staged.update_path_leaf_node() {
                    check_leaf(ctx, leaf)?;
                }
                self.group
                    .merge_staged_commit(&device.provider, *staged)
                    .map_err(mls)?;
                Ok(Received::Commit)
            }
            _ => Ok(Received::Other),
        }
    }
}
