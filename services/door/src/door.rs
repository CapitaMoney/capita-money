//! The decisions of the door: what an order costs, when it counts as
//! paid, what is signed for it. No network and no clock: the merchant's
//! answer and the time come in as values.

use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use capita_core::types::{AccountId, Amount};
use capita_door::encoding::{payment_bytes, ticket_bytes, ticket_text, voucher_bytes};
use capita_door::types::{
    Audience, DoorId, DoorSignature, PaymentRef, Ticket, TicketSerial, Voucher,
};
use capita_rulefile::RuleSet;
use ed25519_dalek::{Signer, SigningKey};

use crate::merchant::{OrderStatus, TalerAmount};
use crate::store::{Entry, Kind, Order, Store};
use crate::{cents_text, hex};

/// The rule values the door follows, from the rules file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// Capita cents one euro buys (`door.capita_per_euro`).
    pub capita_per_euro: u64,
    /// Most euro cents one payment may bring in (`door.max_payment_eur`).
    pub max_payment: u64,
    /// Euro cents of one verification (`verification.price_eur`).
    pub ticket_price: u64,
}

impl Rules {
    /// The values of a rules file.
    #[must_use]
    pub fn from_set(set: &RuleSet) -> Self {
        Self {
            capita_per_euro: set.door.capita_per_euro.value.0.cents(),
            max_payment: set.door.max_payment_eur.value.0.cents(),
            ticket_price: set.verification.price_eur.value.0.cents(),
        }
    }
}

/// What the door is set up with.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The host the vouchers and tickets are for: the hash of the first
    /// block of the chain.
    pub audience: Audience,
    /// The number of this door on the list of the host.
    pub door: DoorId,
    /// The currency the door takes, as the payment service names it: `EUR`;
    /// on test systems `KUDOS`, counted like euro.
    pub currency: String,
    /// The rule values.
    pub rules: Rules,
}

/// Why the door refuses. No reason reveals anything about a payer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The request is not understood.
    Request(&'static str),
    /// Above the most one payment may bring in.
    TooMuch,
    /// No such order.
    Unknown,
    /// The payment service failed or answered nonsense.
    Merchant(String),
    /// The store failed.
    Store(String),
    /// Something of the system failed.
    System(&'static str),
}

impl Refusal {
    /// A short code for the interface.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Request(_) => "request",
            Self::TooMuch => "too-much",
            Self::Unknown => "unknown-order",
            Self::Merchant(_) => "payment-service",
            Self::Store(_) => "store",
            Self::System(_) => "system",
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(why) => write!(f, "request: {why}"),
            Self::TooMuch => f.write_str("above the most one payment may bring in"),
            Self::Unknown => f.write_str("no such order"),
            Self::Merchant(why) => write!(f, "payment service: {why}"),
            Self::Store(why) => write!(f, "store: {why}"),
            Self::System(why) => write!(f, "system: {why}"),
        }
    }
}

/// An order to open at the payment service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewOrder {
    /// Its number, chosen by the door.
    pub id: String,
    /// What it costs.
    pub amount: TalerAmount,
    /// What the wallet shows.
    pub summary: String,
}

/// Where an order stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Not paid yet. `pay_uri`: the link for the wallet; none while a
    /// wallet is paying.
    Unpaid {
        /// The link for the wallet.
        pay_uri: Option<String>,
    },
    /// Paid for Capita: the voucher.
    Voucher {
        /// What the chain redeems.
        voucher: Voucher,
        /// Signed by the door.
        signature: DoorSignature,
        /// Whether the door brought it to a node already.
        sent: bool,
    },
    /// Paid for verifications: one ticket each, in text form.
    Tickets(Vec<String>),
    /// The payment does not count: given back, or not what was ordered.
    Void(&'static str),
}

/// The door: settings, signing key, store.
pub struct Door<S: Store> {
    settings: Settings,
    key: SigningKey,
    store: S,
}

fn random<const N: usize>() -> Result<[u8; N], Refusal> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|_| Refusal::System("no random numbers"))?;
    Ok(out)
}

impl<S: Store> Door<S> {
    /// A door with its settings, the Ed25519 secret it signs with, and its
    /// store.
    #[must_use]
    pub fn new(settings: Settings, seed: &[u8; 32], store: S) -> Self {
        Self {
            settings,
            key: SigningKey::from_bytes(seed),
            store,
        }
    }

    /// The settings.
    #[must_use]
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The public key the door signs with.
    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    /// The store.
    #[must_use]
    pub fn store(&self) -> &S {
        &self.store
    }

    /// An order by its number.
    #[must_use]
    pub fn order(&self, id: &str) -> Option<Order> {
        self.store.order(id)
    }

    fn open(
        &mut self,
        kind: Kind,
        price: u64,
        summary: String,
        now: u64,
    ) -> Result<NewOrder, Refusal> {
        let id = format!("capita-{}", hex(&random::<12>()?));
        self.store
            .record(Entry::Opened {
                order: id.clone(),
                kind,
                price,
                at: now,
            })
            .map_err(Refusal::Store)?;
        Ok(NewOrder {
            id,
            amount: TalerAmount {
                currency: self.settings.currency.clone(),
                cents: price,
            },
            summary,
        })
    }

