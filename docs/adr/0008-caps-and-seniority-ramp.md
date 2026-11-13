# ADR 0008 – Caps: global, per country, seniority ramp

Status: accepted as technical frame (M1, 2026-11-13). Values and details of residence are open (A2, A2b, A3).

## Context
Decided: caps in three phases – one global cap; then a cap per country of residence from GNI per person; then a ramp by seniority. Residence is the only source of the country cap. Moving to a poorer country: its cap at once. Moving to a richer one: the ramp restarts for the difference from the day of the move.

## Decision
- A cap limits the *rate*: effective rate = lower of step rate and cap ÷ cap period. Compared exactly by cross-multiplication.
- Country cap per cap period = GNI per person × share × period ÷ year, never below the global cap. No residence, or a country without entry: the global cap.
- The three phases are three parameter versions (`by_country` empty → filled → `ramp` above zero). No phase logic in code.
- Ramp: per account a starting level and a starting time (`cap_ramp`). The cap rises from the starting level to the target in equal steps, once per cap period, and reaches it after `ramp`. A step function, not a slope: the rate stays constant within a segment (ADR 0002).
- New account: starts at the global cap at opening; the first proof of residence does not restart the ramp (seniority = age of the account).
- Change of country: starting level = cap reached so far, starting time = day of the move. A target at or below the starting level applies at once.
- The starting level is stored as a rate, so a later change of the cap period does not distort it.

## Consequences
+ All numbers are parameters. + Moving back and forth cannot raise a cap faster than staying.
− A proof of residence without country change does not touch the ramp; an attestation without residence leaves the earlier residence in place.

Offene Frage: A2 target level · A3 final stage · A2b residence and localisation (dual residence, refugees, place of purchase).
