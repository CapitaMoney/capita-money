//! Whether a voucher or a ticket counts.
//!
//! What a host keeps – the list of doors, the payments already redeemed,
//! the tickets already used – stays with the host. These functions decide
//! the rest, the same way everywhere.

use core::fmt;

use crate::encoding::{parse_ticket, ticket_bytes, voucher_bytes};
use crate::types::{Audience, Door, DoorId, DoorSignature, DoorStatus, Ticket, Voucher};

/// The signature check of the host.
pub trait Verify {
    /// Whether `signature` is an Ed25519 signature of `message` by `key`.
    fn ed25519(key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool;
}

/// Reasons for refusing. None of them reveals anything about the payer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DoorError {
    /// No door with this number is on the list.
    UnknownDoor,
    /// The door is suspended.
    DoorSuspended,
    /// The voucher is for nothing.
    ZeroAmount,
    /// The voucher is for more than the door may sign for.
    AmountTooHigh,
    /// No key of the door made this signature over these bytes – a wrong
    /// key, another host (audience), or content changed after signing.
    BadSignature,
    /// The text is not a ticket.
    MalformedTicket,
}

impl fmt::Display for DoorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownDoor => "unknown door",
            Self::DoorSuspended => "door suspended",
            Self::ZeroAmount => "voucher for nothing",
            Self::AmountTooHigh => "voucher above the limit of the door",
            Self::BadSignature => "bad signature",
            Self::MalformedTicket => "not a ticket",
        })
    }
}

/// Checks a voucher against the entry of its door (`None`: not on the
/// list). Whether its payment was redeemed before is for the host to know.
pub fn check_voucher<V: Verify>(
    audience: &Audience,
    door: Option<&Door>,
    voucher: &Voucher,
    signature: &DoorSignature,
) -> Result<(), DoorError> {
    let door = door.ok_or(DoorError::UnknownDoor)?;
    if door.status != DoorStatus::Active {
        return Err(DoorError::DoorSuspended);
    }
    if voucher.amount.is_zero() {
        return Err(DoorError::ZeroAmount);
    }
    if voucher.amount > door.max_amount {
        return Err(DoorError::AmountTooHigh);
    }
    if !V::ed25519(&door.key, &voucher_bytes(audience, voucher), &signature.0) {
        return Err(DoorError::BadSignature);
    }
    Ok(())
}

/// Checks a ticket in text form against the doors an identity service
/// accepts tickets from: number and public key of each. Whether it was used
/// before is for the service to know.
pub fn check_ticket<V: Verify>(
    audience: &Audience,
    doors: &[(DoorId, [u8; 32])],
    text: &str,
) -> Result<Ticket, DoorError> {
    let (ticket, signature) = parse_ticket(text).ok_or(DoorError::MalformedTicket)?;
    let key = doors
        .iter()
        .find(|(id, _)| *id == ticket.door)
        .map(|(_, key)| key)
        .ok_or(DoorError::UnknownDoor)?;
    if !V::ed25519(key, &ticket_bytes(audience, &ticket), &signature.0) {
        return Err(DoorError::BadSignature);
    }
    Ok(ticket)
}
