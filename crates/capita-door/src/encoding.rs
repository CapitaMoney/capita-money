//! The bytes a door signs, the text form of a ticket, and the transaction
//! that redeems a voucher.
//!
//! One fixed layout, written out field by field, so that a door in any
//! language can produce it. Integers in signed bytes are big-endian.
//!
//! ```text
//! voucher  "capita-voucher-v1"
//!          audience       32 bytes
//!          door            4 bytes
//!          payment        32 bytes
//!          account        32 bytes
//!          amount          8 bytes   cents
//!
//! ticket   "capita-ticket-v1"
//!          audience       32 bytes
//!          door            4 bytes
//!          serial         16 bytes
//!
//! payment  "capita-payment-v1"       hashed by the door, see `payment_bytes`
//!          order          the address of the order at the payment service
//! ```
//!
//! The text form of a ticket is lower-case hexadecimal of door (4 bytes),
//! serial (16) and signature (64): 168 characters. A person never types
//! it; it travels inside a link.

use alloc::string::String;
use alloc::vec::Vec;

use capita_core::types::{AccountId, Amount};

use crate::types::{Audience, DoorId, DoorSignature, PaymentRef, Ticket, TicketSerial, Voucher};

/// Tag of a signed voucher.
pub const VOUCHER_TAG: &[u8] = b"capita-voucher-v1";
/// Tag of a signed ticket.
pub const TICKET_TAG: &[u8] = b"capita-ticket-v1";
/// Tag of the bytes a payment reference is the hash of.
pub const PAYMENT_TAG: &[u8] = b"capita-payment-v1";
/// Length of a ticket in text form.
pub const TICKET_TEXT_LEN: usize = 168;

/// The bytes a door signs to make `voucher` count on the host `audience`.
#[must_use]
pub fn voucher_bytes(audience: &Audience, voucher: &Voucher) -> Vec<u8> {
    let mut out = Vec::with_capacity(160);
    out.extend_from_slice(VOUCHER_TAG);
    out.extend_from_slice(&audience.0);
    out.extend_from_slice(&voucher.door.0.to_be_bytes());
    out.extend_from_slice(&voucher.payment.0);
    out.extend_from_slice(&voucher.account.0);
    out.extend_from_slice(&voucher.amount.cents().to_be_bytes());
    out
}

/// The bytes a door signs to make `ticket` count for the identity services
/// of the host `audience`.
#[must_use]
pub fn ticket_bytes(audience: &Audience, ticket: &Ticket) -> Vec<u8> {
    let mut out = Vec::with_capacity(72);
    out.extend_from_slice(TICKET_TAG);
    out.extend_from_slice(&audience.0);
    out.extend_from_slice(&ticket.door.0.to_be_bytes());
    out.extend_from_slice(&ticket.serial.0);
    out
}

