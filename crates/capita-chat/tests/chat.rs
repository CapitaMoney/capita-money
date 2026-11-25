// SPDX-License-Identifier: AGPL-3.0-only
//! The verified chat end to end, with test data only: Capita-key login,
//! one account per person, end-to-end encryption, rights, posts, quota.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    missing_docs
)]

use capita_chat::cert::{ChatCert, Kind, SignedCert};
use capita_chat::chain::{ChainView, MemoryChain};
use capita_chat::check::Identity;
use capita_chat::group::{ChatDevice, ChatGroup, Received};
use capita_chat::message::ChatMessage;
use capita_chat::name::NameCard;
use capita_chat::post::{Post, SignedPost};
use capita_chat::quota::{Limits, Quota, What};
use capita_chat::{ChatError, Context, Denied};
use capita_keys::encoding::account_of;
use capita_keys::types::{AccountBytes, Audience, DeviceKey, Proof};
use ed25519_dalek::{Signer as _, SigningKey};

const TODAY: u32 = 20_500;
const DAYS: u32 = 30;
const AUDIENCE: Audience = Audience([7; 32]);

fn ctx<'a>(
    chain: &'a MemoryChain,
    issuers: &'a [[u8; 32]],
    today: u32,
) -> Context<'a, MemoryChain> {
    Context {
        chain,
        audience: AUDIENCE,
        relying_parties: &[],
        origins: &[],
        issuers,
        today,
        cert_days: DAYS,
    }
}

fn cert(
    key: &SigningKey,
    account: AccountBytes,
    chat_key: [u8; 32],
    kind: Kind,
    from: u32,
    until: u32,
) -> SignedCert {
    let cert = ChatCert {
        account,
        signer: DeviceKey::Ed25519(key.verifying_key().to_bytes()),
        chat_key,
        kind,
        from_day: from,
        until_day: until,
    };
    let proof = Proof::Ed25519(key.sign(&cert.challenge(&AUDIENCE)).to_bytes());
    SignedCert { cert, proof }
}

struct Member {
    key: SigningKey,
    account: AccountBytes,
    device: ChatDevice,
}

struct World {
    chain: MemoryChain,
    issuer: SigningKey,
    issuers: Vec<[u8; 32]>,
}

impl World {
    fn new() -> Self {
        let issuer = SigningKey::from_bytes(&[200; 32]);
        let issuers = vec![issuer.verifying_key().to_bytes()];
        Self {
            chain: MemoryChain::new(),
            issuer,
            issuers,
        }
    }

    /// Opens an account; `person` = verified as that person.
    fn member(&mut self, seed: u8, person: Option<u8>, name: &str) -> Member {
        let key = SigningKey::from_bytes(&[seed; 32]);
        let device_key = DeviceKey::Ed25519(key.verifying_key().to_bytes());
        let account = account_of(&device_key);
        self.chain.open(account, device_key);
        if let Some(p) = person {
            self.chain.verify(&account, [p; 32]);
        }
        let mut device = ChatDevice::new().unwrap();
        let signed = cert(
            &key,
            account,
            device.chat_key().unwrap(),
            Kind::Person,
            TODAY,
            TODAY + DAYS - 1,
        );
        let name = person.map(|_| NameCard::sign(&self.issuer, account, name, TODAY).unwrap());
        device.set_identity(Identity { cert: signed, name });
        Member {
            key,
            account,
            device,
        }
    }

    fn ctx(&self) -> Context<'_, MemoryChain> {
        ctx(&self.chain, &self.issuers, TODAY)
    }
}

/// Alice starts a chat and adds Bob; Bob joins.
fn pair(w: &World, alice: &Member, bob: &Member) -> (ChatGroup, ChatGroup) {
    let c = w.ctx();
    let mut a = alice.device.create_group(&c).unwrap();
    let added = a
        .add(&alice.device, &c, &bob.device.key_package().unwrap())
        .unwrap();
    let b = bob.device.join(&c, &added.welcome).unwrap();
    (a, b)
}

