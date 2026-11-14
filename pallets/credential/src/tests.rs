// Capita Money – pallet-capita-credential
// Copyright (C) 2026 Capita Money contributors
// SPDX-License-Identifier: AGPL-3.0-only
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, version 3.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// for more details: <https://www.gnu.org/licenses/>.

//! What the pallet adds to the check of a claim: the issuer list, one claim
//! per account, numbers used once, revocation, and what it tells the
//! identities and the rules.
//!
//! Which claims count is tested in `capita-credential`.

use capita_core::attributes::{AttrKey, AttrValue, Attribute, Residence, VerificationLevel};
use capita_core::types::CountryCode;
use capita_credential::check::person_id;
use capita_credential::did::did_key;
use capita_credential::types::{
    Claim, Issuer, IssuerId, IssuerStatus, PublicKey, Realm, Revocation, Serial, Signature,
    SubjectRef,
};
use frame_support::{assert_noop, assert_ok};
use sp_core::Pair;
use sp_runtime::DispatchError::{self, BadOrigin};

use crate::mock::*;
use crate::{
    Call, Credentials, DropReason, Error, Event, HostCrypto, Issuers, MoveOf, Moves, NextIssuer,
    SerialState, Serials, StopReason,
};

/// Account `n` presents `claim`, signed by issuer key 1.
fn present(n: u8, claim: &Claim) -> sp_runtime::DispatchResult {
    present_as(n, claim, sign(1, claim), None)
}

fn present_as(
    n: u8,
    claim: &Claim,
    signature: Signature,
    name: Option<&str>,
) -> sp_runtime::DispatchResult {
    call(
        signed(n),
        Call::present {
            claim: claim.clone(),
            signature,
            name: name.map(|name| name.as_bytes().to_vec()),
        },
    )
}

/// Anybody (account 9) hands in the revocation of claim `serial` of issuer
/// 0, signed by issuer key `key`.
fn revoke(serial: u8, key: u8) -> sp_runtime::DispatchResult {
    let revocation = Revocation {
        issuer: IssuerId(0),
        serial: Serial([serial; 16]),
    };
    let signature = sign_revocation(key, &revocation);
    call(
        signed(9),
        Call::revoke {
            revocation,
            signature,
        },
    )
}

fn update(issuer: Issuer) -> sp_runtime::DispatchResult {
    call(
        root(),
        Call::update_issuer {
            id: IssuerId(0),
            issuer,
        },
    )
}

/// The invariants of the chain state hold.
fn check() {
    assert_eq!(Credential::do_try_state(), Ok(()));
}

#[test]
fn presenting_a_claim_makes_a_member_with_a_level() {
    new_test_ext().execute_with(|| {
        let mut claim = claim(1, 2, 1);
        claim.residence = Some(Residence {
            country: CountryCode(*b"FR"),
            proven_at: ts(0),
        });
        claim.attributes = vec![Attribute {
            key: AttrKey(1),
            value: AttrValue(42),
        }];
        assert_ok!(present_as(1, &claim, sign(1, &claim), Some("Ada")));

        assert!(is_member(1));
        assert_eq!(world(|w| w.identities[&acct(1)].clone()), b"Ada".to_vec());
        // the rules were told everything the claim states, about a person
        // named by realm and pseudonym
        let stated = world(|w| w.attested[&acct(1)].clone());
        assert_eq!(
            stated.person,
            person_id::<HostCrypto>(Realm(1), &SubjectRef([1; 32]))
        );
        assert_eq!(stated.level, VerificationLevel(2));
        assert_eq!(stated.residence, claim.residence);
        assert_eq!(stated.attributes, claim.attributes);
        assert_eq!(stated.issued_at, ts(0));
        assert_eq!(stated.expires_at, Some(ts(365 * DAY)));

        assert_eq!(Credentials::<Test>::get(acct(1)), Some(claim.clone()));
        assert_eq!(
            Serials::<Test>::get(IssuerId(0), Serial([1; 16])),
            Some(SerialState::Presented(acct(1)))
        );
        assert_eq!(
            events(),
            vec![Event::Presented {
                account: acct(1),
                issuer: IssuerId(0),
                serial: Serial([1; 16]),
                level: VerificationLevel(2),
                expires_at: Some(ts(365 * DAY)),
            }]
        );
        check();
    });
}

