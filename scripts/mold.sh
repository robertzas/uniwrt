#!/usr/bin/env bash
# Installs the pinned mold linker (D-134), its release build checked against its SHA-256, into
# <dir>/mold with <dir>/ld.mold beside it (what `cc -fuse-ld=mold` runs), unless it is there
# already. Usage: scripts/mold.sh <dir>. CI, scripts/cross.sh and `just setup` use it.
set -euo pipefail

# The newest release; bump the version and both checksums together.
version=3.0.0
case $(uname -m) in
  x86_64) arch=x86_64 sha256=6c90d4a474c7c0409dfb575be03a5345878ac14fdba18de8b40fa58c60121189 ;;
  aarch64) arch=aarch64 sha256=52c759d3689babaea4c42af2f31062a74ab83e17ffb5a8090232534c67b70577 ;;
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
if [[ -x $dir/mold && -L $dir/ld.mold ]] && [[ $("$dir/mold" --version) == "mold $version "* ]]; then
  exit 0
fi

mkdir -p "$dir"
name=mold-$version-$arch-linux
tarball=$(mktemp)
trap 'rm -f "$tarball"' EXIT
curl -fsSL -o "$tarball" "https://github.com/rui314/mold/releases/download/v$version/$name.tar.gz"
echo "$sha256  $tarball" | sha256sum -c --quiet
tar -xzf "$tarball" -C "$dir" --strip-components 2 "$name/bin/mold"
ln -sf mold "$dir/ld.mold"
"$dir/mold" --version
