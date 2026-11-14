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

//! # pallet-capita
//!
//! The monetary rules of [Capita Money](https://capitamoney.com) inside a
//! Substrate runtime. The pallet decides nothing itself: every call loads
//! state, asks `capita-core` for a plan and applies it – the same rules and
//! the same code as on a service host (`docs/architecture.md`).
//!
//! What the pallet adds is what a host has to supply:
//! - **storage**: account states, the versions of the rules, the public
//!   totals;
//! - **the ledger**: a balance per account that may go below zero down to
//!   the credit line (which is why `pallet-balances` is not used, see
//!   `docs/adr/0009`);
//! - **time**: the block timestamp, never a device clock;
//! - **authority**: who may change rules, attest persons, grant credit
//!   lines and mint purchased money – each an origin chosen by the runtime;
//! - **consent**: a loan needs an offer of the lender and the acceptance of
//!   the borrower.
//!
//! A call is applied as a whole or not at all (FRAME calls are storage
//! transactions).
//!
//! Open: weights are placeholders (no benchmarks yet); there are no
//! transaction fees – protection against spam is a matter for the runtime.

#![cfg_attr(not(feature = "std"), no_std)]
// FRAME derives `Clone` for events and calls field by field. Clippy versions
// after 1.97 look into that generated code and report every `Copy` field.
#![allow(clippy::clone_on_copy)]
#![forbid(unsafe_code)]

extern crate alloc;

pub use pallet::*;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

#[frame_support::pallet]
pub mod pallet {
    use alloc::vec::Vec;

    use capita_core::account::{AccountState, AccountStatus};
    use capita_core::attributes::Attestation;
    use capita_core::errors::{ArithmeticError, CoreError};
    use capita_core::params::{in_force_at, Params};
    use capita_core::plan::Plan;
    use capita_core::ports::{Event as RuleEvent, LedgerOp, TierGate};
    use capita_core::totals::Totals;
    use capita_core::types::{AccountId as CoreId, Amount, Balance, PersonId, TierId, Timestamp};
    use capita_core::{demurrage, issuance, lifecycle, loans, recovery};
    use frame_support::pallet_prelude::*;
    use frame_support::traits::UnixTime;
    use frame_system::pallet_prelude::*;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    /// What the runtime supplies. Account identifiers must be 32 bytes, the
    /// form the rules use.
    #[pallet::config]
    pub trait Config:
        frame_system::Config<RuntimeEvent: From<Event<Self>>, AccountId: Into<[u8; 32]>>
    {
        /// The clock: block time (`pallet-timestamp`).
        type Time: UnixTime;
        /// Who may put a new version of the rules into force.
        ///
        /// Offene Frage: E1 – who changes the rules.
        type GovernanceOrigin: EnsureOrigin<Self::RuntimeOrigin>;
        /// Who may attest persons and freeze or close accounts: the identity
        /// adapter (a credential pallet, a committee, …).
        ///
        /// Offene Frage: F3 – identity provider; B5 – who freezes and closes.
        type IdentityOrigin: EnsureOrigin<Self::RuntimeOrigin>;
        /// Who may mint purchased money after a payment in ordinary money
        /// has been confirmed outside the chain.
        type PurchaseOrigin: EnsureOrigin<Self::RuntimeOrigin>;
        /// Who may grant and change credit lines.
        ///
        /// Offene Frage: C1 – who grants credit lines.
        type CreditOrigin: EnsureOrigin<Self::RuntimeOrigin>;
        /// Whether a step of the ladder is unlocked.
        ///
        /// Offene Frage: B3 – default `capita_core::ports::NoGate`.
        type TierGate: TierGate + Default;
    }

    /// State of every account, as the rules keep it.
    #[pallet::storage]
    pub type Accounts<T: Config> = StorageMap<_, Blake2_128Concat, CoreId, AccountState>;

    /// The ledger: balance per account in cents. Negative = drawn credit
    /// line.
    #[pallet::storage]
    pub type Balances<T: Config> = StorageMap<_, Blake2_128Concat, CoreId, Balance, ValueQuery>;

    /// Money in circulation in cents, counted by the ledger: everything
    /// minted minus everything burnt. Must equal the supply of
    /// [`PublicTotals`], which is counted from the events.
    #[pallet::storage]
    pub type TotalSupply<T: Config> = StorageValue<_, u128, ValueQuery>;

