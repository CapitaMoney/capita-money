// SPDX-License-Identifier: AGPL-3.0-only
//! The chat certificate: the Capita key of an account signs the chat key of
//! one device for a few days (`chat.cert_days` of the rules file).
//!
//! Why a second key: MLS signs every message, and a fingerprint prompt per
//! message is impossible; a chat key that leaks never moves money. The
//! account key is asked once per device and period.

use capita_keys::encoding::blake2_256;
use capita_keys::types::{AccountBytes, Audience, DeviceKey, PasskeyProof, Proof};

use crate::codec::{put_short, Reader};
use crate::ChatError;

/// Domain tag of what the Capita key signs.
pub const CERT_TAG: &[u8] = b"capita/chat-cert/v1";
/// Version byte of the encoding.
const VERSION: u8 = 1;

/// What a certificate speaks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A verified person – the only kind accepted today.
    Person,
    /// Any other kind (a later bot kind, 2discuss AG). Refused.
    Other(u8),
}

impl Kind {
    fn byte(self) -> u8 {
        match self {
            Self::Person => 0,
            Self::Other(b) => b,
        }
    }

    fn from_byte(b: u8) -> Self {
        if b == 0 {
            Self::Person
        } else {
            Self::Other(b)
        }
    }
}

/// The statement the Capita key signs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatCert {
    /// The Capita account.
    pub account: AccountBytes,
    /// The key of the account that signs (must be on the chain for it).
    pub signer: DeviceKey,
    /// The device's chat key (Ed25519 public key, the MLS signature key).
    pub chat_key: [u8; 32],
    /// What the certificate speaks for.
    pub kind: Kind,
    /// First day it holds (days since 1970-01-01, UTC).
    pub from_day: u32,
    /// Last day it holds.
    pub until_day: u32,
}

impl ChatCert {
    /// The 32 bytes the Capita key signs: `BLAKE2b-256(CERT_TAG ‖ audience ‖
    /// account ‖ signer ‖ chat key ‖ kind ‖ from ‖ until)`.
    #[must_use]
    pub fn challenge(&self, audience: &Audience) -> [u8; 32] {
        let mut m = Vec::with_capacity(160);
        m.extend_from_slice(CERT_TAG);
        m.extend_from_slice(&audience.0);
        m.extend_from_slice(&self.account);
        m.extend_from_slice(&self.signer.to_bytes());
        m.extend_from_slice(&self.chat_key);
        m.push(self.kind.byte());
        m.extend_from_slice(&self.from_day.to_le_bytes());
        m.extend_from_slice(&self.until_day.to_le_bytes());
        blake2_256(&m)
    }
}

/// A certificate with the Capita key's signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedCert {
    /// The statement.
    pub cert: ChatCert,
    /// The signature of `cert.signer` over [`ChatCert::challenge`].
    pub proof: Proof,
}

impl SignedCert {
    /// Appends the certificate as bytes.
    pub(crate) fn write(&self, out: &mut Vec<u8>) -> Result<(), ChatError> {
        let c = &self.cert;
        out.push(VERSION);
        out.extend_from_slice(&c.account);
        out.extend_from_slice(&c.signer.to_bytes());
        out.extend_from_slice(&c.chat_key);
        out.push(c.kind.byte());
        out.extend_from_slice(&c.from_day.to_le_bytes());
        out.extend_from_slice(&c.until_day.to_le_bytes());
        match &self.proof {
            Proof::Ed25519(sig) => {
                out.push(0);
                out.extend_from_slice(sig);
            }
            Proof::Passkey(p) => {
                out.push(1);
                put_short(out, &p.authenticator_data, "passkey data")?;
                put_short(out, &p.client_data_json, "passkey client data")?;
                out.extend_from_slice(&p.signature);
            }
            Proof::Hardware(sig) => {
                out.push(2);
                out.extend_from_slice(sig);
            }
        }
        Ok(())
    }

    /// Reads a certificate from the front of `r`.
    pub(crate) fn read(r: &mut Reader<'_>) -> Result<Self, ChatError> {
        if r.u8("cert version")? != VERSION {
            return Err(ChatError::Malformed("cert version"));
        }
        let account = r.array("account")?;
        let signer = match r.u8("signer kind")? {
            0 => DeviceKey::Ed25519(r.array("signer")?),
            1 => DeviceKey::Passkey(r.array("signer")?),
            2 => DeviceKey::Hardware(r.array("signer")?),
            _ => return Err(ChatError::Malformed("signer kind")),
        };
        let chat_key = r.array("chat key")?;
        let kind = Kind::from_byte(r.u8("kind")?);
        let from_day = r.u32("from day")?;
        let until_day = r.u32("until day")?;
        let proof = match r.u8("proof kind")? {
            0 => Proof::Ed25519(r.array("proof")?),
            1 => Proof::Passkey(PasskeyProof {
                authenticator_data: r.short("passkey data")?.to_vec(),
                client_data_json: r.short("passkey client data")?.to_vec(),
                signature: r.array("proof")?,
            }),
            2 => Proof::Hardware(r.array("proof")?),
            _ => return Err(ChatError::Malformed("proof kind")),
        };
        Ok(Self {
            cert: ChatCert {
                account,
                signer,
                chat_key,
                kind,
                from_day,
                until_day,
            },
            proof,
        })
    }
}