#[test]
fn two_verified_members_chat_end_to_end_with_comments() {
    let mut w = World::new();
    let alice = w.member(1, Some(1), "Erika Mustermann");
    let bob = w.member(2, Some(2), "Max Mustermann");
    let (mut a, mut b) = pair(&w, &alice, &bob);
    assert_eq!(a.members(), vec![alice.account, bob.account]);
    assert_eq!(b.members(), a.members());

    let c = w.ctx();
    let out = a
        .send(&alice.device, &c, &ChatMessage::new("Hello Max"))
        .unwrap();
    assert!(
        !out.wire.windows(9).any(|x| x == b"Hello Max"),
        "the text travels encrypted"
    );
    let Received::Message(m) = b.receive(&bob.device, &c, &out.wire).unwrap() else {
        panic!("a message")
    };
    assert_eq!(m.message.text, "Hello Max");
    assert_eq!(m.from, alice.account);
    assert_eq!(m.name.as_deref(), Some("Erika Mustermann"));
    assert_eq!(m.id, out.id);

    // A comment under Alice's message.
    let reply = b
        .send(&bob.device, &c, &ChatMessage::comment(m.id, "Hello Erika"))
        .unwrap();
    let Received::Message(r) = a.receive(&alice.device, &c, &reply.wire).unwrap() else {
        panic!("a message")
    };
    assert_eq!(r.message.parent, Some(out.id));
    assert_eq!(r.name.as_deref(), Some("Max Mustermann"));
}

#[test]
fn unverified_members_are_in_no_private_chat() {
    let mut w = World::new();
    let alice = w.member(1, Some(1), "Erika Mustermann");
    let carol = w.member(3, None, "");
    let c = w.ctx();
    let mut a = alice.device.create_group(&c).unwrap();
    let kp = carol.device.key_package().unwrap();
    assert_eq!(
        a.add(&alice.device, &c, &kp).unwrap_err(),
        ChatError::Denied(Denied::NotVerified)
    );
    assert_eq!(
        carol.device.create_group(&c).err(),
        Some(ChatError::Denied(Denied::NotVerified))
    );
}

#[test]
fn a_second_account_of_the_same_person_never_writes() {
    let mut w = World::new();
    let alice = w.member(1, Some(1), "Erika Mustermann");
    // Same person, younger account (rule 21: the younger is the double).
    let again = w.member(9, Some(1), "Erika Mustermann");
    let c = w.ctx();
    let mut a = alice.device.create_group(&c).unwrap();
    assert_eq!(
        a.add(&alice.device, &c, &again.device.key_package().unwrap())
            .unwrap_err(),
        ChatError::Denied(Denied::SecondAccount)
    );
    let post = Post {
        forum: [1; 32],
        parent: None,
        day: TODAY,
        text: "new start".into(),
    };
    assert_eq!(
        SignedPost::write(&again.device, &c, post).unwrap_err(),
        ChatError::Denied(Denied::SecondAccount)
    );
}

#[test]
fn a_ban_takes_effect_at_once_in_every_app() {
    let mut w = World::new();
    let alice = w.member(1, Some(1), "Erika Mustermann");
    let bob = w.member(2, Some(2), "Max Mustermann");
    let (mut a, mut b) = pair(&w, &alice, &bob);

    // Bob's app still has the chain of before the ban; Alice's has the new one.
    let before = w.chain.clone();
    w.chain
        .update(&bob.account, |i| i.social_ban_until = Some(TODAY + 31));
    let out = b
        .send(
            &bob.device,
            &ctx(&before, &w.issuers, TODAY),
            &ChatMessage::new("tip"),
        )
        .unwrap();
    assert_eq!(
        a.receive(&alice.device, &w.ctx(), &out.wire).unwrap_err(),
        ChatError::Denied(Denied::SocialBan { until: TODAY + 31 })
    );
    // A current app does not even send.
    assert_eq!(
        b.send(&bob.device, &w.ctx(), &ChatMessage::new("tip"))
            .unwrap_err(),
        ChatError::Denied(Denied::SocialBan { until: TODAY + 31 })
    );
}

#[test]
fn a_posting_ban_ends_on_its_day() {
    let mut w = World::new();
    let alice = w.member(1, Some(1), "Erika Mustermann");
    w.chain
        .update(&alice.account, |i| i.posting_ban_until = Some(TODAY + 3));
    let post = |day| Post {
        forum: [1; 32],
        parent: None,
        day,
        text: "back".into(),
    };
    assert_eq!(
        SignedPost::write(&alice.device, &w.ctx(), post(TODAY)).unwrap_err(),
        ChatError::Denied(Denied::PostingBan { until: TODAY + 3 })
    );
    let later = ctx(&w.chain, &w.issuers, TODAY + 3);
    assert!(SignedPost::write(&alice.device, &later, post(TODAY + 3)).is_ok());
}

