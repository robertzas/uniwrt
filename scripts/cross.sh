#!/usr/bin/env bash
# Builds static musl binaries with cargo-zigbuild inside its container, so the host needs only
# Docker. Usage: scripts/cross.sh <target>...; each lands in dist/uniwrt-<arch> for the Dockerfile.
set -euo pipefail
cd "$(dirname "$0")/.."

# Pinned by digest; bump deliberately and keep its zig version in line with CI (BUILD.md).
image=ghcr.io/rust-cross/cargo-zigbuild:0.23.4@sha256:d8313491ec5798de0633fdc1c5753761bff79967bea69076020dc78121b2cca8

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

# The toolchain from rust-toolchain.toml, the crate registry, zig's cache and the build persist in
# target/cross/, so repeat builds are incremental. The directories exist before `docker run`, so
# Docker doesn't create them as root.
cache=target/cross
mkdir -p "$cache"/{rustup,registry,home,build} dist

docker run --rm --user "$(id -u):$(id -g)" \
  --volume "$PWD:/src" --workdir /src \
  --volume "$PWD/$cache/rustup:/usr/local/rustup" \
  --volume "$PWD/$cache/registry:/usr/local/cargo/registry" \
  --env HOME="/src/$cache/home" --env XDG_CACHE_HOME="/src/$cache/home" \
  --env CARGO_TARGET_DIR="/src/$cache/build" \
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