    /// Records an order of `euro` cents worth of Capita for `account`. The
    /// order is remembered before it is opened at the payment service.
    pub fn buy(&mut self, account: [u8; 32], euro: u64, now: u64) -> Result<NewOrder, Refusal> {
        let rules = self.settings.rules;
        if euro == 0 {
            return Err(Refusal::Request("an amount above zero"));
        }
        if euro > rules.max_payment {
            return Err(Refusal::TooMuch);
        }
        let capita = u128::from(euro)
            .checked_mul(u128::from(rules.capita_per_euro))
            .and_then(|c| c.checked_div(100))
            .and_then(|c| u64::try_from(c).ok())
            .filter(|c| *c > 0)
            .ok_or(Refusal::Request("too little for one cent of Capita"))?;
        let summary = format!("Capita Money: {} Capita", cents_text(capita));
        let kind = Kind::Capita {
            account: hex(&account),
            capita,
        };
        self.open(kind, euro, summary, now)
    }

    /// Records an order of `count` verifications, one ticket each – for a
    /// person, or for a sponsor who passes them on (diary 6).
    pub fn tickets(&mut self, count: u32, now: u64) -> Result<NewOrder, Refusal> {
        let rules = self.settings.rules;
        if count == 0 {
            return Err(Refusal::Request("at least one ticket"));
        }
        let price = rules
            .ticket_price
            .checked_mul(u64::from(count))
            .ok_or(Refusal::TooMuch)?;
        if price > rules.max_payment {
            return Err(Refusal::TooMuch);
        }
        let summary = if count == 1 {
            "Capita Money: 1 verification".to_owned()
        } else {
            format!("Capita Money: {count} verifications")
        };
        self.open(Kind::Tickets { count }, price, summary, now)
    }

    /// The payment reference of an order, from its address at the payment
    /// service.
    #[must_use]
    pub fn payment_ref(address: &str) -> PaymentRef {
        let mut hash = Blake2b::<U32>::new();
        hash.update(payment_bytes(address));
        PaymentRef(hash.finalize().into())
    }

    /// Decides where order `id` stands, given what the payment service
    /// reports (`status`) and the address of the order there. A paid order
    /// is signed for once and remembered; asked again, the door answers
    /// the same.
    pub fn settle(
        &mut self,
        id: &str,
        status: OrderStatus,
        address: &str,
        now: u64,
    ) -> Result<State, Refusal> {
        let order = self.store.order(id).ok_or(Refusal::Unknown)?;
        let (amount, refunded) = match status {
            OrderStatus::Unknown => {
                return Err(Refusal::Merchant("the order is unknown there".into()))
            }
            OrderStatus::Unpaid { pay_uri } => {
                return Ok(State::Unpaid {
                    pay_uri: Some(pay_uri),
                })
            }
            OrderStatus::Claimed => return Ok(State::Unpaid { pay_uri: None }),
            OrderStatus::Paid { amount, refunded } => (amount, refunded),
        };
        if refunded {
            return Ok(State::Void("given back"));
        }
        if amount.currency != self.settings.currency || amount.cents != order.price {
            return Ok(State::Void("not the amount ordered"));
        }
        match order.kind {
            Kind::Capita { account, capita } => {
                let account =
                    crate::unhex_n::<32>(&account).ok_or(Refusal::Store("account".into()))?;
                if order.paid_at.is_none() {
                    self.store
                        .record(Entry::Paid {
                            order: id.to_owned(),
                            tickets: Vec::new(),
                            at: now,
                        })
                        .map_err(Refusal::Store)?;
                }
                let voucher = Voucher {
                    door: self.settings.door,
                    payment: Self::payment_ref(address),
                    account: AccountId(account),
                    amount: Amount::from_cents(capita),
                };
                let bytes = voucher_bytes(&self.settings.audience, &voucher);
                let signature = DoorSignature(self.key.sign(&bytes).to_bytes());
                Ok(State::Voucher {
                    voucher,
                    signature,
                    sent: order.sent,
                })
            }
            Kind::Tickets { count } => {
                if !order.tickets.is_empty() {
                    return Ok(State::Tickets(order.tickets));
                }
                let mut tickets = Vec::new();
                for _ in 0..count {
                    let ticket = Ticket {
                        door: self.settings.door,
                        serial: TicketSerial(random::<16>()?),
                    };
                    let bytes = ticket_bytes(&self.settings.audience, &ticket);
                    let signature = DoorSignature(self.key.sign(&bytes).to_bytes());
                    tickets.push(ticket_text(&ticket, &signature));
                }
                self.store
                    .record(Entry::Paid {
                        order: id.to_owned(),
                        tickets: tickets.clone(),
                        at: now,
                    })
                    .map_err(Refusal::Store)?;
                Ok(State::Tickets(tickets))
            }
        }
    }

    /// Remembers that the voucher of order `id` reached a node.
    pub fn mark_sent(&mut self, id: &str, now: u64) -> Result<(), Refusal> {
        self.store
            .record(Entry::Sent {
                order: id.to_owned(),
                at: now,
            })
            .map_err(Refusal::Store)
    }
}