    /// Every version of the rules, oldest first. Kept in full: a claim or a
    /// demurrage settlement may reach back across several versions.
    #[pallet::storage]
    #[pallet::unbounded]
    pub type ParamsHistory<T: Config> = StorageValue<_, Vec<Params>, ValueQuery>;

    /// The account each verified person holds: one person, one account.
    #[pallet::storage]
    pub type PersonAccount<T: Config> = StorageMap<_, Blake2_128Concat, PersonId, CoreId>;

    /// Accounts that moved to a new key: the closed account, and the
    /// account of its person now. What is paid to the first reaches the
    /// second.
    #[pallet::storage]
    pub type MovedTo<T: Config> = StorageMap<_, Blake2_128Concat, CoreId, CoreId>;

    /// Public sums over all events (transparency).
    #[pallet::storage]
    pub type PublicTotals<T: Config> = StorageValue<_, Totals, ValueQuery>;

    /// The account of the verification service: the only place a start
    /// grant can be paid to.
    #[pallet::storage]
    pub type ServiceAccount<T: Config> = StorageValue<_, CoreId>;

    /// Loans offered and not yet accepted: lender, borrower, amount.
    #[pallet::storage]
    pub type LoanOffers<T: Config> =
        StorageDoubleMap<_, Blake2_128Concat, CoreId, Blake2_128Concat, CoreId, Amount>;

    /// The rules the chain starts with.
    #[pallet::genesis_config]
    #[derive(frame_support::DefaultNoBound)]
    pub struct GenesisConfig<T: Config> {
        /// First version of the rules. Without it nothing works until
        /// governance enacts one.
        pub params: Option<Params>,
        #[serde(skip)]
        pub _config: core::marker::PhantomData<T>,
    }

    #[pallet::genesis_build]
    impl<T: Config> BuildGenesisConfig for GenesisConfig<T> {
        fn build(&self) {
            if let Some(params) = &self.params {
                assert!(params.validate().is_ok(), "invalid genesis parameters");
                ParamsHistory::<T>::put(alloc::vec![params.clone()]);
            }
        }
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// A fact from the rules, exactly as `capita-core` records it.
        Rule(RuleEvent),
        /// A lender offered a loan.
        LoanOffered {
            /// Who offers.
            lender: CoreId,
            /// To whom.
            borrower: CoreId,
            /// How much.
            amount: Amount,
        },
        /// A lender withdrew an offer.
        LoanOfferWithdrawn {
            /// Who had offered.
            lender: CoreId,
            /// To whom.
            borrower: CoreId,
        },
        /// The account of the verification service was set.
        ServiceAccountSet {
            /// The account.
            account: CoreId,
        },
    }

    /// Errors of the pallet itself, followed by the errors of the rules
    /// (`capita_core::errors::CoreError`, one variant each).
    #[pallet::error]
    pub enum Error<T> {
        /// No version of the rules has been enacted yet.
        NoParams,
        /// The account has not been opened.
        NoAccount,
        /// The account has been opened before.
        AccountExists,
        /// No verification service account has been set.
        NoServiceAccount,
        /// There is no loan offer from this lender.
        NoLoanOffer,
        /// A calculation left the range of its integer type.
        Overflow,
        /// A calculation fell below the range of its integer type.
        Underflow,
        /// Division by zero.
        DivisionByZero,
        /// The time is earlier than one already seen for the account.
        ClockWentBackwards,
        /// A value violates its documented range.
        OutOfRange,
        /// The parameters break a structural rule.
        InvalidParams,
        /// No parameter version covers the requested time.
        NoParamsInForce,
        /// The stored account state contradicts itself.
        InvalidState,
        /// The account is frozen or closed.
        AccountNotActive,
        /// An amount of zero where money has to move.
        ZeroAmount,
        /// Both sides are the same account.
        SameAccount,
        /// The balance would fall below minus the credit line, or an unspent
        /// start grant would be touched.
        InsufficientFunds,
        /// Demurrage has not been settled.
        DemurrageNotSettled,
        /// The step is not on the ladder.
        UnknownTier,
        /// The verification level does not reach the step.
        LevelTooLow,
        /// The step is not unlocked yet.
        TierGated,
        /// The attestation belongs to another person than the account.
        PersonMismatch,
        /// The person already holds another account.
        DuplicatePerson,
        /// The attestation is not valid at this time.
        AttestationNotValid,
        /// The account to move is not the account of the person named.
        NotAccountOfPerson,
        /// The account to move to belongs to a person already.
        MoveTargetBound,
        /// An account with a credit line or recorded debt cannot move.
        MoveWithDebt,
        /// The credit line exceeds the maximum.
        CreditLineTooHigh,
        /// The loan would exceed the maximum recorded debt.
        DebtLimitExceeded,
        /// The account already owes money to another lender.
        OtherLenderOutstanding,
        /// More would be repaid than is owed.
        RepaymentExceedsDebt,
        /// The start grant has been spent before.
        StartGrantAlreadySpent,
        /// A sponsor filter uses an attribute key that is not published.
        UnknownSponsorKey,
        /// The claim would have to be split into too many pieces.
        TooManySegments,
    }

