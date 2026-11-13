# ADR 0002 – Lazy issuance

Status: accepted (M0, 2026-11-13); bookkeeping refined in M1

## Context
Money is issued per person and time, down to one cent per minute. Writing to every account every minute does not scale; letting devices create money is unsafe.

## Decision
- Nothing is issued until claimed. Per account: `anchor`, `rate`, `paid_since_anchor`, `paid_until`, `last_claim`, `carried`.
- Entitlement = floor(rate × (now − anchor)) − `paid_since_anchor`. Exact: two claims pay the same as one; no cent is created by rounding.
- Rate = step rate limited by the account's cap; the rate of the open segment is stored, so a claim needs only the parameter versions since `paid_until`. A change of step, cap or parameter version closes the segment at the moment of change and starts a new anchor (sub-cent remainder dropped, in favour of nobody).
- Moments of change are absolute (start of a version, expiry of a verification, steps of the seniority ramp counted from its start) – never the time of a claim. Hence claiming often and claiming once pay the same.
- A change made to the account itself (step, level, residence, freeze) first accounts everything accrued under the old conditions and sets it aside (`carried`); the next claim mints it. Verification level and its expiry are stored in the account state, so the past is never judged by a later attestation.
- A verification that expired counts as level 0 from the moment of expiry: the rate falls back to the highest step that level still reaches.
- Claim window (`claim_window`, reference: Circles, 14 days): entitlement older than the window lapses.
- Time comes from the host as an argument and may never go backwards per account.
- Parameters never change retroactively – a technical requirement of this scheme.
- Ripening (if ever switched on) = delayed mint, not a second balance: a claim at `now` covers the time up to `now` − ripening; money set aside at a change ripens one ripening period after the change (several amounts set aside share the latest date – simple and never early).
- Guard: a claim is split into at most `MAX_SEGMENTS` pieces; `Params::validate` keeps parameter sets far below.

## Consequences
+ One ledger operation per claim, none for idle accounts. + Device cannot mint. + Clock errors bounded by the window.
− People must claim (or the host claims for them on a schedule). − Total entitlement not yet claimed is not on the ledger; totals report it as an estimate.

## Sources of the idea
Duniter universal dividend (claim-based), Circles (retroactive cap). Ideas only, no code taken.