/// The bytes whose hash is the [`PaymentRef`] of an order: the tag and the
/// address of the order at the payment service (for GNU Taler: base address
/// of the merchant instance, then `orders/` and the order number). Two
/// doors at two merchants never collide.
#[must_use]
pub fn payment_bytes(order: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(PAYMENT_TAG.len().saturating_add(order.len()));
    out.extend_from_slice(PAYMENT_TAG);
    out.extend_from_slice(order.as_bytes());
    out
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn push_hex(out: &mut String, bytes: &[u8]) {
    for byte in bytes {
        for nibble in [byte >> 4, byte & 0x0f] {
            if let Some(&c) = HEX.get(usize::from(nibble)) {
                out.push(char::from(c));
            }
        }
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => c.checked_sub(b'0'),
        b'a'..=b'f' => c.checked_sub(b'a').and_then(|v| v.checked_add(10)),
        b'A'..=b'F' => c.checked_sub(b'A').and_then(|v| v.checked_add(10)),
        _ => None,
    }
}

/// A ticket and its signature as text.
#[must_use]
pub fn ticket_text(ticket: &Ticket, signature: &DoorSignature) -> String {
    let mut out = String::with_capacity(TICKET_TEXT_LEN);
    push_hex(&mut out, &ticket.door.0.to_be_bytes());
    push_hex(&mut out, &ticket.serial.0);
    push_hex(&mut out, &signature.0);
    out
}

/// A ticket from its text form; spaces around it are ignored, letters may
/// be upper or lower case. `None`: not a ticket.
#[must_use]
pub fn parse_ticket(text: &str) -> Option<(Ticket, DoorSignature)> {
    let text = text.trim().as_bytes();
    if text.len() != TICKET_TEXT_LEN {
        return None;
    }
    let mut bytes = [0u8; TICKET_TEXT_LEN / 2];
    for (out, pair) in bytes.iter_mut().zip(text.chunks_exact(2)) {
        let (high, low) = match pair {
            [high, low] => (nibble(*high)?, nibble(*low)?),
            _ => return None,
        };
        *out = (high << 4) | low;
    }
    let (door, rest) = bytes.split_first_chunk::<4>()?;
    let (serial, signature) = rest.split_first_chunk::<16>()?;
    let signature = <[u8; 64]>::try_from(signature).ok()?;
    let ticket = Ticket {
        door: DoorId(u32::from_be_bytes(*door)),
        serial: TicketSerial(*serial),
    };
    Some((ticket, DoorSignature(signature)))
}

/// The canonical text of a ticket: lower case, no spaces. An identity
/// service remembers used tickets in this form.
#[must_use]
pub fn canonical_ticket(text: &str) -> Option<String> {
    parse_ticket(text).map(|(ticket, signature)| ticket_text(&ticket, &signature))
}

/// SCALE compact encoding of a length.
fn push_compact(out: &mut Vec<u8>, n: usize) -> Option<()> {
    let n = u32::try_from(n).ok()?;
    if n < 1 << 6 {
        out.push(u8::try_from(n.checked_shl(2)?).ok()?);
    } else if n < 1 << 14 {
        let v = u16::try_from(n.checked_shl(2)? | 0b01).ok()?;
        out.extend_from_slice(&v.to_le_bytes());
    } else if n < 1 << 30 {
        out.extend_from_slice(&(n.checked_shl(2)? | 0b10).to_le_bytes());
    } else {
        return None;
    }
    Some(())
}

/// The call `redeem(voucher, signature)` of the door pallet, SCALE encoded:
/// index of the pallet in the runtime, index of the call, the voucher, the
/// signature. Integers little-endian, as SCALE has them.
#[must_use]
pub fn redeem_call(
    pallet_index: u8,
    call_index: u8,
    voucher: &Voucher,
    signature: &DoorSignature,
) -> Vec<u8> {
    let Voucher {
        door: DoorId(door),
        payment: PaymentRef(payment),
        account: AccountId(account),
        amount,
    } = voucher;
    let amount: &Amount = amount;
    let mut out = Vec::with_capacity(144);
    out.push(pallet_index);
    out.push(call_index);
    out.extend_from_slice(&door.to_le_bytes());
    out.extend_from_slice(payment);
    out.extend_from_slice(account);
    out.extend_from_slice(&amount.cents().to_le_bytes());
    out.extend_from_slice(&signature.0);
    out
}

/// A transaction without signer (a "bare" extrinsic, format version 4)
/// around an encoded call: what any node accepts through
/// `author_submitExtrinsic`. The voucher inside carries its own signature,
/// so nobody needs an account, or pays, to bring it to the chain.
#[must_use]
pub fn bare_extrinsic(call: &[u8]) -> Option<Vec<u8>> {
    let body = call.len().checked_add(1)?;
    let mut out = Vec::with_capacity(body.checked_add(4)?);
    push_compact(&mut out, body)?;
    out.push(0x04);
    out.extend_from_slice(call);
    Some(out)
}
