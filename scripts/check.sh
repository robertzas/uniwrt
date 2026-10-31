#!/usr/bin/env bash
# Runs everything CI gates on, one line per step (`just check`). A failing step prints the start of
# its output; full logs stay in target/check/.
set -uo pipefail
cd "$(dirname "$0")/.." || exit

logs=target/check
mkdir -p "$logs"
failed=()

# step NAME DETAIL CMD...: runs CMD with its output captured. On success it prints the last match
# of the extended regex DETAIL in that output (such as a test count), if DETAIL is not empty.
step() {
  local name=$1 detail=$2 log=$logs/${1// /-}.log start=$SECONDS note lines
  shift 2
  if "$@" >"$log" 2>&1; then
    note=$([[ -n $detail ]] && grep -oE "$detail" "$log" | tail -n 1 | tr -s ' ')
    printf '✓ %s%s (%ss)\n' "$name" "${note:+: $note}" $((SECONDS - start))
    return 0
  fi
  lines=$(wc -l <"$log")
  printf '✗ %s (%ss)\n' "$name" $((SECONDS - start))
  head -n 150 "$log"
  if ((lines > 150)); then printf '… %s more lines in %s\n' $((lines - 150)) "$log"; fi
  failed+=("$name")
  return 1
}

skip() { printf -- '- %s (skipped: %s)\n' "$1" "$2"; }

# need STEP TOOL PROBE...: true if PROBE runs; otherwise STEP fails with an install hint.
need() {
  local name=$1 tool=$2
  shift 2
  if "$@" >/dev/null 2>&1; then return 0; fi
  printf '✗ %s (%s is missing: run "just setup")\n' "$name" "$tool"
  failed+=("$name")
  return 1
}

step rustfmt '' cargo fmt --all --check
if ! step clippy '' cargo clippy --workspace --all-targets --locked -- -D warnings; then
  skip 'rust tests' 'clippy failed'
elif need 'rust tests' cargo-nextest cargo nextest --version; then
  # Also refreshes web/src/api/gen (ts-rs), so it runs before the web step.
  step 'rust tests' '[0-9]+ passed, [0-9]+ skipped' \
    cargo nextest run --workspace --locked --status-level fail --final-status-level fail
fi
step 'code map' 'regenerated .*' scripts/gen-map.sh
step 'arch check' '[0-9]+ crates, [0-9]+ web imports' scripts/arch-check.sh
if need cargo-deny cargo-deny cargo deny --version; then
  step cargo-deny '' cargo deny check
fi
step scripts '' bash -c 'scripts/next-version.test.sh && scripts/arch-check.test.sh'
if need web bun bun --version; then
  # The last "N passed" is vitest's test count (after its file count).
  step web '[0-9]+ passed' \
    bash -c 'cd web && bun install --frozen-lockfile && bun run check'
fi
if command -v shellcheck >/dev/null; then
  step shellcheck '' shellcheck scripts/*.sh
else
  skip shellcheck 'not installed; CI runs it'
fi

if ((${#failed[@]})); then
  printf '\ncheck failed (%ss): %s\n' "$SECONDS" "$(printf '%s, ' "${failed[@]}" | sed 's/, $//')"
  exit 1
fi
printf '\ncheck passed (%ss)\n' "$SECONDS"
