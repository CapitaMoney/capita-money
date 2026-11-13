# ADR 0003 – Integer cents, rational rates, calendar-free time

Status: accepted (M0, 2026-11-13)

## Decision
- Unit: cent. 1 Capita = 100 cents. `Amount` = `u64` (never negative), `Balance` = `i64` (negative = drawn credit line), totals = `u128`.
- No floats anywhere. Rates are rationals (`Rate` = amount per duration). Fractions are parts per million (`Share`).
- Every operation checked: overflow, underflow and division by zero are errors, never wrap, never saturate.
- Products use `u128` intermediates. Rounding always down; remainders carried (ADR 0002).
- Time: whole seconds since the Unix epoch, UTC. No calendar, no time zones: `MONTH` = 2 629 800 s, `YEAR` = 31 557 600 s (365.25 days). A "monthly" amount is per such month.
- The guide value "1 Capita ≈ 1 €/$" is for display. The core contains no conversion.

## Consequences
+ Identical results on every machine and in every runtime. + Supply sums exact to the cent.
− Adapters convert at the edge: Taler amounts (value + 8-digit fraction, value below 2^52), Substrate `u128` balances. − Calendar months differ slightly from core months; user interfaces say "per 30.4 days" or simply show the next amount.

## Range check
`u64` cents ≈ 1.8 × 10^17 Capita per amount – far above any account. World supply (10^10 people × 432 Capita × 12 months × 100 years ≈ 5 × 10^15 Capita) fits `u128` with room to spare.