#[test]
fn a_claim_is_for_its_subject_only() {
    new_test_ext().execute_with(|| {
        let claim = claim(1, 2, 1);
        // somebody else got hold of the claim
        assert_noop!(present(2, &claim), Error::<Test>::NotSubject);
        // and cannot make it their own
        let mut stolen = claim.clone();
        stolen.subject = capita_core::types::AccountId([2; 32]);
        assert_noop!(
            present_as(2, &stolen, sign(1, &claim), None),
            Error::<Test>::BadSignature
        );
        assert_noop!(
            call(
                root(),
                Call::present {
                    claim: claim.clone(),
                    signature: sign(1, &claim),
                    name: None
                }
            ),
            BadOrigin
        );
        assert_ok!(present(1, &claim));
        check();
    });
}

#[test]
fn a_claim_needs_a_listed_active_issuer_and_its_signature() {
    new_test_ext().execute_with(|| {
        let claim1 = claim(1, 2, 1);
        // not signed by a key of the issuer
        assert_noop!(
            present_as(1, &claim1, sign(2, &claim1), None),
            Error::<Test>::BadSignature
        );
        // signed for another chain
        let other = capita_credential::types::Audience([8; 32]);
        let bytes = capita_credential::encoding::claim_bytes(&other, &claim1);
        let elsewhere = Signature::Ed25519(secret(1).sign(&bytes).0);
        assert_noop!(
            present_as(1, &claim1, elsewhere, None),
            Error::<Test>::BadSignature
        );
        // an issuer nobody listed
        let mut unknown = claim1.clone();
        unknown.issuer = IssuerId(1);
        assert_noop!(present(1, &unknown), Error::<Test>::UnknownIssuer);
        // more than the issuer is trusted for
        assert_noop!(present(1, &claim(1, 4, 1)), Error::<Test>::LevelNotAllowed);
        // a suspended issuer issues nothing
        assert_ok!(update(Issuer {
            status: IssuerStatus::Suspended,
            ..issuer()
        }));
        assert_noop!(present(1, &claim1), Error::<Test>::IssuerNotActive);
        assert_ok!(update(issuer()));
        assert_ok!(present(1, &claim1));
        check();
    });
}

#[test]
fn an_sr25519_issuer_signs_too() {
    new_test_ext().execute_with(|| {
        let pair = sp_core::sr25519::Pair::from_seed(&[3; 32]);
        let key = PublicKey::Sr25519(pair.public().0);
        assert_ok!(call(
            root(),
            Call::register_issuer {
                issuer: Issuer {
                    did: did_key(&key),
                    keys: vec![key],
                    ..issuer()
                }
            }
        ));
        let mut claim = claim(1, 1, 1);
        claim.issuer = IssuerId(1);
        let bytes = capita_credential::encoding::claim_bytes(&Credential::audience(), &claim);
        let signature = Signature::Sr25519(pair.sign(&bytes).0);
        // the right bytes under the wrong scheme label match nothing
        let Signature::Sr25519(raw) = signature else {
            unreachable!()
        };
        assert_noop!(
            present_as(1, &claim, Signature::Ed25519(raw), None),
            Error::<Test>::BadSignature
        );
        assert_ok!(present_as(1, &claim, signature, None));
        assert_eq!(level(1), 1);
        check();
    });
}

#[test]
fn the_account_must_be_open_with_the_rules() {
    new_test_ext().execute_with(|| {
        world(|w| w.open.remove(&acct(1)));
        assert_noop!(
            present(1, &claim(1, 2, 1)),
            DispatchError::Other("no account")
        );
        assert!(!is_member(1));
    });
}