#[test]
fn an_inactive_account_does_not_write() {
    let mut w = World::new();
    let alice = w.member(1, Some(1), "Erika Mustermann");
    w.chain.update(&alice.account, |i| i.active = false);
    assert_eq!(
        alice.device.create_group(&w.ctx()).err(),
        Some(ChatError::Denied(Denied::Inactive))
    );
}

#[test]
fn a_commit_that_adds_someone_who_may_not_write_is_not_taken() {
    let mut w = World::new();
    let alice = w.member(1, Some(1), "Erika Mustermann");
    let bob = w.member(2, Some(2), "Max Mustermann");
    let carol = w.member(3, None, "");
    let (_a, mut b) = pair(&w, &alice, &bob);
    let (mut a, _) = (_a, ());
    // Bob's app wrongly believes Carol is verified.
    let mut wrong = w.chain.clone();
    wrong.verify(&carol.account, [3; 32]);
    let added = b
        .add(
            &bob.device,
            &ctx(&wrong, &w.issuers, TODAY),
            &carol.device.key_package().unwrap(),
        )
        .unwrap();
    assert_eq!(
        a.receive(&alice.device, &w.ctx(), &added.commit)
            .unwrap_err(),
        ChatError::Denied(Denied::NotVerified)
    );
}

#[test]
fn certificates_that_do_not_hold_are_refused() {
    let mut w = World::new();
    let alice = w.member(1, Some(1), "Erika Mustermann");
    let bob = w.member(2, Some(2), "Max Mustermann");
    let c = w.ctx();
    let mut a = alice.device.create_group(&c).unwrap();

    let try_add = |a: &mut ChatGroup,
                   signed: SignedCert,
                   name: Option<NameCard>,
                   device: Option<ChatDevice>| {
        let mut d = device.unwrap_or_else(|| ChatDevice::new().unwrap());
        d.set_identity(Identity { cert: signed, name });
        a.add(&alice.device, &c, &d.key_package().unwrap())
            .unwrap_err()
    };
    let fresh = || ChatDevice::new().unwrap();

    // Signed by a key that is not Bob's.
    let d = fresh();
    let mut forged = cert(
        &alice.key,
        bob.account,
        d.chat_key().unwrap(),
        Kind::Person,
        TODAY,
        TODAY + 1,
    );
    forged.cert.signer = DeviceKey::Ed25519(bob.key.verifying_key().to_bytes());
    assert_eq!(
        try_add(&mut a, forged, None, Some(d)),
        ChatError::BadSignature
    );

    // Signed by a key that is not on Bob's account.
    let d = fresh();
    let stranger = SigningKey::from_bytes(&[77; 32]);
    let other = cert(
        &stranger,
        bob.account,
        d.chat_key().unwrap(),
        Kind::Person,
        TODAY,
        TODAY + 1,
    );
    assert_eq!(
        try_add(&mut a, other, None, Some(d)),
        ChatError::KeyNotOnAccount
    );

    // Expired, and longer than the rules allow.
    let d = fresh();
    let old = cert(
        &bob.key,
        bob.account,
        d.chat_key().unwrap(),
        Kind::Person,
        TODAY - 40,
        TODAY - 11,
    );
    assert_eq!(try_add(&mut a, old, None, Some(d)), ChatError::Expired);
    let d = fresh();
    let long = cert(
        &bob.key,
        bob.account,
        d.chat_key().unwrap(),
        Kind::Person,
        TODAY,
        TODAY + DAYS,
    );
    assert_eq!(try_add(&mut a, long, None, Some(d)), ChatError::Expired);

    // A bot certificate (2discuss AG: no bots).
    let d = fresh();
    let bot = cert(
        &bob.key,
        bob.account,
        d.chat_key().unwrap(),
        Kind::Other(1),
        TODAY,
        TODAY + 1,
    );
    assert_eq!(
        try_add(&mut a, bot, None, Some(d)),
        ChatError::KindNotAllowed
    );

    // Bob's certificate on another device: its chat key is not the leaf's.
    let copied = bob.device.identity().unwrap().cert.clone();
    assert_eq!(try_add(&mut a, copied, None, None), ChatError::KeyMismatch);

    // A name card from a service that is not trusted.
    let d = fresh();
    let ok = cert(
        &bob.key,
        bob.account,
        d.chat_key().unwrap(),
        Kind::Person,
        TODAY,
        TODAY + 1,
    );
    let fake = NameCard::sign(
        &SigningKey::from_bytes(&[66; 32]),
        bob.account,
        "Someone Else",
        TODAY,
    )
    .unwrap();
    assert_eq!(
        try_add(&mut a, ok, Some(fake), Some(d)),
        ChatError::BadNameCard
    );

    // A name card of another account.
    let d = fresh();
    let ok = cert(
        &bob.key,
        bob.account,
        d.chat_key().unwrap(),
        Kind::Person,
        TODAY,
        TODAY + 1,
    );
    let alices = alice.device.identity().unwrap().name.clone();
    assert_eq!(try_add(&mut a, ok, alices, Some(d)), ChatError::BadNameCard);
}

