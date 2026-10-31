#!/usr/bin/env bash
# Tests scripts/new-feature.sh (`just test-new-feature`; CI's rust job runs it): scaffolds an
# area with telemetry in a throwaway worktree that mirrors the working tree, then holds the result
# to what CI gates on (clippy, the new tests, the web checks, arch-check), and checks that a
# second run refuses to write. Builds share target/, so a warm cache keeps it to about a minute.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
tmp=$(mktemp -d)
wt=$tmp/wt
cleanup() {
  git -C "$root" worktree remove --force "$wt" 2>/dev/null || true
  rm -rf "$tmp"
}
trap cleanup EXIT

# The worktree: HEAD plus the working tree's changes, untracked files included.
git worktree add --quiet --detach "$wt" HEAD
git diff HEAD --binary | git -C "$wt" apply --allow-empty
git ls-files --others --exclude-standard -z | xargs -0 -r cp --parents -t "$wt"
# Its own node_modules: a symlink to the main one would load two copies of Solid.
(cd "$wt/web" && bun install --frozen-lockfile >/dev/null)
export CARGO_TARGET_DIR=$root/target

cd "$wt"
failures=0
step() {
  local name=$1 log
  shift
  log=$tmp/${name// /-}.log
  if "$@" >"$log" 2>&1; then
    printf 'ok    %s\n' "$name"
  else
    printf 'FAIL  %s:\n' "$name"
    tail -n 60 "$log"
    failures=$((failures + 1))
  fi
}

step 'scaffold clients with telemetry' bash scripts/new-feature.sh clients --telemetry
step 'clippy' cargo clippy --workspace --all-targets --locked -- -D warnings
step 'the scaffolded tests' cargo nextest run --workspace --locked --status-level fail \
  -E 'test(/clients/) | test(/bundle/)'
step 'web checks' bash -c 'cd web && bun run check'
step 'arch-check' bash scripts/arch-check.sh
step 'TS types are generated' test -f web/src/api/gen/ClientsOverview.ts
step 'a second run refuses' bash -c "! bash scripts/new-feature.sh clients 2>/dev/null"
step 'a page missing from nav.ts is refused' \
  bash -c "bash scripts/new-feature.sh widgets 2>&1 | grep -q 'has no page /widgets'"

if ((failures)); then
  printf '%d failure(s)\n' "$failures"
  exit 1
fi
echo 'all new-feature tests passed'
