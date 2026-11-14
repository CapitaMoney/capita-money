//! The data model: doors, vouchers, tickets.

use capita_core::types::{AccountId, Amount};

macro_rules! small {
    ($(#[$meta:meta])* $vis:vis struct $name:ident ($inner:ty);) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
        $vis struct $name(pub $inner);
    };
}

macro_rules! data {
    ($(#[$meta:meta])* $vis:vis $kind:ident $name:ident $body:tt $(;)?) => {
        $(#[$meta])*
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
        $vis $kind $name $body
    };
}

small! {
    /// Number of a door on the list of the host.
    pub struct DoorId(u32);
}

small! {
    /// The host a voucher or ticket is made out to: the hash of the first
    /// block of a chain. A statement for one host counts on no other.
    pub struct Audience([u8; 32]);
}

small! {
    /// One payment, as the chain knows it: a hash the door computes from the
    /// place of the order at the payment service ([`crate::encoding::payment_ref`]).
    /// Each payment buys once.
    pub struct PaymentRef([u8; 32]);
}

small! {
    /// Number of a ticket, chosen at random by the door.
    pub struct TicketSerial([u8; 16]);
}

/// An Ed25519 signature of a door. No serde form: it travels as
/// hexadecimal text in the services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
pub struct DoorSignature(pub [u8; 64]);

data! {
    /// Whether a door counts.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum DoorStatus {
        /// Its vouchers count.
        Active,
        /// Its vouchers do not count, for now. What it signed before and was
        /// redeemed stays.
        Suspended,
    }
}

data! {
    /// An entry of the public list of doors.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Door {
        /// The Ed25519 public key it signs with.
        pub key: [u8; 32],
        /// Most Capita one voucher may mint: a brake if the door is broken
        /// into.
        pub max_amount: Amount,
        /// Whether it counts.
        pub status: DoorStatus,
    }
}

data! {
    /// "This payment bought this much Capita for this account." Signed by a
    /// door, redeemed on the chain once.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Voucher {
        /// The door that took the payment.
        pub door: DoorId,
        /// The payment.
        pub payment: PaymentRef,
        /// The account that receives the Capita.
        pub account: AccountId,
        /// How much Capita, in cents.
        pub amount: Amount,
    }
}

data! {
    /// "This payment paid one verification." Signed by a door, accepted by
    /// an identity service once.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Ticket {
        /// The door that took the payment.
        pub door: DoorId,
        /// Its number.
        pub serial: TicketSerial,
    }
}
