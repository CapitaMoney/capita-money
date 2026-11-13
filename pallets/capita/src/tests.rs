// Capita Money – pallet-capita
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

//! The rules, run through the pallet: storage, ledger, origins, events.
//!
//! The rules themselves are tested in `capita-core`. These tests check what
//! the pallet adds, and that the invariants of the core hold on chain state.

use capita_core::account::AccountStatus;
use capita_core::attributes::{Attestation, VerificationLevel};
use capita_core::ports::Event as RuleEvent;
use capita_core::totals::Totals;
use capita_core::types::{ParamsVersion, PersonId, TierId};
use frame_support::{assert_noop, assert_ok};
use sp_runtime::DispatchError::BadOrigin;

use crate::mock::*;
use crate::{
    Accounts, Call, Error, Event, LoanOffers, ParamsHistory, PersonAccount, PublicTotals,
    ServiceAccount, TotalSupply,
};

fn open(n: u8) {
    assert_ok!(call(signed(n), Call::open {}));
}

fn attestation(person: u8, level: u8) -> Attestation {
    Attestation {
        person: PersonId([person; 32]),
        level: VerificationLevel(level),
        residence: None,
        attributes: vec![],
        issued_at: Capita::now(),
        expires_at: None,
    }
}

/// Opens account 9 and names it verification service.
fn service() {
    open(9);
    assert_ok!(call(root(), Call::set_service_account { account: acct(9) }));
}

/// Opens account `n`, pays its verification with the start grant, attests
/// `level` and moves it to the ladder step of the same number.
fn member(n: u8, level: u8) {
    open(n);
    assert_ok!(call(signed(n), Call::pay_verification {}));
    let attestation = Some(attestation(n, level));
    assert_ok!(call(
        root(),
        Call::attest {
            account: acct(n),
            attestation
        }
    ));
    let tier = TierId(u16::from(level));
    assert_ok!(call(signed(n), Call::set_tier { tier }));
}

fn purchase(n: u8, amount: u64) {
    assert_ok!(call(
        root(),
        Call::purchase {
            account: acct(n),
            amount: cents(amount)
        }
    ));
}

fn transfer(from: u8, to: u8, amount: u64) -> sp_runtime::DispatchResult {
    call(
        signed(from),
        Call::transfer {
            to: acct(to),
            amount: cents(amount),
        },
    )
}

/// The invariants of the whole chain state hold.
fn check() {
    assert_eq!(Capita::do_try_state(), Ok(()));
}

#[test]
fn opening_an_account_grants_the_start_capita_once() {
    new_test_ext(params()).execute_with(|| {
        open(1);
        assert_eq!(balance(1), 100);
        assert_eq!(TotalSupply::<Test>::get(), 100);
        assert_eq!(PublicTotals::<Test>::get().start_grants, 100);
        assert_eq!(PublicTotals::<Test>::get().accounts_opened, 1);
        assert_eq!(
            rule_events(),
            vec![
                RuleEvent::AccountOpened {
                    account: cid(1),
                    at: ts(0)
                },
                RuleEvent::StartGranted {
                    account: cid(1),
                    amount: cents(100),
                    at: ts(0)
                },
            ]
        );
        assert_noop!(call(signed(1), Call::open {}), Error::<Test>::AccountExists);
        assert_noop!(call(signed(2), Call::claim {}), Error::<Test>::NoAccount);
        check();
    });
}

#[test]
fn nothing_works_before_rules_are_enacted() {
    ext(None).execute_with(|| {
        assert_noop!(call(signed(1), Call::open {}), Error::<Test>::NoParams);
        let mut broken = params();
        broken.ladder.clear();
        assert_noop!(
            call(root(), Call::enact_params { params: broken }),
            Error::<Test>::InvalidParams
        );
        assert_ok!(call(root(), Call::enact_params { params: params() }));
        open(1);
        check();
    });
}

