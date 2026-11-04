#!/usr/bin/env bash
# The uci crate's conformance fixtures (TESTING.md, Fixtures): runs `uci export` on every fixture
# in crates/uci/fixtures/{cases,openwrt/*} inside OpenWrt rootfs containers, requires the releases
# to agree, and writes what uci printed next to each fixture as <name>.out (its stdout when it
# succeeded, else its error). The crate's tests then hold the parser and serializer to it, and
# check that every fixture is listed in crates/uci/src/tests/conformance.rs.
# Usage: scripts/uci-conformance.sh [--check] [image…]   (--check compares instead of writing)
set -euo pipefail
cd "$(dirname "$0")/.."

check=false
if [[ ${1:-} == --check ]]; then
  check=true
  shift
fi
# The releases in the test matrix (TESTING.md); pinned, so a run is reproducible.
images=("$@")
if ((${#images[@]} == 0)); then
  images=(openwrt/rootfs:x86-64-24.10.8 openwrt/rootfs:x86-64-25.12.6)
fi

fixtures=crates/uci/fixtures
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Inside the container: /f holds the fixtures (read-only), /o receives the outputs.
# shellcheck disable=SC2016 # expanded by the container's shell
probe='
for dir in /f/cases /f/openwrt/*; do
  group=${dir#/f/}
  mkdir -p "/o/$group"
  for f in "$dir"/*; do
    case $f in *.out) continue ;; esac
    name=${f##*/}
    if uci -c "$dir" export "$name" >/o/.stdout 2>/o/.stderr; then
      mv /o/.stdout "/o/$group/$name.out"
    else
      mv /o/.stderr "/o/$group/$name.out"
    fi
  done
done
rm -f /o/.stdout /o/.stderr'

first=''
for image in "${images[@]}"; do
  out="$work/${image##*:}"
  mkdir -p "$out"
  docker run --rm --network none --user "$(id -u):$(id -g)" \
    -v "$PWD/$fixtures:/f:ro" -v "$out:/o" "$image" sh -c "$probe"
  if [[ -z $first ]]; then
    first=$out
  elif ! diff -r "$first" "$out"; then
    echo "✗ uci in ${images[0]} and $image disagree (above)" >&2
    exit 1
  fi
done

listed=crates/uci/src/tests/conformance.rs
status=0
while IFS= read -r expected; do
  rel=${expected#"$first"/}
  name=${rel%.out}
  if ! grep -q "\"$name\"" "$listed"; then
    echo "✗ $name is not listed in $listed" >&2
    status=1
  fi
  if $check; then
    if ! cmp -s "$expected" "$fixtures/$rel"; then
      echo "✗ $fixtures/$rel differs from what uci prints:" >&2
      diff "$fixtures/$rel" "$expected" >&2 || true
      status=1
    fi
  else
    cp "$expected" "$fixtures/$rel"
  fi
done < <(find "$first" -name '*.out' | sort)
exit $status
