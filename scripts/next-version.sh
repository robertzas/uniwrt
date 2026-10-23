#!/usr/bin/env bash
# Prints the next release version from the Conventional Commits since the last vX.Y.Z tag (D-008),
# or nothing when HEAD is already released. CI runs it on pushes to main; it needs full history.
set -euo pipefail

if [[ $(git rev-parse --is-shallow-repository) == true ]]; then
  echo "next-version: needs the full history and tags (fetch-depth: 0)" >&2
  exit 1
fi

# The highest release tag reachable from HEAD; other tags (such as v2.0.0-rc.1) don't count.
last=$(git tag --merged HEAD --list 'v*' --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -n 1 || true)
if [[ -z $last ]]; then
  echo 0.1.0
  exit 0
fi
[[ $last =~ ^v([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]
major=${BASH_REMATCH[1]} minor=${BASH_REMATCH[2]} patch=${BASH_REMATCH[3]}

if [[ $(git rev-list --count "$last..HEAD") == 0 ]]; then
  exit 0
fi

subjects=$(git log --format=%s "$last..HEAD")
bodies=$(git log --format=%b "$last..HEAD")
if grep -qE '^[a-z]+(\([^)]*\))?!:' <<<"$subjects" || grep -qE '^BREAKING[ -]CHANGE:' <<<"$bodies"; then
  # Breaking changes bump the minor version while on 0.x.
  if ((major == 0)); then
    minor=$((minor + 1)) patch=0
  else
    major=$((major + 1)) minor=0 patch=0
  fi
elif grep -qE '^feat(\([^)]*\))?:' <<<"$subjects"; then
  minor=$((minor + 1)) patch=0
else
  patch=$((patch + 1))
fi
echo "$major.$minor.$patch"
