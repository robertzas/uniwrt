#!/usr/bin/env bash
# Prints the next release version from the Conventional Commits since the last vX.Y.Z tag (D-008),
# or nothing when HEAD is already released. CI runs it on pushes to main; it needs full history.
#
# A `Release-As: X.Y.Z` line in a commit body since the last tag asks for that version (the
# highest such line wins): it must be above the last tag and not below what the commits call for.
# That is how 1.0 is released, and the only way to 2.0 (D-152): from 1.0 on, a breaking change
# without `Release-As: <next major>.0.0` fails, so no 1.x breaks the compatibility promise.
set -euo pipefail

fail() {
  echo "next-version: $1" >&2
  exit 1
}

if [[ $(git rev-parse --is-shallow-repository) == true ]]; then
  fail "needs the full history and tags (fetch-depth: 0)"
fi

# Whether version $1 is above version $2.
above() {
  [[ $1 != "$2" && $(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -n 1) == "$1" ]]
}

# The highest release tag reachable from HEAD; other tags (such as v2.0.0-rc.1) don't count.
last=$(git tag --merged HEAD --list 'v*' --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -n 1 || true)
if [[ -n $last ]]; then
  [[ $(git rev-list --count "$last..HEAD") == 0 ]] && exit 0
  range="$last..HEAD"
else
  range=HEAD
fi

subjects=$(git log --format=%s "$range")
bodies=$(git log --format=%b "$range")
asked=$(sed -nE 's/^Release-As: *v?([0-9]+\.[0-9]+\.[0-9]+) *$/\1/p' <<<"$bodies" | sort -V | tail -n 1)
breaking=false
if grep -qE '^[a-z]+(\([^)]*\))?!:' <<<"$subjects" || grep -qE '^BREAKING[ -]CHANGE:' <<<"$bodies"; then
  breaking=true
fi

if [[ -z $last ]]; then
  echo "${asked:-0.1.0}"
  exit 0
fi
[[ $last =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]
major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]}

if $breaking; then
  if ((major == 0)); then
    # Breaking changes bump the minor version while on 0.x.
    minor=$((minor + 1)) patch=0
  else
    major=$((major + 1)) minor=0 patch=0
  fi
elif grep -qE '^feat(\([^)]*\))?:' <<<"$subjects"; then
  minor=$((minor + 1)) patch=0
else
  patch=$((patch + 1))
fi
next="$major.$minor.$patch"

if [[ -n $asked ]]; then
  above "$asked" "${last#v}" || fail "Release-As: $asked is not above $last"
  above "$next" "$asked" && fail "Release-As: $asked is below the $next the commits since $last call for"
  next=$asked
elif $breaking && [[ ${last#v} != 0.* ]]; then
  fail "a breaking change since $last: 1.x keeps its compatibility promise (D-152). Drop the \
breaking change, or release $next with a \`Release-As: $next\` line in a commit body"
fi
echo "$next"
