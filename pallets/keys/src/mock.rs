// Capita Money – pallet-capita-keys
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! A test runtime: the system pallet (pallet 0) and the keys (pallet 1) –
//! the indices the app's vectors use (`app/tests/make-key-vectors.mjs`).

use core::cell::RefCell;

use capita_keys::encoding::{account_of, act_challenge, add_challenge, base64url};
use capita_keys::types::{DeviceKey, PasskeyProof, Proof};
use codec::Encode;
use frame_support::derive_impl;
use frame_support::pallet_prelude::DispatchResult;
use frame_support::traits::{ConstU32, ConstU64};
use frame_system::EnsureRoot;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::SigningKey;
use sp_core::Pair;
use sp_runtime::traits::IdentityLookup;
use sp_runtime::{AccountId32, BuildStorage};

use crate as pallet_capita_keys;

type Block = frame_system::mocking::MockBlock<Test>;

frame_support::construct_runtime!(
    pub enum Test {
        System: frame_system,
        Keys: pallet_capita_keys,
    }
);

#[derive_impl(frame_system::config_preludes::TestDefaultConfig)]
impl frame_system::Config for Test {
    type Block = Block;
    type AccountId = AccountId32;
    type Lookup = IdentityLookup<Self::AccountId>;
}

thread_local! {
    /// Seconds of the test clock.
    pub static NOW: RefCell<u64> = const { RefCell::new(1_000_000) };
}

/// The test clock.
pub struct Clock;

impl frame_support::traits::UnixTime for Clock {
    fn now() -> core::time::Duration {
        core::time::Duration::from_secs(NOW.with(|n| *n.borrow()))
    }
}

pub fn advance(secs: u64) {
    NOW.with(|n| *n.borrow_mut() += secs);
}

pub const WEEK: u64 = 7 * 24 * 3600;
pub const RP: &str = "app.capitamoney.com";
pub const ORIGIN: &str = "https://app.capitamoney.com";

frame_support::parameter_types! {
    pub const Priority: u64 = 1 << 20;
}

impl pallet_capita_keys::Config for Test {
    type RuntimeCall = RuntimeCall;
    type PolicyOrigin = EnsureRoot<AccountId32>;
    type Time = Clock;
    type MaxKeys = ConstU32<3>;
    type HomeDelay = ConstU64<WEEK>;
    type UnsignedPriority = Priority;
}

pub fn new_test_ext() -> sp_io::TestExternalities {
    NOW.with(|n| *n.borrow_mut() = 1_000_000);
    let storage = RuntimeGenesisConfig {
        system: Default::default(),
        keys: pallet_capita_keys::GenesisConfig {
            relying_parties: vec![RP.into()],
            origins: vec![ORIGIN.into()],
            ..Default::default()
        },
    }
    .build_storage()
    .unwrap();
    let mut ext = sp_io::TestExternalities::new(storage);
    ext.execute_with(|| System::set_block_number(1));
    ext
}

// ------------------------------------------------------------- device keys

pub fn ed(n: u8) -> sp_core::ed25519::Pair {
    sp_core::ed25519::Pair::from_seed(&[n; 32])
}

pub fn ed_key(n: u8) -> DeviceKey {
    DeviceKey::Ed25519(ed(n).public().0)
}

pub fn remark(b: u8) -> RuntimeCall {
    RuntimeCall::System(frame_system::Call::remark { remark: vec![b] })
}

pub fn challenge(account: &AccountId32, nonce: u32, call: &RuntimeCall) -> [u8; 32] {
    let raw: [u8; 32] = account.clone().into();
    act_challenge(
        &Keys::audience(),
        &raw,
        nonce,
        &sp_io::hashing::blake2_256(&call.encode()),
    )
}

pub fn sign_ed(n: u8, account: &AccountId32, nonce: u32, call: &RuntimeCall) -> Proof {
    Proof::Ed25519(ed(n).sign(&challenge(account, nonce, call)).0)
}

/// Opens the account of device key `n` (first call: a remark).
pub fn open_ed(n: u8) -> AccountId32 {
    let key = ed_key(n);
    let account = AccountId32::from(account_of(&key));
    let call = remark(0);
    let proof = sign_ed(n, &account, 0, &call);
    Keys::open(RuntimeOrigin::none(), key, Box::new(call), proof).unwrap();
    account
}

/// Device key `n` acts for `account` with the account's next nonce.
pub fn act_ed(n: u8, account: &AccountId32, call: RuntimeCall) -> DispatchResult {
    let nonce = crate::Nonce::<Test>::get(account);
    let proof = sign_ed(n, account, nonce, &call);
    Keys::act(
        RuntimeOrigin::none(),
        account.clone(),
        ed_key(n),
        Box::new(call),
        nonce,
        proof,
    )
}

/// The call that links device key `new`, with its proof of possession.
pub fn add_ed(new: u8, account: &AccountId32) -> RuntimeCall {
    let raw: [u8; 32] = account.clone().into();
    let c = add_challenge(&Keys::audience(), &raw, &ed_key(new));
    RuntimeCall::Keys(crate::Call::add_key {
        key: ed_key(new),
        possession: Proof::Ed25519(ed(new).sign(&c).0),
    })
}

/// What the last action's call answered.
pub fn last_result() -> Option<DispatchResult> {
    System::events().into_iter().rev().find_map(|r| match r.event {
        RuntimeEvent::Keys(crate::Event::Acted { result, .. }) => Some(result),
        _ => None,
    })
}

// ---------------------------------------------------------------- passkeys

pub fn phone(n: u8) -> SigningKey {
    SigningKey::from_slice(&[n; 32]).unwrap()
}

pub fn passkey(phone: &SigningKey) -> DeviceKey {
    let point = phone.verifying_key().to_encoded_point(true);
    DeviceKey::Passkey(point.as_bytes().try_into().unwrap())
}

/// What a phone's passkey answers for `challenge` on `origin`.
pub fn assertion(phone: &SigningKey, challenge: &[u8; 32], origin: &str) -> Proof {
    let mut data = sp_io::hashing::sha2_256(RP.as_bytes()).to_vec();
    data.extend_from_slice(&[0x05, 0, 0, 0, 0]);
    let c = String::from_utf8(base64url(challenge)).unwrap();
    let client = format!(
        r#"{{"type":"webauthn.get","challenge":"{c}","origin":"{origin}","crossOrigin":false}}"#
    );
    let mut message = data.clone();
    message.extend_from_slice(&sp_io::hashing::sha2_256(client.as_bytes()));
    let signature: p256::ecdsa::Signature = phone.sign(&message);
    let mut raw = [0u8; 64];
    raw.copy_from_slice(&signature.to_bytes());
    Proof::Passkey(PasskeyProof {
        authenticator_data: data,
        client_data_json: client.into_bytes(),
        signature: raw,
    })
}
