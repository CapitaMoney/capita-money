# ADR 0012 – One rules file for every rule value

Status: accepted (2026-11-13). Owner: rules change often during the alpha ("learning by doing"); a change must be easy.

## Context
The core already takes every monetary number as a parameter (ADR 0001). But the numbers a host starts with were written three times in Rust – tests of the core, tests of the pallet, genesis of the chain – and one rule value (how long a verification holds) stood among the technical constants of the runtime. The owner keeps a rules diary; nothing tied the code to it. Meanwhile the diary had dropped the incentive ladder, country caps and the ramp (2026-11-13), and the chain still started with the old examples.

## Decision
- **`rules/alpha.toml` is the one place for rule values.** It mirrors the owner's rules diary (`diary` names the version). Every rule is a small table: `value`, `status` (`decided` · `default` · `example`), `source` (rule of the diary, question of the interview, ADR), `note`. Values are written as people write them: `"1.00"`, `"14 days"`, `"50 %"`, `"off"`.
- **A register for rules that are behaviour.** Not every rule is a number ("no way back from Capita to euro"). Those stand in the same file under `[[register]]`, with the files that hold them (`code`) or the part that will (`planned`). A change starts at the place named. A test checks that the files exist.
- **`capita-rulefile`** (std) reads and checks a file: form, units, the validation of the core, and what the core cannot know (a new account must be able to stand on the first step; a verification holds for some time). An unknown key is an error – a typing mistake never passes silently. It also writes a rule set as Rust source.
- **`capita-rules`** (`no_std`) is that source, compiled: its build turns every file of `rules/` into a module – `alpha::params()` for the parameters of the core, constants for values that are no parameter of the core (`MEMBERSHIP_PERIOD`, `TOP_LEVEL`, `VERIFICATION_PRICE_EUR_CENTS`). A file that is not valid stops the build with the name of the rule. Hosts depend on this crate and hold no rule value of their own.
- **Two files.** `alpha.toml` = the rules. `dev-fast.toml` = no rules: the same structure with rates that show an effect within minutes. The chain preset `development` starts with `dev-fast`, `local_testnet` with `alpha`. Constants of the runtime (membership period) come from `alpha`.
- **Tests do not read the rules.** The tests of the core and of the pallets keep fixed numbers of their own; the tests of the runtime start from `dev-fast`. So a change of `alpha.toml` breaks no test with an expected number. What must hold for any numbers is tested for every rule set without expecting a number (`crates/capita-rules/tests/sets.rs`): open, verify, move up, claim; never more than rate and cap allow.
- **Overview:** `cargo run -p capita-rulefile --example overview -- rules/alpha.toml` lists the rules by status, the open ones first.

## How a rule changes
1. A number: edit `rules/alpha.toml`, raise `params_version` if a running host is to enact it, build. A new chain starts with it; a running chain takes it by `capita.enactParams` (a new version, never retroactive – ADR 0002), a running service by reading the file.
2. A value that is a constant of the runtime (membership period): edit the file; a running chain needs a runtime upgrade.
3. A new kind of value: one field in `capita-rulefile` (model, overview, source), one line in each rules file, and the place that reads it.
4. Behaviour: the register names the files. If the behaviour should become a choice, it becomes a value of the file (step 3) and a branch in the code named.

## Consequences
+ One file answers "what are the rules, and which are still open". + The chain no longer starts with rules the owner dropped. + A service host (M3b, M4) reads the same file while it runs; nothing is copied.
− The build of `capita-rules` needs a TOML reader on the building machine (not in the runtime: the runtime sees constants only).
− Behaviour is registered, not yet switchable. Each switch costs a branch and tests for both sides; they are added when a decision needs them, not in advance.
− The diary stays with the owner, outside this repository: the file can lag behind it. `diary` shows which version it mirrors.

Offene Frage: with the ladder dropped, a verified person still has to choose the step by a call of their own (`register step-by-holder`). Simpler would be: verification moves the account to the highest step it reaches.