#[test]
fn the_start_grant_buys_the_verification_and_nothing_else() {
    new_test_ext(params()).execute_with(|| {
        open(1);
        open(2);
        assert_noop!(transfer(1, 2, 1), Error::<Test>::InsufficientFunds);
        assert_noop!(
            call(signed(1), Call::pay_verification {}),
            Error::<Test>::NoServiceAccount
        );
        assert_noop!(
            call(signed(1), Call::set_service_account { account: acct(9) }),
            BadOrigin
        );
        assert_noop!(
            call(root(), Call::set_service_account { account: acct(9) }),
            Error::<Test>::NoAccount
        );
        service();
        assert_eq!(ServiceAccount::<Test>::get(), Some(cid(9)));
        assert_ok!(call(signed(1), Call::pay_verification {}));
        assert_eq!((balance(1), balance(9)), (0, 200));
        assert_noop!(
            call(signed(1), Call::pay_verification {}),
            Error::<Test>::StartGrantAlreadySpent
        );
        check();
    });
}

#[test]
fn a_member_claims_one_cent_per_minute_of_block_time() {
    new_test_ext(params()).execute_with(|| {
        service();
        member(1, 1);
        set_time(HOUR);
        assert_ok!(call(signed(1), Call::claim {}));
        assert_eq!(balance(1), 60);
        assert_eq!(
            rule_events().last(),
            Some(&RuleEvent::Claimed {
                account: cid(1),
                issued: cents(60),
                repaid: cents(0),
                from: ts(0),
                to: ts(HOUR)
            })
        );
        // A second claim in the same block finds nothing.
        assert_ok!(call(signed(1), Call::claim {}));
        assert_eq!(balance(1), 60);
        // Device time plays no part: only block time moves the claim.
        set_time(HOUR + 90);
        assert_ok!(call(signed(1), Call::claim {}));
        assert_eq!(balance(1), 61);
        assert_eq!(PublicTotals::<Test>::get().issued, 61);
        check();
    });
}

#[test]
fn payments_respect_balance_and_credit_line() {
    new_test_ext(params()).execute_with(|| {
        service();
        member(1, 1);
        member(2, 1);
        purchase(1, 500);
        assert_noop!(transfer(1, 2, 501), Error::<Test>::InsufficientFunds);
        assert_noop!(transfer(1, 1, 1), Error::<Test>::SameAccount);
        assert_noop!(transfer(1, 2, 0), Error::<Test>::ZeroAmount);
        assert_noop!(transfer(1, 3, 1), Error::<Test>::NoAccount);
        assert_ok!(transfer(1, 2, 500));
        assert_eq!((balance(1), balance(2)), (0, 500));

        let line = |limit| Call::set_credit_line {
            account: acct(1),
            limit: cents(limit),
        };
        assert_noop!(call(signed(1), line(400)), BadOrigin);
        assert_noop!(call(root(), line(50_001)), Error::<Test>::CreditLineTooHigh);
        assert_ok!(call(root(), line(400)));
        assert_ok!(transfer(1, 2, 400));
        assert_eq!(balance(1), -400);
        assert_noop!(transfer(1, 2, 1), Error::<Test>::InsufficientFunds);
        // The line cannot be cut below what is drawn.
        assert_noop!(call(root(), line(399)), Error::<Test>::InsufficientFunds);
        // Money moved, none was created.
        assert_eq!(TotalSupply::<Test>::get(), 300 + 500);
        assert_eq!(PublicTotals::<Test>::get().paid, 200 + 900);
        check();
    });
}