#[test]
fn one_person_one_account() {
    new_test_ext().execute_with(|| {
        assert_ok!(present(1, &claim(1, 2, 1)));
        // the same person, known to the issuer under the same pseudonym,
        // tries a second account
        let mut second = claim(2, 2, 2);
        second.subject_ref = SubjectRef([1; 32]);
        assert_noop!(
            present(2, &second),
            DispatchError::Other("duplicate person")
        );
        assert!(!is_member(2));
        // and an account stays with its person
        let mut other_person = claim(1, 2, 3);
        other_person.subject_ref = SubjectRef([7; 32]);
        assert_noop!(
            present(1, &other_person),
            DispatchError::Other("person mismatch")
        );
        check();
    });
}

#[test]
fn a_claim_is_presented_once_and_a_later_one_replaces_it() {
    new_test_ext().execute_with(|| {
        let first = claim(1, 1, 1);
        assert_ok!(present(1, &first));
        assert_noop!(present(1, &first), Error::<Test>::AlreadyPresented);

        set_time(10 * DAY);
        let second = claim(1, 3, 2);
        assert_ok!(present(1, &second));
        assert_eq!(level(1), 3);
        assert_eq!(Credentials::<Test>::get(acct(1)), Some(second));
        // the membership was renewed, not started twice
        assert_eq!(world(|w| w.members[&acct(1)]), 2);

        // no way back to an older statement, even one never presented
        let mut old = claim(1, 2, 3);
        old.issued_at = ts(5 * DAY);
        assert_noop!(present(1, &old), Error::<Test>::OlderThanCurrent);
        assert_noop!(present(1, &first), Error::<Test>::AlreadyPresented);
        check();
    });
}

#[test]
fn an_expired_claim_is_cleared_away_by_anybody() {
    new_test_ext().execute_with(|| {
        assert_ok!(present(1, &claim(1, 2, 1)));
        let drop = || call(signed(9), Call::drop_invalid { account: acct(1) });
        set_time(365 * DAY - 1);
        assert_noop!(drop(), Error::<Test>::StillCounts);
        assert_ok!(call(signed(1), Call::renew {}));

        set_time(365 * DAY);
        assert_noop!(
            call(signed(1), Call::renew {}),
            Error::<Test>::NoLongerCounts
        );
        assert_ok!(drop());
        assert!(!is_member(1));
        assert_eq!(level(1), 0);
        assert_eq!(Credentials::<Test>::get(acct(1)), None);
        assert_eq!(
            events().last(),
            Some(&Event::Dropped {
                account: acct(1),
                reason: DropReason::Invalid
            })
        );
        assert_noop!(drop(), Error::<Test>::NoCredential);
        // the expired claim cannot come back; a new one can
        assert_noop!(present(1, &claim(1, 2, 1)), Error::<Test>::AlreadyPresented);
        assert_ok!(present(1, &claim(1, 2, 2)));
        assert!(is_member(1));
        check();
    });
}

#[test]
fn renewing_restores_what_a_lapsed_membership_took() {
    new_test_ext().execute_with(|| {
        assert_noop!(call(signed(1), Call::renew {}), Error::<Test>::NoCredential);
        assert_ok!(present(1, &claim(1, 2, 1)));

        // the membership runs out before the claim does: the identity
        // pallets end it and report that
        set_time(200 * DAY);
        world(|w| w.members.remove(&acct(1)));
        Credential::membership_ended(&acct(1));
        assert_eq!(level(1), 0);
        assert!(Credentials::<Test>::get(acct(1)).is_some());

        assert_ok!(call(signed(1), Call::renew {}));
        assert!(is_member(1));
        assert_eq!(level(1), 2);
        // exactly what the claim stated, validity included
        let stated = world(|w| w.attested[&acct(1)].clone());
        assert_eq!(stated.issued_at, ts(0));
        assert_eq!(stated.expires_at, Some(ts(365 * DAY)));
        assert_eq!(events().last(), Some(&Event::Renewed { account: acct(1) }));
        check();
    });
}

