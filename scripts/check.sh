#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# The Rust checks of CI, run here before a push. Reads the one-line cargo
# steps straight from .github/workflows/ci.yml, so it never drifts from CI.
#   scripts/check.sh               every job
#   scripts/check.sh core issuer   only these jobs
#   scripts/check.sh --list        show the steps, run nothing
# Not run here: steps with their own env block (face models), the WebAssembly
# runtime and node builds (job "node"), the Android and app jobs. CI stays the
# judge; this catches formatting, lints and tests first.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
list=0
if [ "${1:-}" = "--list" ]; then list=1; shift; fi
want=" $* "
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-$(sed -n 's/^  RUSTUP_TOOLCHAIN: "\(.*\)"$/\1/p' .github/workflows/ci.yml)}"

# job<TAB>command for each "- run: ... cargo ..." line not followed by "env:"
steps=$(awk '
  /^  [a-z][a-z0-9-]*:$/ { job = substr($1, 1, length($1) - 1) }
  pending != "" { if ($1 != "env:") print pending; pending = "" }
  /^      - run: / && /cargo (fmt|clippy|test|check|build)/ {
    cmd = $0; sub(/^      - run: /, "", cmd); pending = job "\t" cmd
  }
  END { if (pending != "") print pending }
' .github/workflows/ci.yml)

installed=$(rustup target list --installed 2>/dev/null)
failed=() skipped=() passed=0
while IFS=$'\t' read -r job cmd; do
  [ -z "$job" ] && continue
  [ "$want" != "  " ] && [[ "$want" != *" $job "* ]] && continue
  [ "$job" = node ] && continue
  if [ "$list" = 1 ]; then printf '%-8s %s\n' "$job" "$cmd"; continue; fi
  target=$(grep -o -- '--target [^ ]*' <<<"$cmd" | cut -d' ' -f2)
  if [ -n "$target" ] && ! grep -qx "$target" <<<"$installed"; then
    skipped+=("$job: $cmd (rustup target add $target)"); continue
  fi
  printf '\n\033[1m[%s] %s\033[0m\n' "$job" "$cmd"
  if bash -c "$cmd"; then passed=$((passed + 1)); else failed+=("$job: $cmd"); fi
done <<<"$steps"
[ "$list" = 1 ] && exit 0

printf '\n%d passed, %d failed, %d skipped\n' "$passed" "${#failed[@]}" "${#skipped[@]}"
for s in "${skipped[@]}"; do echo "  skipped: $s"; done
for f in "${failed[@]}"; do echo "  FAILED:  $f"; done
[ "${#failed[@]}" = 0 ]