#[test]
fn a_loan_needs_both_sides_and_is_repaid_from_claims_without_interest() {
    new_test_ext(params()).execute_with(|| {
        service();
        member(1, 1);
        member(2, 1);
        purchase(2, 1_000);
        let accept = || call(signed(1), Call::accept_loan { lender: acct(2) });
        let offer = |amount| {
            call(
                signed(2),
                Call::offer_loan {
                    borrower: acct(1),
                    amount: cents(amount),
                },
            )
        };
        // Nobody can be made a debtor, nobody a lender, by the other side.
        assert_noop!(accept(), Error::<Test>::NoLoanOffer);
        assert_ok!(offer(100));
        assert_eq!(balance(1), 0);
        assert_ok!(call(
            signed(2),
            Call::withdraw_loan_offer { borrower: acct(1) }
        ));
        assert_noop!(accept(), Error::<Test>::NoLoanOffer);

        // An offer above the funds of the lender fails at acceptance and
        // stays on the books untouched.
        assert_ok!(offer(1_001));
        assert_noop!(accept(), Error::<Test>::InsufficientFunds);
        assert_eq!(LoanOffers::<Test>::get(cid(2), cid(1)), Some(cents(1_001)));

        assert_ok!(offer(100));
        assert_ok!(accept());
        assert_eq!((balance(1), balance(2)), (100, 900));
        assert_eq!(LoanOffers::<Test>::get(cid(2), cid(1)), None);

        // One hour: 60 cents issued, half goes on to the lender.
        set_time(HOUR);
        assert_ok!(call(signed(1), Call::claim {}));
        assert_eq!((balance(1), balance(2)), (130, 930));
        assert_noop!(
            call(signed(1), Call::repay { amount: cents(71) }),
            Error::<Test>::RepaymentExceedsDebt
        );
        assert_ok!(call(signed(1), Call::repay { amount: cents(60) }));
        set_time(2 * HOUR);
        assert_ok!(call(signed(1), Call::claim {}));
        // Lent 100, repaid exactly 100.
        assert_eq!(balance(2), 1_000);
        let state = Accounts::<Test>::get(cid(1)).unwrap();
        assert!(state.loan.outstanding.is_zero() && state.loan.lender.is_none());
        let totals = PublicTotals::<Test>::get();
        assert_eq!((totals.lent, totals.repaid), (100, 100));
        check();
    });
}

#[test]
fn only_the_named_origins_may_use_the_privileged_calls() {
    new_test_ext(params()).execute_with(|| {
        open(1);
        let account = acct(1);
        let privileged = [
            Call::purchase {
                account: account.clone(),
                amount: cents(1),
            },
            Call::attest {
                account: account.clone(),
                attestation: None,
            },
            Call::set_status {
                account: account.clone(),
                status: AccountStatus::Frozen,
            },
            Call::set_credit_line {
                account: account.clone(),
                limit: cents(1),
            },
            Call::set_service_account { account },
            Call::enact_params { params: params() },
        ];
        for privileged_call in privileged {
            assert_noop!(call(signed(1), privileged_call), BadOrigin);
        }
        // And the other way round: root is not a person.
        assert_noop!(call(root(), Call::claim {}), BadOrigin);
    });
}

#[test]
fn new_rules_apply_from_their_start_and_never_before() {
    new_test_ext(params()).execute_with(|| {
        service();
        member(1, 1);
        let mut v2 = params();
        v2.version = ParamsVersion(2);
        v2.valid_from = ts(DAY);
        v2.ladder[1].rate =
            capita_core::types::Rate::new(cents(2), capita_core::types::Duration::MINUTE).unwrap();
        assert_ok!(call(root(), Call::enact_params { params: v2.clone() }));
        assert_eq!(
            rule_events().last(),
            Some(&RuleEvent::ParamsEnacted {
                version: ParamsVersion(2),
                valid_from: ts(DAY)
            })
        );
        set_time(2 * DAY);
        assert_ok!(call(signed(1), Call::claim {}));
        // One day at 1 cent per minute, one day at 2.
        assert_eq!(balance(1), 1_440 + 2_880);

        let mut retroactive = v2.clone();
        retroactive.version = ParamsVersion(3);
        retroactive.valid_from = ts(DAY + 1);
        let mut same_version = v2;
        same_version.valid_from = ts(3 * DAY);
        for refused in [retroactive, same_version] {
            assert_noop!(
                call(root(), Call::enact_params { params: refused }),
                Error::<Test>::InvalidParams
            );
        }
        assert_eq!(ParamsHistory::<Test>::get().len(), 2);
        check();
    });
}