#[test]
fn an_issuer_takes_a_claim_back() {
    new_test_ext().execute_with(|| {
        assert_ok!(present(1, &claim(1, 2, 1)));
        // only the issuer can
        assert_noop!(revoke(1, 2), Error::<Test>::BadSignature);
        assert_ok!(revoke(1, 1));
        assert!(!is_member(1));
        assert_eq!(level(1), 0);
        assert_eq!(Credentials::<Test>::get(acct(1)), None);
        assert_eq!(
            Serials::<Test>::get(IssuerId(0), Serial([1; 16])),
            Some(SerialState::Revoked)
        );
        assert_eq!(
            events()[1..],
            [
                Event::Revoked {
                    issuer: IssuerId(0),
                    serial: Serial([1; 16])
                },
                Event::Dropped {
                    account: acct(1),
                    reason: DropReason::Revoked
                },
            ]
        );
        assert_noop!(revoke(1, 1), Error::<Test>::AlreadyRevoked);
        assert_noop!(present(1, &claim(1, 2, 1)), Error::<Test>::ClaimRevoked);
        assert_noop!(call(signed(1), Call::renew {}), Error::<Test>::NoCredential);

        // a claim taken back before anybody presented it
        assert_ok!(revoke(2, 1));
        assert_noop!(present(2, &claim(2, 2, 2)), Error::<Test>::ClaimRevoked);
        check();
    });
}

#[test]
fn taking_back_a_replaced_claim_leaves_the_current_one() {
    new_test_ext().execute_with(|| {
        assert_ok!(present(1, &claim(1, 1, 1)));
        assert_ok!(present(1, &claim(1, 3, 2)));
        assert_ok!(revoke(1, 1));
        assert!(is_member(1));
        assert_eq!(level(1), 3);
        assert!(Credentials::<Test>::get(acct(1)).is_some());
        check();
    });
}

#[test]
fn the_issuer_list_is_kept_by_its_origin() {
    new_test_ext().execute_with(|| {
        let second = Issuer {
            did: b"did:web:letters.example.net".to_vec(),
            keys: vec![public(2)],
            realm: Realm(2),
            ..issuer()
        };
        let register = |origin, issuer: &Issuer| {
            call(
                origin,
                Call::register_issuer {
                    issuer: issuer.clone(),
                },
            )
        };
        assert_noop!(register(signed(1), &second), BadOrigin);
        assert_ok!(register(root(), &second));
        assert_eq!(Issuers::<Test>::get(IssuerId(1)), Some(second.clone()));
        assert_eq!(NextIssuer::<Test>::get(), 2);
        assert_eq!(events(), vec![Event::IssuerRegistered { id: IssuerId(1) }]);

        // entries are checked
        let no_keys = Issuer {
            keys: vec![],
            ..second.clone()
        };
        assert_noop!(register(root(), &no_keys), Error::<Test>::InvalidKeys);
        let no_did = Issuer {
            did: b"letters.example.net".to_vec(),
            ..second.clone()
        };
        assert_noop!(register(root(), &no_did), Error::<Test>::InvalidDid);
        let wrong_key = Issuer {
            did: did_key(&public(3)),
            ..second.clone()
        };
        assert_noop!(register(root(), &wrong_key), Error::<Test>::DidKeyMismatch);

        // updates: by the origin, of a listed issuer, never of the realm
        let update = |origin, id: u32, issuer: &Issuer| {
            call(
                origin,
                Call::update_issuer {
                    id: IssuerId(id),
                    issuer: issuer.clone(),
                },
            )
        };
        assert_noop!(update(signed(1), 1, &second), BadOrigin);
        assert_noop!(update(root(), 2, &second), Error::<Test>::UnknownIssuer);
        let moved = Issuer {
            realm: Realm(1),
            ..second.clone()
        };
        assert_noop!(update(root(), 1, &moved), Error::<Test>::RealmFixed);
        assert_noop!(update(root(), 1, &no_keys), Error::<Test>::InvalidKeys);
        let suspended = Issuer {
            status: IssuerStatus::Suspended,
            ..second
        };
        assert_ok!(update(root(), 1, &suspended));
        assert_eq!(
            events().last(),
            Some(&Event::IssuerUpdated {
                id: IssuerId(1),
                status: IssuerStatus::Suspended
            })
        );
        check();
    });
}

