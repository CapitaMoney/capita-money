// Capita Money – capita-door
// SPDX-License-Identifier: AGPL-3.0-only

//! Vouchers and tickets: signed, checked, refused.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use capita_core::types::{AccountId, Amount};
use capita_door::check::{check_ticket, check_voucher, DoorError, Verify};
use capita_door::encoding::{
    bare_extrinsic, canonical_ticket, parse_ticket, payment_bytes, redeem_call, ticket_bytes,
    ticket_text, voucher_bytes, TICKET_TEXT_LEN,
};
use capita_door::types::{
    Audience, Door, DoorId, DoorSignature, DoorStatus, PaymentRef, Ticket, TicketSerial, Voucher,
};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};

struct Dalek;
impl Verify for Dalek {
    fn ed25519(key: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> bool {
        let Ok(key) = VerifyingKey::from_bytes(key) else {
            return false;
        };
        key.verify(message, &ed25519_dalek::Signature::from_bytes(signature))
            .is_ok()
    }
}

const HOST: Audience = Audience([7; 32]);

fn key() -> SigningKey {
    SigningKey::from_bytes(b"capita-money development door!!!")
}

fn door(max: u64) -> Door {
    Door {
        key: key().verifying_key().to_bytes(),
        max_amount: Amount::from_cents(max),
        status: DoorStatus::Active,
    }
}

fn voucher(cents: u64) -> Voucher {
    Voucher {
        door: DoorId(0),
        payment: PaymentRef([1; 32]),
        account: AccountId([2; 32]),
        amount: Amount::from_cents(cents),
    }
}

fn sign(bytes: &[u8]) -> DoorSignature {
    DoorSignature(key().sign(bytes).to_bytes())
}

#[test]
fn a_signed_voucher_counts() {
    let v = voucher(500);
    let s = sign(&voucher_bytes(&HOST, &v));
    assert_eq!(
        check_voucher::<Dalek>(&HOST, Some(&door(10_000)), &v, &s),
        Ok(())
    );
}

#[test]
fn a_voucher_counts_on_its_host_only_and_unchanged() {
    let v = voucher(500);
    let s = sign(&voucher_bytes(&HOST, &v));
    let d = door(10_000);
    let other = Audience([8; 32]);
    assert_eq!(
        check_voucher::<Dalek>(&other, Some(&d), &v, &s),
        Err(DoorError::BadSignature)
    );
    let more = Voucher {
        amount: Amount::from_cents(600),
        ..v
    };
    assert_eq!(
        check_voucher::<Dalek>(&HOST, Some(&d), &more, &s),
        Err(DoorError::BadSignature)
    );
    let elsewhere = Voucher {
        account: AccountId([3; 32]),
        ..v
    };
    assert_eq!(
        check_voucher::<Dalek>(&HOST, Some(&d), &elsewhere, &s),
        Err(DoorError::BadSignature)
    );
}

#[test]
fn limits_of_the_door_hold() {
    let v = voucher(10_001);
    let s = sign(&voucher_bytes(&HOST, &v));
    assert_eq!(
        check_voucher::<Dalek>(&HOST, Some(&door(10_000)), &v, &s),
        Err(DoorError::AmountTooHigh)
    );
    let zero = voucher(0);
    let s = sign(&voucher_bytes(&HOST, &zero));
    assert_eq!(
        check_voucher::<Dalek>(&HOST, Some(&door(10_000)), &zero, &s),
        Err(DoorError::ZeroAmount)
    );
    let v = voucher(1);
    let s = sign(&voucher_bytes(&HOST, &v));
    let suspended = Door {
        status: DoorStatus::Suspended,
        ..door(10_000)
    };
    assert_eq!(
        check_voucher::<Dalek>(&HOST, Some(&suspended), &v, &s),
        Err(DoorError::DoorSuspended)
    );
    assert_eq!(
        check_voucher::<Dalek>(&HOST, None, &v, &s),
        Err(DoorError::UnknownDoor)
    );
}

#[test]
fn the_voucher_layout_is_fixed() {
    let bytes = voucher_bytes(&HOST, &voucher(0x0102));
    assert_eq!(&bytes[..17], b"capita-voucher-v1");
    assert_eq!(bytes.len(), 17 + 32 + 4 + 32 + 32 + 8);
    assert_eq!(&bytes[bytes.len() - 2..], &[1, 2]);
}

#[test]
fn tickets_go_through_text_and_back() {
    let t = Ticket {
        door: DoorId(3),
        serial: TicketSerial([9; 16]),
    };
    let s = sign(&ticket_bytes(&HOST, &t));
    let text = ticket_text(&t, &s);
    assert_eq!(text.len(), TICKET_TEXT_LEN);
    assert_eq!(parse_ticket(&text), Some((t, s)));
    let shouted = format!("  {}\n", text.to_uppercase());
    assert_eq!(canonical_ticket(&shouted).as_deref(), Some(text.as_str()));
    let doors = [(DoorId(3), key().verifying_key().to_bytes())];
    assert_eq!(check_ticket::<Dalek>(&HOST, &doors, &shouted), Ok(t));
}

#[test]
fn false_tickets_are_refused() {
    let t = Ticket {
        door: DoorId(3),
        serial: TicketSerial([9; 16]),
    };
    let s = sign(&ticket_bytes(&HOST, &t));
    let text = ticket_text(&t, &s);
    let doors = [(DoorId(3), key().verifying_key().to_bytes())];
    assert_eq!(
        check_ticket::<Dalek>(&HOST, &[], &text),
        Err(DoorError::UnknownDoor)
    );
    assert_eq!(
        check_ticket::<Dalek>(&Audience([0; 32]), &doors, &text),
        Err(DoorError::BadSignature)
    );
    let mut forged = text.clone().into_bytes();
    forged[20] = if forged[20] == b'0' { b'1' } else { b'0' };
    let forged = String::from_utf8(forged).unwrap();
    assert_eq!(
        check_ticket::<Dalek>(&HOST, &doors, &forged),
        Err(DoorError::BadSignature)
    );
    assert_eq!(
        check_ticket::<Dalek>(&HOST, &doors, "hello"),
        Err(DoorError::MalformedTicket)
    );
    assert_eq!(
        check_ticket::<Dalek>(&HOST, &doors, &text[..166]),
        Err(DoorError::MalformedTicket)
    );
    let mut bad = text.into_bytes();
    bad[0] = b'x';
    assert_eq!(
        check_ticket::<Dalek>(&HOST, &doors, &String::from_utf8(bad).unwrap()),
        Err(DoorError::MalformedTicket)
    );
}

#[test]
fn payments_of_two_orders_differ() {
    assert_ne!(
        payment_bytes("https://a/orders/1"),
        payment_bytes("https://a/orders/2")
    );
    assert!(payment_bytes("x").starts_with(b"capita-payment-v1"));
}

#[test]
fn the_redeem_transaction_has_the_scale_layout() {
    let v = voucher(500);
    let s = DoorSignature([5; 64]);
    let call = redeem_call(23, 0, &v, &s);
    assert_eq!(call.len(), 2 + 4 + 32 + 32 + 8 + 64);
    assert_eq!(&call[..6], &[23, 0, 0, 0, 0, 0]);
    assert_eq!(&call[70..78], &500u64.to_le_bytes());
    let ext = bare_extrinsic(&call).unwrap();
    // 143 bytes of body: two-byte compact length (143 << 2 | 1), version 4
    assert_eq!(&ext[..3], &[0x3d, 0x02, 0x04]);
    assert_eq!(&ext[3..], &call[..]);
    let small = bare_extrinsic(&[1, 2]).unwrap();
    assert_eq!(small, vec![12, 4, 1, 2]);
}

#[cfg(feature = "scale")]
#[test]
fn the_hand_layout_equals_the_codec() {
    use codec::Encode;
    let v = voucher(500);
    let s = DoorSignature([5; 64]);
    let mut expected = vec![23u8, 0];
    expected.extend((v, s).encode());
    assert_eq!(redeem_call(23, 0, &v, &s), expected);
}