#[test]
fn demurrage_is_settled_by_the_pallet_before_money_moves() {
    new_test_ext(params_with_demurrage()).execute_with(|| {
        service();
        member(1, 1);
        member(2, 1);
        purchase(1, 101_000);
        let supply = TotalSupply::<Test>::get();
        set_time(MONTH);
        // 1 % of the 100 000 cents above the free allowance is burnt first.
        assert_ok!(transfer(1, 2, 10));
        assert_eq!(balance(1), 101_000 - 1_000 - 10);
        assert_eq!(balance(2), 10);
        assert_eq!(TotalSupply::<Test>::get(), supply - 1_000);
        assert_eq!(PublicTotals::<Test>::get().demurrage, 1_000);
        // A claim settles too – also the lender it repays.
        set_time(MONTH + HOUR);
        assert_ok!(call(signed(2), Call::claim {}));
        assert_eq!(
            Accounts::<Test>::get(cid(2)).unwrap().demurrage_settled_at,
            ts(MONTH + HOUR)
        );
        check();
    });
}

#[test]
fn frozen_and_closed_accounts_receive_nothing() {
    new_test_ext(params()).execute_with(|| {
        service();
        member(1, 1);
        member(2, 1);
        purchase(2, 50);
        let status = |status| Call::set_status {
            account: acct(1),
            status,
        };
        set_time(HOUR);
        assert_ok!(call(root(), status(AccountStatus::Frozen)));
        set_time(5 * HOUR);
        assert_noop!(
            call(signed(1), Call::claim {}),
            Error::<Test>::AccountNotActive
        );
        assert_noop!(transfer(2, 1, 1), Error::<Test>::AccountNotActive);
        assert_ok!(call(root(), status(AccountStatus::Active)));
        set_time(6 * HOUR);
        assert_ok!(call(signed(1), Call::claim {}));
        // The hour before the freeze and the hour after it; nothing between.
        assert_eq!(balance(1), 120);

        // Exit by the holder is final.
        assert_ok!(call(signed(1), Call::close {}));
        assert_noop!(
            call(signed(1), Call::claim {}),
            Error::<Test>::AccountNotActive
        );
        assert_noop!(
            call(root(), status(AccountStatus::Active)),
            Error::<Test>::AccountNotActive
        );
        check();
    });
}

#[test]
fn one_person_holds_one_account() {
    new_test_ext(params()).execute_with(|| {
        open(1);
        open(2);
        let attest = |n: u8, attestation| {
            call(
                root(),
                Call::attest {
                    account: acct(n),
                    attestation,
                },
            )
        };
        assert_ok!(attest(1, Some(attestation(7, 1))));
        assert_eq!(PersonAccount::<Test>::get(PersonId([7; 32])), Some(cid(1)));
        assert_noop!(
            attest(2, Some(attestation(7, 1))),
            Error::<Test>::DuplicatePerson
        );
        assert_noop!(
            attest(1, Some(attestation(8, 1))),
            Error::<Test>::PersonMismatch
        );
        // A step needs its level; a revoked verification loses it.
        assert_noop!(
            call(signed(1), Call::set_tier { tier: TierId(2) }),
            Error::<Test>::LevelTooLow
        );
        assert_ok!(call(signed(1), Call::set_tier { tier: TierId(1) }));
        assert_ok!(attest(1, None));
        assert_noop!(
            call(signed(1), Call::set_tier { tier: TierId(1) }),
            Error::<Test>::LevelTooLow
        );
        check();
    });
}