#[test]
fn replacing_a_key_of_an_issuer() {
    new_test_ext().execute_with(|| {
        // both keys for a while, then the new one alone
        assert_ok!(update(Issuer {
            keys: vec![public(1), public(2)],
            ..issuer()
        }));
        let old = claim(1, 2, 1);
        assert_ok!(present_as(1, &old, sign(1, &old), None));
        let new = claim(2, 2, 2);
        assert_ok!(present_as(2, &new, sign(2, &new), None));
        assert_ok!(update(Issuer {
            keys: vec![public(2)],
            ..issuer()
        }));
        let late = claim(3, 2, 3);
        assert_noop!(
            present_as(3, &late, sign(1, &late), None),
            Error::<Test>::BadSignature
        );
        // what the old key signed before stays accepted
        assert_eq!(level(1), 2);
        assert_ok!(call(signed(1), Call::renew {}));
        // and the issuer takes it back with the key it has now
        assert_noop!(revoke(1, 1), Error::<Test>::BadSignature);
        assert_ok!(revoke(1, 2));
        assert_eq!(level(1), 0);
        check();
    });
}

#[test]
fn a_suspended_issuer_keeps_its_claims_a_revoked_one_loses_them() {
    new_test_ext().execute_with(|| {
        assert_ok!(present(1, &claim(1, 2, 1)));
        let drop = || call(signed(9), Call::drop_invalid { account: acct(1) });
        assert_ok!(update(Issuer {
            status: IssuerStatus::Suspended,
            ..issuer()
        }));
        assert_noop!(drop(), Error::<Test>::StillCounts);
        assert_ok!(call(signed(1), Call::renew {}));
        // a suspended issuer can still take back
        assert_ok!(present_as_active(2));
        assert_ok!(revoke(2, 1));
        assert_eq!(level(2), 0);

        assert_ok!(update(Issuer {
            status: IssuerStatus::Revoked,
            ..issuer()
        }));
        assert_noop!(
            call(signed(1), Call::renew {}),
            Error::<Test>::NoLongerCounts
        );
        // until somebody clears the claim away the level stands
        assert_eq!(level(1), 2);
        assert_ok!(drop());
        assert_eq!(level(1), 0);
        assert!(!is_member(1));
        check();
    });
}

/// Account `n` presents a claim while the issuer is active again for a
/// moment, then the issuer is suspended as before.
fn present_as_active(n: u8) -> sp_runtime::DispatchResult {
    update(issuer())?;
    present(n, &claim(n, 2, n))?;
    update(Issuer {
        status: IssuerStatus::Suspended,
        ..issuer()
    })
}

#[test]
fn a_revoked_identity_ends_the_claim_and_freezes_the_account() {
    new_test_ext().execute_with(|| {
        assert_ok!(present(1, &claim(1, 2, 1)));
        // the identity pallets revoke the identity: membership first, …
        world(|w| {
            w.members.remove(&acct(1));
            w.revoked.insert(acct(1));
        });
        Credential::membership_ended(&acct(1));
        Credential::identity_revoked(&acct(1));

        assert_eq!(level(1), 0);
        assert!(world(|w| w.frozen.contains(&acct(1))));
        assert_eq!(Credentials::<Test>::get(acct(1)), None);
        assert_eq!(
            events().last(),
            Some(&Event::Dropped {
                account: acct(1),
                reason: DropReason::IdentityRevoked
            })
        );
        // no claim brings a revoked identity back
        set_time(DAY);
        assert_noop!(
            present(1, &claim(1, 2, 2)),
            DispatchError::Other("identity revoked")
        );
        assert_noop!(call(signed(1), Call::renew {}), Error::<Test>::NoCredential);
        // an account without a claim: frozen all the same, nothing dropped
        let before = events().len();
        Credential::identity_revoked(&acct(2));
        assert!(world(|w| w.frozen.contains(&acct(2))));
        assert_eq!(events().len(), before);
        check();
    });
}

