# ADR 0005 – Licence AGPL-3.0 and what it obliges us to do

Status: accepted (M0, 2026-11-13). Not legal advice.

## Decision
- Whole repository: GNU AGPL, version 3 (`LICENSE`). Source files carry a copyright line, the SPDX identifier `AGPL-3.0-only` and the standard notice.
- Offene Frage (owner decision): "version 3 only" or "version 3 or later". Chosen for now: only – it can be widened later, never narrowed.

## Duties
1. Keep every upstream copyright and licence notice.
2. Mark our changes to upstream files (date, what).
3. Publish the complete corresponding source of what we run (public repository).
4. Network use: every user of a running server or app gets a link to the exact source (AGPL section 13) – a visible "Source" link in app, website footer and API.
5. Copied code only with a compatible licence and a notice in the file; record it in `docs/upstream-mapping.md`.

## Practice
- Ideas (formulas, designs) may be reimplemented freely; code may not be copied without notice.
- Forks (Duniter, Ğecko, wallet) stay under their own licences in their own repositories.
- Telling upstream projects about our use: courtesy, not a duty.
