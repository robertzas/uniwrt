#!/usr/bin/env bash
# Installs the pinned wild linker (D-127), a static musl build checked against its SHA-256, into
# <dir>/wild unless it is there already. Usage: scripts/wild.sh <dir>. CI and scripts/cross.sh use
# it; locally `just setup` installs the same version with cargo.
set -euo pipefail

# Keep in line with `just setup`.
version=0.11.0
case $(uname -m) in
  x86_64) arch=x86_64 sha256=71abbcb7264a31571f3eeb1b7342b0dd6e0ddbcaf280bcfe7e427c0a2bead79f ;;
  aarch64) arch=aarch64 sha256=4b3696f6b030a03d4e903abe267c2c858b22d03df53998d96d4a166a074dede5 ;;
  *)
    echo "unsupported host: $(uname -m)" >&2
    exit 1
    ;;
esac

if (($# != 1)); then
  echo "usage: $0 <dir>" >&2
  exit 2
fi
dir=$1
if [[ -x $dir/wild ]] && [[ $("$dir/wild" --version) == *" $version "* ]]; then
  exit 0
fi

mkdir -p "$dir"
name=wild-linker-$version-$arch-unknown-linux-musl
tarball=$(mktemp)
trap 'rm -f "$tarball"' EXIT
curl -fsSL -o "$tarball" "https://github.com/wild-linker/wild/releases/download/$version/$name.tar.gz"
echo "$sha256  $tarball" | sha256sum -c --quiet
tar -xzf "$tarball" -C "$dir" --strip-components 1 "$name/wild"
"$dir/wild" --version