    impl<T> From<CoreError> for Error<T> {
        fn from(error: CoreError) -> Self {
            match error {
                CoreError::Arithmetic(ArithmeticError::Overflow) => Self::Overflow,
                CoreError::Arithmetic(ArithmeticError::Underflow) => Self::Underflow,
                CoreError::Arithmetic(ArithmeticError::DivisionByZero) => Self::DivisionByZero,
                CoreError::ClockWentBackwards { .. } => Self::ClockWentBackwards,
                CoreError::OutOfRange(_) => Self::OutOfRange,
                CoreError::InvalidParams(_) => Self::InvalidParams,
                CoreError::NoParamsInForce => Self::NoParamsInForce,
                CoreError::InvalidState(_) => Self::InvalidState,
                CoreError::AccountNotActive(_) => Self::AccountNotActive,
                CoreError::ZeroAmount => Self::ZeroAmount,
                CoreError::SameAccount => Self::SameAccount,
                CoreError::InsufficientFunds => Self::InsufficientFunds,
                CoreError::DemurrageNotSettled => Self::DemurrageNotSettled,
                CoreError::UnknownTier => Self::UnknownTier,
                CoreError::LevelTooLow => Self::LevelTooLow,
                CoreError::TierGated => Self::TierGated,
                CoreError::PersonMismatch => Self::PersonMismatch,
                CoreError::DuplicatePerson => Self::DuplicatePerson,
                CoreError::AttestationNotValid => Self::AttestationNotValid,
                CoreError::NotAccountOfPerson => Self::NotAccountOfPerson,
                CoreError::MoveTargetBound => Self::MoveTargetBound,
                CoreError::MoveWithDebt => Self::MoveWithDebt,
                CoreError::CreditLineTooHigh => Self::CreditLineTooHigh,
                CoreError::DebtLimitExceeded => Self::DebtLimitExceeded,
                CoreError::OtherLenderOutstanding => Self::OtherLenderOutstanding,
                CoreError::RepaymentExceedsDebt => Self::RepaymentExceedsDebt,
                CoreError::StartGrantAlreadySpent => Self::StartGrantAlreadySpent,
                CoreError::UnknownSponsorKey => Self::UnknownSponsorKey,
                CoreError::TooManySegments => Self::TooManySegments,
            }
        }
    }

    /// Most keys one person can lose in a row before payments to the
    /// oldest account stop being forwarded.
    const MAX_MOVES: u32 = 64;

