#!/usr/bin/env bash
# Tests `scripts/compat.sh versions` against throwaway git repositories; `just check` and CI run it.
set -euo pipefail

script=$(cd "$(dirname "$0")" && pwd)/compat.sh
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
failures=0

repo() {
  rm -rf "$tmp/repo"
  git init -q "$tmp/repo"
  cd "$tmp/repo"
  git config user.name test
  git config user.email test@example.invalid
  git config commit.gpgsign false
  git config tag.gpgsign false
}
commit() { git commit -q --allow-empty -m "$1"; }
expect() {
  local got
  got=$("$script" versions | paste -sd ' ' -)
  if [[ $got == "$1" ]]; then
    printf 'ok    %s\n' "$2"
  else
    printf 'FAIL  %s: expected "%s", got "%s"\n' "$2" "$1" "$got"
    failures=$((failures + 1))
  fi
}

repo
commit 'chore: start'
commit 'feat: one'
expect '' 'no release yet: nothing to check'
git tag v0.1.0 HEAD~
expect 'v0.1.0' 'the previous release'
git tag v0.2.0
expect 'v0.1.0' "HEAD's own tag isn't older (a rerun of a release)"
commit 'fix: two'
git tag v0.10.0-rc.1
git tag other
expect 'v0.2.0' 'versions sort numerically; other tags and pre-releases are ignored'
git tag v0.10.0
commit 'fix: three'
expect 'v0.10.0' 'v0.10.0 is newer than v0.2.0'
git tag v1.0.0
commit 'fix: four'
expect 'v1.0.0' "1.0.0 is the previous release: listed once"
git tag v1.0.1
commit 'feat: five'
git tag v1.1.0
commit 'fix: six'
expect 'v1.1.0 v1.0.0' "from 1.0, also the major's first release"
git tag v2.0.0
commit 'fix: seven'
git tag v2.0.1
commit 'fix: eight'
expect 'v2.0.1 v2.0.0' "the current major's first release, not 1.0.0"
git checkout -q -b side v1.0.1
commit 'fix: side'
expect 'v1.0.1 v1.0.0' 'only tags reachable from HEAD'

if ((failures > 0)); then
  echo "$failures failed"
  exit 1
fi