#[test]
fn forum_posts_are_signed_readable_by_all_and_commented() {
    let mut w = World::new();
    let alice = w.member(1, Some(1), "Erika Mustermann");
    let bob = w.member(2, Some(2), "Max Mustermann");
    let carol = w.member(3, None, "");
    let c = w.ctx();
    let forum = [5; 32];
    let post = SignedPost::write(
        &alice.device,
        &c,
        Post {
            forum,
            parent: None,
            day: TODAY,
            text: "Who sells a bike in Freiburg?".into(),
        },
    )
    .unwrap();
    let bytes = post.to_bytes().unwrap();
    let read = SignedPost::from_bytes(&bytes).unwrap();
    assert_eq!(read, post);
    let writer = read.verify(&c).unwrap();
    assert_eq!(writer.account, alice.account);
    assert_eq!(writer.name.as_deref(), Some("Erika Mustermann"));

    // Changed text: the signature no longer holds.
    let mut changed = read.clone();
    changed.post.text = "Who buys a bike?".into();
    assert_eq!(changed.verify(&c).unwrap_err(), ChatError::BadSignature);

    // Bob comments; Carol (unverified) may read but not write.
    let comment = SignedPost::write(
        &bob.device,
        &c,
        Post {
            forum,
            parent: Some(post.id().unwrap()),
            day: TODAY,
            text: "I do".into(),
        },
    )
    .unwrap();
    assert_eq!(comment.verify(&c).unwrap().account, bob.account);
    assert_eq!(comment.post.parent, Some(post.id().unwrap()));
    assert_eq!(
        SignedPost::write(
            &carol.device,
            &c,
            Post {
                forum,
                parent: None,
                day: TODAY,
                text: "me too".into()
            }
        )
        .unwrap_err(),
        ChatError::Denied(Denied::NotVerified)
    );
    assert!(w.chain.account(&carol.account).is_some());
}

#[test]
fn the_quota_counts_per_person_and_day() {
    let limits = Limits {
        messages_per_day: 2,
        posts_per_day: 1,
    };
    let mut q = Quota::new();
    let p = [1; 32];
    assert!(q.count(&limits, &p, TODAY, What::Message).is_ok());
    assert!(q.count(&limits, &p, TODAY, What::Message).is_ok());
    assert_eq!(
        q.count(&limits, &p, TODAY, What::Message),
        Err(ChatError::QuotaReached)
    );
    assert!(q.count(&limits, &p, TODAY, What::Post).is_ok());
    assert_eq!(
        q.count(&limits, &p, TODAY, What::Post),
        Err(ChatError::QuotaReached)
    );
    assert!(q.count(&limits, &[2; 32], TODAY, What::Message).is_ok());
    assert!(q.count(&limits, &p, TODAY + 1, What::Message).is_ok());
}

#[test]
fn the_limits_come_from_the_rules_file() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rules/alpha.toml");
    let set = capita_rulefile::RuleSet::from_toml(&std::fs::read_to_string(path).unwrap()).unwrap();
    let limits = Limits {
        messages_per_day: set.chat.messages_per_day.value,
        posts_per_day: set.chat.posts_per_day.value,
    };
    assert_eq!(limits.messages_per_day, 500);
    assert_eq!(limits.posts_per_day, 20);
    assert_eq!(set.chat.cert_days.value, DAYS);
}

#[test]
fn identities_survive_their_bytes() {
    let mut w = World::new();
    let alice = w.member(1, Some(1), "Erika Mustermann");
    let id = alice.device.identity().unwrap();
    assert_eq!(&Identity::from_bytes(&id.to_bytes().unwrap()).unwrap(), id);
    let mut bytes = id.to_bytes().unwrap();
    bytes.push(0);
    assert!(Identity::from_bytes(&bytes).is_err());
    assert!(Identity::from_bytes(&bytes[..10]).is_err());
}