    /// Placeholder weight until the pallet is benchmarked: a fixed base plus
    /// the database accesses of the call.
    fn weight_of<T: Config>(reads: u64, writes: u64) -> Weight {
        Weight::from_parts(50_000_000, 0)
            .saturating_add(T::DbWeight::get().reads_writes(reads, writes))
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        #[cfg(feature = "try-runtime")]
        fn try_state(_n: BlockNumberFor<T>) -> Result<(), sp_runtime::TryRuntimeError> {
            Self::do_try_state().map_err(Into::into)
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Open an account for the caller: lowest step of the ladder, and
        /// the one-time start grant.
        #[pallet::call_index(0)]
        #[pallet::weight(weight_of::<T>(5, 6))]
        pub fn open(origin: OriginFor<T>) -> DispatchResult {
            let account = ensure_signed(origin)?;
            let who = Self::id(account.clone());
            ensure!(!Accounts::<T>::contains_key(who), Error::<T>::AccountExists);
            let (now, list) = Self::context()?;
            let params = in_force_at(&list, now).map_err(Self::rule)?;
            Self::apply(lifecycle::plan_open(who, params, now).map_err(Self::rule)?)?;
            // For the rest of the chain a key exists once the system pallet
            // counts a provider for it; other pallets ask there (Duniter's
            // identity pallet before it creates an identity). The Capita
            // account is that provider. Its record is never deleted, so
            // the reference stays.
            frame_system::Pallet::<T>::inc_providers(&account);
            Ok(())
        }

        /// Claim the per-capita money accrued since the last claim. While
        /// debt is recorded, a share of it goes on to the lender.
        #[pallet::call_index(1)]
        #[pallet::weight(weight_of::<T>(7, 7))]
        pub fn claim(origin: OriginFor<T>) -> DispatchResult {
            let who = Self::id(ensure_signed(origin)?);
            let (now, list) = Self::context()?;
            Self::settle(&who, &list, now)?;
            let state = Self::account(&who)?;
            if let Some(lender) = state.loan.lender {
                Self::settle(&lender, &list, now)?;
            }
            Self::apply(issuance::plan_claim(&state, &list, now).map_err(Self::rule)?)
        }

        /// Pay `amount` to another account. If that account moved to a new
        /// key, the payment reaches the account it moved to.
        #[pallet::call_index(2)]
        #[pallet::weight(weight_of::<T>(9, 7))]
        pub fn transfer(origin: OriginFor<T>, to: T::AccountId, amount: Amount) -> DispatchResult {
            let who = Self::id(ensure_signed(origin)?);
            let to = Self::resolve(Self::id(to));
            let (now, list) = Self::context()?;
            let params = in_force_at(&list, now).map_err(Self::rule)?;
            Self::settle(&who, &list, now)?;
            Self::settle(&to, &list, now)?;
            let (payer, payee) = (Self::account(&who)?, Self::account(&to)?);
            let balance = Balances::<T>::get(who);
            let plan = loans::plan_payment(&payer, balance, &payee, amount, params, now);
            Self::apply(plan.map_err(Self::rule)?)
        }

        /// Pay the start grant to the verification service, for the
        /// verification of the caller. Once only.
        #[pallet::call_index(3)]
        #[pallet::weight(weight_of::<T>(8, 7))]
        pub fn pay_verification(origin: OriginFor<T>) -> DispatchResult {
            let who = Self::id(ensure_signed(origin)?);
            let service = ServiceAccount::<T>::get().ok_or(Error::<T>::NoServiceAccount)?;
            let (now, list) = Self::context()?;
            let params = in_force_at(&list, now).map_err(Self::rule)?;
            Self::settle(&who, &list, now)?;
            Self::settle(&service, &list, now)?;
            let (state, service) = (Self::account(&who)?, Self::account(&service)?);
            let balance = Balances::<T>::get(who);
            let plan = loans::plan_spend_start_grant(&state, balance, &service, params, now);
            Self::apply(plan.map_err(Self::rule)?)
        }

        /// Move the caller to another step of the incentive ladder.
        #[pallet::call_index(4)]
        #[pallet::weight(weight_of::<T>(3, 3))]
        pub fn set_tier(origin: OriginFor<T>, tier: TierId) -> DispatchResult {
            let who = Self::id(ensure_signed(origin)?);
            let (now, list) = Self::context()?;
            let state = Self::account(&who)?;
            let gate = T::TierGate::default();
            let plan = lifecycle::plan_set_tier(&state, &list, &gate, tier, now);
            Self::apply(plan.map_err(Self::rule)?)
        }

        /// Offer an interest-free loan to `borrower`. Nothing moves until
        /// the borrower accepts; a new offer replaces an earlier one.
        #[pallet::call_index(5)]
        #[pallet::weight(weight_of::<T>(2, 1))]
        pub fn offer_loan(
            origin: OriginFor<T>,
            borrower: T::AccountId,
            amount: Amount,
        ) -> DispatchResult {
            let (lender, borrower) = (Self::id(ensure_signed(origin)?), Self::id(borrower));
            ensure!(lender != borrower, Error::<T>::SameAccount);
            ensure!(!amount.is_zero(), Error::<T>::ZeroAmount);
            Self::account(&lender)?;
            Self::account(&borrower)?;
            LoanOffers::<T>::insert(lender, borrower, amount);
            Self::deposit_event(Event::LoanOffered {
                lender,
                borrower,
                amount,
            });
            Ok(())
        }

        /// Withdraw an offer that has not been accepted.
        #[pallet::call_index(6)]
        #[pallet::weight(weight_of::<T>(1, 1))]
        pub fn withdraw_loan_offer(origin: OriginFor<T>, borrower: T::AccountId) -> DispatchResult {
            let (lender, borrower) = (Self::id(ensure_signed(origin)?), Self::id(borrower));
            LoanOffers::<T>::take(lender, borrower).ok_or(Error::<T>::NoLoanOffer)?;
            Self::deposit_event(Event::LoanOfferWithdrawn { lender, borrower });
            Ok(())
        }

        /// Accept the loan `lender` offered to the caller. The money moves,
        /// the debt is recorded and repaid from a share of each claim.
        #[pallet::call_index(7)]
        #[pallet::weight(weight_of::<T>(8, 7))]
        pub fn accept_loan(origin: OriginFor<T>, lender: T::AccountId) -> DispatchResult {
            let (who, lender) = (Self::id(ensure_signed(origin)?), Self::id(lender));
            let amount = LoanOffers::<T>::take(lender, who).ok_or(Error::<T>::NoLoanOffer)?;
            let (now, list) = Self::context()?;
            let params = in_force_at(&list, now).map_err(Self::rule)?;
            Self::settle(&who, &list, now)?;
            Self::settle(&lender, &list, now)?;
            let (borrower, giver) = (Self::account(&who)?, Self::account(&lender)?);
            let funds = Balances::<T>::get(lender);
            let plan = loans::plan_lend(&borrower, &giver, funds, amount, params, now);
            Self::apply(plan.map_err(Self::rule)?)
        }

        /// Repay recorded debt voluntarily, on top of what claims repay.
        #[pallet::call_index(8)]
        #[pallet::weight(weight_of::<T>(7, 7))]
        pub fn repay(origin: OriginFor<T>, amount: Amount) -> DispatchResult {
            let who = Self::id(ensure_signed(origin)?);
            let (now, list) = Self::context()?;
            let params = in_force_at(&list, now).map_err(Self::rule)?;
            Self::settle(&who, &list, now)?;
            let state = Self::account(&who)?;
            if let Some(lender) = state.loan.lender {
                Self::settle(&lender, &list, now)?;
            }
            let balance = Balances::<T>::get(who);
            let plan = loans::plan_repay(&state, balance, amount, params, now);
            Self::apply(plan.map_err(Self::rule)?)
        }

        /// Close the account of the caller for good (exit).
        #[pallet::call_index(9)]
        #[pallet::weight(weight_of::<T>(5, 6))]
        pub fn close(origin: OriginFor<T>) -> DispatchResult {
            let who = Self::id(ensure_signed(origin)?);
            Self::do_set_status(who, AccountStatus::Closed)
        }

        /// Grant or change the credit line of an account.
        #[pallet::call_index(20)]
        #[pallet::weight(weight_of::<T>(3, 3))]
        pub fn set_credit_line(
            origin: OriginFor<T>,
            account: T::AccountId,
            limit: Amount,
        ) -> DispatchResult {
            T::CreditOrigin::ensure_origin(origin)?;
            let account = Self::id(account);
            let (now, list) = Self::context()?;
            let params = in_force_at(&list, now).map_err(Self::rule)?;
            let state = Self::account(&account)?;
            let balance = Balances::<T>::get(account);
            let plan = loans::plan_set_credit_line(&state, balance, limit, params, now);
            Self::apply(plan.map_err(Self::rule)?)
        }

        /// Mint money bought with ordinary money, after the payment has been
        /// confirmed outside the chain. There is no call for the way back.
        #[pallet::call_index(21)]
        #[pallet::weight(weight_of::<T>(5, 6))]
        pub fn purchase(
            origin: OriginFor<T>,
            account: T::AccountId,
            amount: Amount,
        ) -> DispatchResult {
            T::PurchaseOrigin::ensure_origin(origin)?;
            let account = Self::id(account);
            let (now, list) = Self::context()?;
            let params = in_force_at(&list, now).map_err(Self::rule)?;
            Self::settle(&account, &list, now)?;
            let state = Self::account(&account)?;
            let plan = issuance::plan_purchase(&state, params, amount, now);
            Self::apply(plan.map_err(Self::rule)?)
        }

        /// Record what the identity provider states about the holder of an
        /// account. `None`: the verification was revoked.
        #[pallet::call_index(22)]
        #[pallet::weight(weight_of::<T>(4, 4))]
        pub fn attest(
            origin: OriginFor<T>,
            account: T::AccountId,
            attestation: Option<Attestation>,
        ) -> DispatchResult {
            T::IdentityOrigin::ensure_origin(origin)?;
            Self::do_attest(Self::id(account), attestation)
        }

        /// Freeze, reopen or close an account.
        #[pallet::call_index(23)]
        #[pallet::weight(weight_of::<T>(5, 6))]
        pub fn set_status(
            origin: OriginFor<T>,
            account: T::AccountId,
            status: AccountStatus,
        ) -> DispatchResult {
            T::IdentityOrigin::ensure_origin(origin)?;
            Self::do_set_status(Self::id(account), status)
        }

        /// Put a new version of the rules into force from
        /// `params.valid_from`: valid in itself, a higher version, a start
        /// that is not in the past. Rules never change retroactively.
        #[pallet::call_index(30)]
        #[pallet::weight(weight_of::<T>(1, 1))]
        pub fn enact_params(origin: OriginFor<T>, params: Params) -> DispatchResult {
            T::GovernanceOrigin::ensure_origin(origin)?;
            let now = Self::now();
            let mut list = ParamsHistory::<T>::get();
            match list.last() {
                Some(previous) => params.validate_successor(previous, now),
                None => params.validate(),
            }
            .map_err(Self::rule)?;
            let event = RuleEvent::ParamsEnacted {
                version: params.version,
                valid_from: params.valid_from,
            };
            list.push(params);
            ParamsHistory::<T>::put(list);
            Self::deposit_event(Event::Rule(event));
            Ok(())
        }

        /// Name the account of the verification service.
        #[pallet::call_index(31)]
        #[pallet::weight(weight_of::<T>(1, 1))]
        pub fn set_service_account(origin: OriginFor<T>, account: T::AccountId) -> DispatchResult {
            T::GovernanceOrigin::ensure_origin(origin)?;
            let account = Self::id(account);
            Self::account(&account)?;
            ServiceAccount::<T>::put(account);
            Self::deposit_event(Event::ServiceAccountSet { account });
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        /// The account identifier of the rules for an account of the chain.
        pub fn id(who: T::AccountId) -> CoreId {
            CoreId(who.into())
        }

        /// Block time in whole seconds.
        pub fn now() -> Timestamp {
            Timestamp::from_secs(T::Time::now().as_secs())
        }

        /// Records a statement of the identity provider. For identity
        /// pallets of the same runtime; the call `attest` does the same for
        /// an origin.
        pub fn do_attest(account: CoreId, attestation: Option<Attestation>) -> DispatchResult {
            let (now, list) = Self::context()?;
            let state = Self::account(&account)?;
            let attestation = attestation.as_ref();
            let held = attestation.and_then(|a| Self::holder_of(&a.person));
            let plan = lifecycle::plan_attest(&state, &list, attestation, held, now);
            let plan = plan.map_err(Self::rule)?;
            if let Some(attestation) = attestation {
                PersonAccount::<T>::insert(attestation.person, account);
            }
            Self::apply(plan)
        }

        /// The account a person holds, if any: one person, one account. An
        /// identifier the person was known under before names the same
        /// account.
        pub fn holder_of(person: &PersonId) -> Option<CoreId> {
            PersonAccount::<T>::get(person).map(Self::resolve)
        }

        /// Where an account is now: itself, or – if it moved to a new key,
        /// perhaps more than once – the account at the end of that chain.
        pub fn resolve(mut id: CoreId) -> CoreId {
            // Bounded: one step per lost key of one person.
            for _ in 0..MAX_MOVES {
                match MovedTo::<T>::get(id) {
                    Some(next) => id = next,
                    None => break,
                }
            }
            id
        }

        /// Moves the account `from` to the account `to`, which its person
        /// opened with a new key, on the strength of a new statement of an
        /// identity service (`capita_core::recovery`). For identity pallets
        /// of the same runtime: they decide whether and when.
        pub fn do_move(from: CoreId, to: CoreId, attestation: &Attestation) -> DispatchResult {
            let (now, list) = Self::context()?;
            Self::settle(&from, &list, now)?;
            Self::settle(&to, &list, now)?;
            let (old, new) = (Self::account(&from)?, Self::account(&to)?);
            // One person, one account: whoever the statement names may
            // hold no third account.
            ensure!(
                Self::holder_of(&attestation.person).is_none_or(|holder| holder == from),
                Error::<T>::DuplicatePerson
            );
            let balance = Balances::<T>::get(from);
            let plan = recovery::plan_move(&old, balance, &new, &list, attestation, now)
                .map_err(Self::rule)?;
            ensure!(
                old.revision.checked_add(1) == Some(plan.from.revision)
                    && new.revision.checked_add(1) == Some(plan.to.revision),
                Error::<T>::InvalidState
            );
            for op in &plan.ops {
                Self::apply_op(op)?;
            }
            let mut totals = PublicTotals::<T>::get();
            for event in &plan.events {
                totals.record(event).map_err(|e| Self::rule(e.into()))?;
            }
            PublicTotals::<T>::put(totals);
            Accounts::<T>::insert(from, &plan.from);
            Accounts::<T>::insert(to, &plan.to);
            PersonAccount::<T>::insert(attestation.person, to);
            MovedTo::<T>::insert(from, to);
            for event in plan.events {
                Self::deposit_event(Event::Rule(event));
            }
            Ok(())
        }

        /// Changes the life-cycle state of an account. For identity pallets
        /// of the same runtime.
        pub fn do_set_status(account: CoreId, status: AccountStatus) -> DispatchResult {
            let (now, list) = Self::context()?;
            Self::settle(&account, &list, now)?;
            let state = Self::account(&account)?;
            let balance = Balances::<T>::get(account);
            let plan = lifecycle::plan_set_status(&state, balance, &list, status, now);
            Self::apply(plan.map_err(Self::rule)?)
        }

        /// Time and rules of this call.
        fn context() -> Result<(Timestamp, Vec<Params>), DispatchError> {
            let list = ParamsHistory::<T>::get();
            ensure!(!list.is_empty(), Error::<T>::NoParams);
            Ok((Self::now(), list))
        }

        fn account(id: &CoreId) -> Result<AccountState, DispatchError> {
            Accounts::<T>::get(id).ok_or_else(|| Error::<T>::NoAccount.into())
        }

        fn rule(error: CoreError) -> DispatchError {
            Error::<T>::from(error).into()
        }

        /// Settles demurrage on an account before money moves on it. Does
        /// nothing while demurrage is switched off.
        fn settle(id: &CoreId, list: &[Params], now: Timestamp) -> DispatchResult {
            let params = in_force_at(list, now).map_err(Self::rule)?;
            if params.demurrage.rate.is_zero() {
                return Ok(());
            }
            let Some(state) = Accounts::<T>::get(id) else {
                return Ok(());
            };
            if state.status == AccountStatus::Closed || state.demurrage_settled_at == now {
                return Ok(());
            }
            let balance = Balances::<T>::get(id);
            let plan = demurrage::plan_demurrage(&state, balance, list, now);
            Self::apply(plan.map_err(Self::rule)?)
        }

        /// Applies a plan: ledger operations, public totals, account state,
        /// events.
        fn apply(plan: Plan) -> DispatchResult {
            let stored = Accounts::<T>::get(plan.state.id).map_or(0, |s| s.revision);
            ensure!(
                stored.checked_add(1) == Some(plan.state.revision),
                Error::<T>::InvalidState
            );
            for op in &plan.ops {
                Self::apply_op(op)?;
            }
            let mut totals = PublicTotals::<T>::get();
            for event in &plan.events {
                totals.record(event).map_err(|e| Self::rule(e.into()))?;
            }
            PublicTotals::<T>::put(totals);
            Accounts::<T>::insert(plan.state.id, &plan.state);
            for event in plan.events {
                Self::deposit_event(Event::Rule(event));
            }
            Ok(())
        }

        fn apply_op(op: &LedgerOp) -> DispatchResult {
            match *op {
                LedgerOp::Mint { to, amount, .. } => {
                    Self::credit(&to, amount)?;
                    let supply = TotalSupply::<T>::get()
                        .checked_add(u128::from(amount.cents()))
                        .ok_or(Error::<T>::Overflow)?;
                    TotalSupply::<T>::put(supply);
                }
                LedgerOp::Burn { from, amount, .. } => {
                    Self::debit(&from, amount)?;
                    let supply = TotalSupply::<T>::get()
                        .checked_sub(u128::from(amount.cents()))
                        .ok_or(Error::<T>::Underflow)?;
                    TotalSupply::<T>::put(supply);
                }
                LedgerOp::Transfer {
                    from, to, amount, ..
                } => {
                    Self::debit(&from, amount)?;
                    Self::credit(&to, amount)?;
                }
                // The credit line lives in the account state.
                LedgerOp::SetCreditLine { .. } => {}
            }
            Ok(())
        }

        /// Money for an account that moved to a new key reaches the
        /// account it moved to (a repayment to a lender, for example).
        fn credit(id: &CoreId, amount: Amount) -> DispatchResult {
            let id = Self::resolve(*id);
            let balance = Balances::<T>::get(id).checked_credit(amount);
            Balances::<T>::insert(id, balance.map_err(|e| Self::rule(e.into()))?);
            Ok(())
        }

        /// The ledger refuses on its own what would take a balance below
        /// minus its credit line, whatever the plan says.
        fn debit(id: &CoreId, amount: Amount) -> DispatchResult {
            let line = Accounts::<T>::get(id).map_or(Amount::ZERO, |s| s.loan.credit_line);
            let balance = Balances::<T>::get(id);
            balance.check_debit(amount, line).map_err(Self::rule)?;
            let balance = balance.checked_debit(amount);
            Balances::<T>::insert(id, balance.map_err(|e| Self::rule(e.into()))?);
            Ok(())
        }

        /// The invariants of the whole state. Run after every block by
        /// `try-runtime`, and by the tests.
        #[cfg(any(test, feature = "try-runtime"))]
        pub fn do_try_state() -> Result<(), &'static str> {
            let totals = PublicTotals::<T>::get();
            let supply = TotalSupply::<T>::get();
            if totals.supply() != Ok(supply) {
                return Err("supply of the ledger differs from the public totals");
            }
            let mut sum: i128 = 0;
            let mut debt: u128 = 0;
            for (id, balance) in Balances::<T>::iter() {
                sum = sum
                    .checked_add(i128::from(balance.cents()))
                    .ok_or("overflow")?;
                let Some(state) = Accounts::<T>::get(id) else {
                    return Err("balance without an account");
                };
                let free = i128::from(balance.cents())
                    .checked_add(i128::from(state.loan.credit_line.cents()))
                    .ok_or("overflow")?;
                if free < 0 {
                    return Err("balance below the credit line");
                }
                if free < i128::from(state.start_grant_reserved.cents()) {
                    return Err("start grant touched");
                }
            }
            if i128::try_from(supply) != Ok(sum) {
                return Err("sum of balances differs from the supply");
            }
            for (id, state) in Accounts::<T>::iter() {
                if state.id != id {
                    return Err("account stored under a foreign key");
                }
                if state.loan.outstanding.is_zero() != state.loan.lender.is_none() {
                    return Err("debt and lender do not match");
                }
                debt = debt
                    .checked_add(u128::from(state.loan.outstanding.cents()))
                    .ok_or("overflow")?;
                if let Some(person) = state.person {
                    if Self::holder_of(&person) != Some(id) {
                        return Err("person not registered for the account");
                    }
                }
            }
            if totals.outstanding() != Ok(debt) {
                return Err("recorded debt differs from the public totals");
            }
            for (_person, account) in PersonAccount::<T>::iter() {
                let holder = Self::resolve(account);
                let state = Accounts::<T>::get(holder).ok_or("person registered for no account")?;
                // An identifier a person had before points at the account
                // too; the account names the present one.
                if state.person.is_none() {
                    return Err("person registered for an account that is nobody's");
                }
            }
            for (from, to) in MovedTo::<T>::iter() {
                let closed = Accounts::<T>::get(from)
                    .is_some_and(|state| state.status == AccountStatus::Closed);
                if !closed || !Accounts::<T>::contains_key(to) {
                    return Err("an account that moved is not closed, or moved to nowhere");
                }
                if !Balances::<T>::get(from).positive_part().is_zero() {
                    return Err("money left behind on an account that moved");
                }
            }
            let list = ParamsHistory::<T>::get();
            if !list.is_empty() && capita_core::params::check_sequence(&list).is_err() {
                return Err("parameter versions out of order");
            }
            Ok(())
        }
    }
}
