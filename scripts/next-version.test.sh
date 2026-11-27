#!/usr/bin/env bash
# Tests scripts/next-version.sh against throwaway git repositories; `just check` and CI run it.
set -euo pipefail

script=$(cd "$(dirname "$0")" && pwd)/next-version.sh
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
fails() {
  if "$script" 2>/dev/null; then
    printf 'FAIL  %s: expected a failure\n' "$1"
    failures=$((failures + 1))
  else
    printf 'ok    %s\n' "$1"
  fi
}
expect() {
  local got
  got=$("$script")
  if [[ $got == "$1" ]]; then
    printf 'ok    %s\n' "$2"
  else
    printf 'FAIL  %s: expected "%s", got "%s"\n' "$2" "$1" "$got"
    failures=$((failures + 1))
  fi
}

repo
commit 'chore: start'
expect 0.1.0 'the first release is 0.1.0'
git tag v0.1.0
expect '' 'a released HEAD prints nothing'
commit 'docs: a'
expect 0.1.1 'docs bumps the patch version'
commit 'fix(web): b'
expect 0.1.1 'fix bumps the patch version'
commit 'feat(server): c'
expect 0.2.0 'feat bumps the minor version'
commit 'featured: not a feat'
expect 0.2.0 'only the feat type counts as a feature'
commit 'fix!: d'
expect 0.2.0 'a breaking change bumps the minor version on 0.x'
git tag v0.2.0
commit $'refactor: e\n\nBREAKING CHANGE: the API moved'
expect 0.3.0 'a BREAKING CHANGE footer counts as breaking'
git tag v1.4.9
commit 'fix: f'
expect 1.4.10 'the patch version has no digit limit'
commit 'feat(api)!: g'
fails 'a breaking change from 1.0 fails without Release-As'
commit $'chore: two\n\nRelease-As: 2.0.0'
expect 2.0.0 'Release-As of the next major releases a breaking change from 1.0'
git tag v2.0.0-rc.1
expect 2.0.0 'tags other than vX.Y.Z are ignored'

repo
commit 'chore: start'
git tag v0.40.2
commit 'feat: a'
commit $'chore(release): 1.0\n\nThe hardware run passed.\n\nRelease-As: 1.0.0'
expect 1.0.0 'Release-As releases 1.0 from 0.x'
git tag v1.0.0
commit 'fix!: b'
commit $'docs: c\n\nRelease-As: v2.0.0'
expect 2.0.0 'Release-As takes a leading v'
git tag v2.0.0
commit $'fix: d\n\nRelease-As: 2.0.0'
fails 'Release-As of the last version fails'
git tag v2.0.1
commit $'fix: e\n\nRelease-As: 1.9.0'
fails 'Release-As below the last version fails'
git tag v2.0.2
commit 'feat: f'
commit $'fix: g\n\nRelease-As: 2.0.3'
fails 'Release-As below what the commits call for fails'
commit $'fix: h\n\nRelease-As: 2.3.0'
expect 2.3.0 'the highest Release-As wins'
commit $'fix: i\n\n  Release-As: 9.0.0'
expect 2.3.0 'Release-As counts only at the start of a line'

repo
commit $'feat: first\n\nRelease-As: 0.5.0'
expect 0.5.0 'Release-As names the first release too'

repo
commit 'feat: shallow'
commit 'fix: clone'
git clone -q --depth 1 "file://$tmp/repo" "$tmp/shallow"
cd "$tmp/shallow"
if "$script" 2>/dev/null; then
  printf 'FAIL  a shallow clone is refused\n'
  failures=$((failures + 1))
else
  printf 'ok    a shallow clone is refused\n'
fi

if ((failures)); then
  echo "next-version: $failures failed" >&2
  exit 1
fi
