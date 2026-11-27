#!/usr/bin/env bash
# The compatibility job (D-152; docs/features/lifecycle.md, Compatibility): older releases' binaries write
# a data directory, a controller backup and a site file through their API (scripts/compat/
# write.sh, from each release's own tag once it has one), and this build's binary must upgrade,
# restore and import them (scripts/compat/check.sh). CI's `compat` job and `just compat` run it.
#
#   scripts/compat.sh versions          the releases to check against, one tag per line
#   scripts/compat.sh run BIN [TAG...]  checks x86_64 binary BIN against them (default: versions)
#
# The releases: the newest `vX.Y.Z` tag before HEAD and, from 1.0, the first release of its major
# (`vX.0.0`): every X.y.z upgrades in place from both. The older binaries come from the GitHub
# releases (`gh release download`, checked against their SHA256SUMS). COMPAT_OUT keeps each
# release's output and logs (default: a temporary directory, removed unless a check fails).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)

versions() {
  local previous major
  previous=$(git tag --merged HEAD~ --list 'v*' 2>/dev/null | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' |
    sort -V | tail -n 1) || true
  [[ -n $previous ]] || return 0
  echo "$previous"
  major=${previous#v}
  major=${major%%.*}
  if ((major >= 1)) && [[ $previous != "v$major.0.0" ]] && git rev-parse -q --verify "refs/tags/v$major.0.0" >/dev/null; then
    echo "v$major.0.0"
  fi
}

# `check BIN TAG OUT`
check() {
  local bin=$1 tag=$2 out=$3 version=${2#v} target=x86_64-unknown-linux-musl scripts
  mkdir -p "$out/release"
  echo "== $tag"
  gh release download "$tag" --dir "$out/release" --clobber \
    --pattern "uniwrt-$version-$target.tar.gz" --pattern SHA256SUMS
  (cd "$out/release" && sha256sum --quiet --check --ignore-missing SHA256SUMS)
  tar -xzf "$out/release/uniwrt-$version-$target.tar.gz" -C "$out/release"
  if git cat-file -e "$tag:scripts/compat/write.sh" 2>/dev/null; then
    mkdir -p "$out/scripts"
    git archive "$tag" scripts/compat | tar -x -C "$out/scripts"
    scripts=$out/scripts/scripts/compat
  else
    echo "$tag has no scripts/compat/write.sh: writing with this one (its API must still match)"
    scripts=$here/compat
  fi
  bash "$scripts/write.sh" "$out/release/uniwrt-$version-$target/uniwrt" "$out/data" "$out/written"
  bash "$here/compat/check.sh" "$bin" "$out/data" "$out/written"
}

case ${1-} in
versions) versions ;;
run)
  (($# >= 2)) || { echo "usage: compat.sh run BIN [TAG...]" >&2; exit 2; }
  bin=$(realpath "$2")
  shift 2
  tags=("$@")
  if ((${#tags[@]} == 0)); then mapfile -t tags < <(versions); fi
  if ((${#tags[@]} == 0)); then
    echo "no older release to check against"
    exit 0
  fi
  out=${COMPAT_OUT:-$(mktemp -d)}
  keep=${COMPAT_OUT:+yes}
  # Not `if ! check`: errexit doesn't apply inside a function called as a condition.
  current=""
  trap '[[ -z $current ]] || echo "compat: $current failed; its output and logs are in $out/$current" >&2' EXIT
  for tag in "${tags[@]}"; do
    current=$tag
    check "$bin" "$tag" "$out/$tag"
  done
  current=""
  [[ -n $keep ]] || rm -rf "$out"
  ;;
*)
  echo "usage: compat.sh versions | run BIN [TAG...]" >&2
  exit 2
  ;;
esac