#[test]
fn names_of_identities() {
    new_test_ext().execute_with(|| {
        let first = claim(1, 2, 1);
        assert_ok!(present_as(1, &first, sign(1, &first), Some("Ada")));
        // a name is taken once
        let second = claim(2, 2, 2);
        assert_noop!(
            present_as(2, &second, sign(1, &second), Some("Ada")),
            DispatchError::Other("name taken")
        );
        // without a name the identities derive one
        assert_ok!(present_as(2, &second, sign(1, &second), None));
        assert_eq!(
            world(|w| w.identities[&acct(2)].clone()),
            b"derived".to_vec()
        );
        // an identity keeps its name when it presents a later claim
        set_time(DAY);
        let later = claim(1, 3, 3);
        assert_ok!(present_as(1, &later, sign(1, &later), Some("Grace")));
        assert_eq!(world(|w| w.identities[&acct(1)].clone()), b"Ada".to_vec());
        check();
    });
}

#[test]
fn a_claim_for_another_chain_does_not_count_here() {
    new_test_ext().execute_with(|| {
        // the audience is the hash of the first block
        let genesis = System::block_hash(0);
        assert_eq!(Credential::audience().0, genesis.0);
        assert_ne!(genesis.0, [0; 32]);
    });
}

/// Account `to` asks for the move of the account of person `person`, with
/// a new claim numbered `serial`.
fn request_move(to: u8, person: u8, serial: u8) -> sp_runtime::DispatchResult {
    let mut claim = claim(to, 1, serial);
    claim.subject_ref = SubjectRef([person; 32]);
    let signature = sign(1, &claim);
    call(
        signed(to),
        Call::request_move {
            claim,
            signature,
            name: None,
        },
    )
}

#[test]
fn a_lost_key_moves_the_account_after_the_waiting_time() {
    new_test_ext().execute_with(|| {
        assert_ok!(present(1, &claim(1, 1, 1)));
        set_time(30 * DAY);
        // Somebody who is nobody's person has nothing to move; the holder
        // presents instead of moving; a claim for another account is of no
        // use.
        assert_noop!(request_move(2, 5, 2), Error::<Test>::NothingToMove);
        assert_noop!(request_move(1, 1, 2), Error::<Test>::AlreadyHolder);
        let mut foreign = claim(3, 1, 2);
        foreign.subject_ref = SubjectRef([1; 32]);
        let signature = sign(1, &foreign);
        assert_noop!(
            call(
                signed(2),
                Call::request_move {
                    claim: foreign,
                    signature,
                    name: None
                }
            ),
            Error::<Test>::NotSubject
        );
        // Person 1, verified again, with account 2 under a new key.
        assert_ok!(request_move(2, 1, 2));
        assert_eq!(MoveOf::<Test>::get(acct(2)), Some(acct(1)));
        assert_eq!(
            events().last(),
            Some(&Event::MoveRequested {
                from: acct(1),
                to: acct(2),
                executable_at: ts(37 * DAY),
            })
        );
        // Nothing moved yet; one move at a time; the claim is used.
        assert!(is_member(1) && !is_member(2));
        assert_noop!(request_move(3, 1, 3), Error::<Test>::MovePending);
        assert_noop!(request_move(2, 1, 2), Error::<Test>::AlreadyPresented);
        assert_noop!(
            call(signed(9), Call::complete_move { from: acct(1) }),
            Error::<Test>::MoveNotDue
        );
        check();
        set_time(37 * DAY);
        assert_ok!(call(signed(9), Call::complete_move { from: acct(1) }));
        assert_eq!(
            events().last(),
            Some(&Event::Moved {
                from: acct(1),
                to: acct(2)
            })
        );
        assert!(!is_member(1) && is_member(2));
        assert_eq!(level(1), 0);
        assert_eq!(level(2), 1);
        assert!(world(|w| w.retired.contains(&acct(1))));
        assert_eq!(world(|w| w.moved.get(&acct(1)).cloned()), Some(acct(2)));
        assert!(Credentials::<Test>::get(acct(1)).is_none());
        assert_eq!(
            Credentials::<Test>::get(acct(2)).unwrap().serial,
            Serial([2; 16])
        );
        assert!(Moves::<Test>::get(acct(1)).is_none() && MoveOf::<Test>::get(acct(2)).is_none());
        assert_noop!(
            call(signed(9), Call::complete_move { from: acct(1) }),
            Error::<Test>::NoMove
        );
        // The new account renews and is verified again like any other.
        assert_ok!(call(signed(2), Call::renew {}));
        check();
    });
}

