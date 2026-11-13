# ADR 0007 – Demurrage: linear, lazy, remainder carried

Status: accepted as technical frame (M1, 2026-11-13). Whether demurrage is used at all is open (B2); default off.

## Decision
- Charge for a span of time = (balance − free allowance) × rate × time ÷ period, rounded down. Linear, as with the stamps of Wörgl; no exponential, hence no fixed-point library and no floats.
- Lazy: nothing is charged until the account is settled (`plan_demurrage`). With demurrage on, the core refuses to move money on an account not settled up to `now` – so the charge is always computed on the balance actually held.
- The sub-cent remainder is kept per account (`demurrage_carry`) and added at the next settlement. Settling often cannot round the charge away.
- Never more than the chargeable balance; negative balances are not charged. Once rate × time reaches 100 %, the charge is everything chargeable – this also rules out overflow.
- Time under a parameter version without demurrage costs nothing: switching it on never reaches back. The remainder is dropped at a change of version.
- The charge is burnt (`Burn`, reason `Demurrage`): supply = minted − burnt.

## Consequences
+ Integer-exact, identical on every host. + Supply stays auditable.
− Linear between settlements: settling often charges marginally less than settling once (the charge shrinks with the balance). Bounded and in favour of the holder.
− On Taler, coins in a wallet are out of reach (ADR 0004).

## Sources of the idea
encointer (lazy demurrage from balance and last update), Gesell, Wörgl. Ideas only, no code taken.