#[test]
fn pallet_events_report_offers() {
    new_test_ext(params()).execute_with(|| {
        open(1);
        open(2);
        assert_ok!(call(
            signed(2),
            Call::offer_loan {
                borrower: acct(1),
                amount: cents(5)
            }
        ));
        System::assert_last_event(
            Event::LoanOffered {
                lender: cid(2),
                borrower: cid(1),
                amount: cents(5),
            }
            .into(),
        );
    });
}

/// A small deterministic random number generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below
    }

    fn who(&mut self) -> u8 {
        u8::try_from(self.next(4)).unwrap() + 1
    }
}

/// Whatever people and authorities do, in whatever order, the state of the
/// chain keeps every invariant after every single call – successful or not.
fn random_history(rules: capita_core::params::Params, seed: u64, steps: u32) -> Totals {
    new_test_ext(rules).execute_with(|| {
        service();
        for n in 1..=4 {
            open(n);
        }
        let mut rng = Rng(seed);
        let mut clock = 0;
        for _ in 0..steps {
            clock += rng.next(2 * DAY);
            set_time(clock);
            let (a, b) = (rng.who(), rng.who());
            let amount = cents(rng.next(3_000));
            let _ = match rng.next(16) {
                0..=3 => call(signed(a), Call::claim {}),
                4..=5 => call(
                    signed(a),
                    Call::transfer {
                        to: acct(b),
                        amount,
                    },
                ),
                6 => call(
                    signed(a),
                    Call::offer_loan {
                        borrower: acct(b),
                        amount,
                    },
                ),
                7 => call(signed(a), Call::accept_loan { lender: acct(b) }),
                8 => call(signed(a), Call::repay { amount }),
                9 => call(signed(a), Call::pay_verification {}),
                10 => {
                    let tier = TierId(u16::try_from(rng.next(4)).unwrap());
                    call(signed(a), Call::set_tier { tier })
                }
                11 => call(
                    root(),
                    Call::set_credit_line {
                        account: acct(a),
                        limit: amount,
                    },
                ),
                12 => call(
                    root(),
                    Call::purchase {
                        account: acct(a),
                        amount,
                    },
                ),
                13..=14 => {
                    let level = u8::try_from(rng.next(3)).unwrap();
                    let attestation = Some(attestation(a, level));
                    call(
                        root(),
                        Call::attest {
                            account: acct(a),
                            attestation,
                        },
                    )
                }
                _ => {
                    let status = match rng.next(40) {
                        0 => AccountStatus::Closed,
                        1..=9 => AccountStatus::Frozen,
                        _ => AccountStatus::Active,
                    };
                    call(
                        root(),
                        Call::set_status {
                            account: acct(a),
                            status,
                        },
                    )
                }
            };
            check();
        }
        PublicTotals::<Test>::get()
    })
}

/// The histories were not empty: every kind of money movement happened.
fn assert_eventful(histories: &[Totals]) {
    let sum = |field: fn(&Totals) -> u128| histories.iter().map(field).sum::<u128>();
    assert!(sum(|t| t.issued) > 0 && sum(|t| t.purchased) > 0 && sum(|t| t.paid) > 0);
    assert!(sum(|t| t.lent) > 0 && sum(|t| t.repaid) > 0);
}

#[test]
fn random_histories_keep_every_invariant() {
    let histories: Vec<_> = (1..=12)
        .map(|seed| random_history(params(), seed, 400))
        .collect();
    assert_eventful(&histories);
}

#[test]
fn random_histories_with_demurrage_and_ripening_keep_every_invariant() {
    let histories: Vec<_> = (1..=12)
        .map(|seed| {
            let mut rules = params_with_demurrage();
            rules.demurrage.free_allowance = cents(100);
            rules.ripening = capita_core::types::Duration::from_secs(seed * HOUR);
            random_history(rules, 100 + seed, 400)
        })
        .collect();
    assert_eventful(&histories);
    assert!(histories.iter().any(|t| t.demurrage > 0));
}