#[test]
fn the_old_key_stops_a_move_and_the_asker_can_withdraw() {
    new_test_ext().execute_with(|| {
        assert_ok!(present(1, &claim(1, 1, 1)));
        assert_ok!(request_move(2, 1, 2));
        // Nobody else.
        assert_noop!(call(signed(3), Call::stop_move {}), Error::<Test>::NoMove);
        // "This was not me."
        assert_ok!(call(signed(1), Call::stop_move {}));
        assert_eq!(
            events().last(),
            Some(&Event::MoveStopped {
                from: acct(1),
                to: acct(2),
                reason: StopReason::ByOldKey,
            })
        );
        set_time(8 * DAY);
        assert_noop!(
            call(signed(9), Call::complete_move { from: acct(1) }),
            Error::<Test>::NoMove
        );
        assert!(is_member(1) && !is_member(2));
        // Asked again with a new claim, and withdrawn by the asker.
        assert_ok!(request_move(2, 1, 3));
        assert_ok!(call(signed(2), Call::stop_move {}));
        assert_eq!(
            events().last(),
            Some(&Event::MoveStopped {
                from: acct(1),
                to: acct(2),
                reason: StopReason::Withdrawn,
            })
        );
        check();
    });
}

#[test]
fn a_move_whose_claim_counts_no_longer_does_not_happen() {
    new_test_ext().execute_with(|| {
        assert_ok!(present(1, &claim(1, 1, 1)));
        // Taken back by the issuer while the move waits.
        assert_ok!(request_move(2, 1, 2));
        assert_ok!(revoke(2, 1));
        assert_eq!(
            events().last(),
            Some(&Event::MoveStopped {
                from: acct(1),
                to: acct(2),
                reason: StopReason::Lapsed,
            })
        );
        assert!(is_member(1));
        // The issuer struck off the list while the move waits.
        assert_ok!(request_move(2, 1, 3));
        assert_ok!(update(Issuer {
            status: IssuerStatus::Revoked,
            ..issuer()
        }));
        set_time(7 * DAY);
        assert_ok!(call(signed(9), Call::complete_move { from: acct(1) }));
        assert_eq!(
            events().last(),
            Some(&Event::MoveStopped {
                from: acct(1),
                to: acct(2),
                reason: StopReason::Lapsed,
            })
        );
        assert!(world(|w| w.moved.is_empty()));
        check();
    });
}

#[test]
fn without_a_waiting_time_a_move_happens_at_once() {
    new_test_ext().execute_with(|| {
        MoveDelay::set(&0);
        assert_ok!(present(1, &claim(1, 1, 1)));
        // An account that holds a claim of its own is no target.
        assert_ok!(present(3, &claim(3, 1, 3)));
        assert_noop!(request_move(3, 1, 4), Error::<Test>::TargetHoldsClaim);
        assert_ok!(request_move(2, 1, 2));
        assert!(!is_member(1) && is_member(2));
        assert_eq!(level(2), 1);
        assert!(Moves::<Test>::iter().next().is_none());
        // A person whose key changed twice.
        assert_ok!(request_move(4, 1, 5));
        assert!(!is_member(2) && is_member(4));
        check();
    });
}
