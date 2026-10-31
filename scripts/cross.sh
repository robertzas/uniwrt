#!/usr/bin/env bash
# Builds static musl binaries with cargo-zigbuild inside its container, so the host needs only
# Docker. Usage: scripts/cross.sh <target>...; each lands in dist/uniwrt-<arch> for the Dockerfile.
set -euo pipefail
cd "$(dirname "$0")/.."

# Pinned by digest; bump deliberately and keep cargo-zigbuild in line with CI (BUILD.md).
image=ghcr.io/rust-cross/cargo-zigbuild:0.24.0@sha256:eb5578cfc06e070a09deb7b5852474bd128a4e561f9f123261b5ed2405e478b1
# CI's zig, which is newer than the image's (0.16.0): downloaded once, checked against
# ziglang.org's index, and run in place of the image's own.
zig_version=0.17.0
case $(uname -m) in
  x86_64) zig_arch=x86_64 zig_sha256=1cbe9df9f27e6b78d14ccbca43b6703a404ef79ef1c463de901d7f088d4e2026 ;;
  aarch64) zig_arch=aarch64 zig_sha256=9e8d11661d4ae3bd57702a3832781e23ad151dde5798e16a5ccd503f65234ff8 ;;
  *)
    echo "unsupported host: $(uname -m)" >&2
    exit 1
    ;;
esac

# Docker's TARGETARCH plus TARGETVARIANT for each release target.
arch() {
  case $1 in
    x86_64-unknown-linux-musl) echo amd64 ;;
    i686-unknown-linux-musl) echo 386 ;;
    armv7-unknown-linux-musleabihf) echo armv7 ;;
    aarch64-unknown-linux-musl) echo arm64 ;;
    *)
      echo "unsupported target: $1" >&2
      return 1
      ;;
  esac
}

if (($# == 0)); then
  echo "usage: $0 <target>..." >&2
  exit 2
fi
for target; do arch "$target" >/dev/null; done

# The toolchain from rust-toolchain.toml, the crate registry, zig, its cache and the build persist
# in target/cross/, so repeat builds are incremental. The directories exist before `docker run`,
# so Docker doesn't create them as root.
cache=target/cross
mkdir -p "$cache"/{rustup,registry,home,build} dist

zig=$cache/zig-$zig_version
if [[ ! -x $zig/zig ]]; then
  tarball=$cache/zig-$zig_version.tar.xz
  curl -fsSL -o "$tarball" "https://ziglang.org/download/$zig_version/zig-$zig_arch-linux-$zig_version.tar.xz"
  echo "$zig_sha256  $tarball" | sha256sum -c --quiet
  mkdir -p "$zig"
  tar -xJf "$tarball" -C "$zig" --strip-components 1
  rm "$tarball"
fi

docker run --rm --user "$(id -u):$(id -g)" \
  --volume "$PWD:/src" --workdir /src \
  --volume "$PWD/$cache/rustup:/usr/local/rustup" \
  --volume "$PWD/$cache/registry:/usr/local/cargo/registry" \
  --env HOME="/src/$cache/home" --env XDG_CACHE_HOME="/src/$cache/home" \
  --env CARGO_TARGET_DIR="/src/$cache/build" \
  --env CARGO_ZIGBUILD_ZIG_PATH="/src/$zig/zig" \
  --env UNIWRT_WEB_DIST=web/dist \
  --env UNIWRT_VERSION --env UNIWRT_COMMIT --env SOURCE_DATE_EPOCH \
  "$image" bash -euc '
    rustup --quiet toolchain install --no-self-update
    rustup --quiet target add "$@"
    targets=()
    for target; do targets+=(--target "$target"); done
    cargo zigbuild --release --locked "${targets[@]}"
  ' cross "$@"

for target; do
  bin=dist/uniwrt-$(arch "$target")
  install -m 0755 "$cache/build/$target/release/uniwrt" "$bin"
  printf '%s\t%s bytes\n' "$bin" "$(stat -c %s "$bin")"
done
