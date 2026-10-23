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
expect 2.0.0 'a breaking change bumps the major version from 1.0'
git tag v2.0.0-rc.1
expect 2.0.0 'tags other than vX.Y.Z are ignored'

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
