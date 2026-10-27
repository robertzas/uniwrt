# uniwrt tasks (D-019). `just` lists the recipes; docs/dev/BUILD.md explains the build.

set shell := ["bash", "-euo", "pipefail", "-c"]

# The release targets (BUILD.md, Artifacts). Docker platforms map to them in scripts/cross.sh.
release_targets := "x86_64-unknown-linux-musl i686-unknown-linux-musl armv7-unknown-linux-musleabihf aarch64-unknown-linux-musl"
platforms := "linux/amd64,linux/386,linux/arm/v7,linux/arm64"

# List the recipes
default:
    @just --list --unsorted

# Backend on 127.0.0.1:8080 plus the Vite dev server, which proxies /api to it
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    # Job control gives each process its own group, so stopping one also stops its children.
    set -m
    # Stops both and waits for them. Ctrl-C exits 0; if one exits by itself, its status is kept.
    stop() {
        trap - EXIT INT TERM
        for pid in $(jobs -p); do kill -TERM -- "-$pid" 2>/dev/null || true; done
        wait
        exit "$1"
    }
    trap 'stop 0' INT TERM
    trap 'stop $?' EXIT
    UNIWRT_HTTP_LISTEN=127.0.0.1:8080 cargo run --quiet -- serve </dev/null &
    # Vite itself rather than `bun run dev`, whose exit-code report would add noise on Ctrl-C.
    (cd web && exec node_modules/.bin/vite) </dev/null &
    wait -n

# Everything CI gates on, one line per step
check:
    @scripts/check.sh

# Rust and web tests
test:
    cargo nextest run --workspace --locked
    cd web && bun run test

# The transport tests against dropbear in OpenWrt rootfs containers (needs Docker)
test-dropbear *images:
    @scripts/test-dropbear.sh {{ images }}

# Format Rust and web code in place
fmt:
    cargo fmt --all
    cd web && bun run fmt

# Lint Rust (clippy) and web (Biome) code
lint:
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cd web && bun run lint

# Regenerate the TypeScript types of API DTOs in web/src/api/gen
gen:
    cargo test --workspace --locked --quiet export_bindings

# Web build plus a native release binary with the UI embedded (target/release/uniwrt)
build: web-build
    UNIWRT_WEB_DIST=web/dist cargo build --release --locked

# Static musl binaries built in the zigbuild container: dist/uniwrt-<arch> (default: all targets)
cross *targets=release_targets: web-build
    scripts/cross.sh {{ targets }}

# The cross binaries plus a local multi-arch image
image tag="uniwrt:dev": cross
    docker buildx build --platform {{ platforms }} --tag {{ tag }} --load .

# Regenerate docs/MAP.md
map:
    @scripts/gen-map.sh

# Publish dates of the crates Cargo.lock adds against a revision; fails inside the 7-day cooldown
crate-age rev="HEAD":
    @scripts/crate-age.sh {{ rev }}

# Run against the simulated fleet (arrives with it in M1.5)
demo:
    @echo "just demo arrives with the simulated fleet in M1.5" >&2; exit 1

# Install the cargo tools (the versions CI pins) and the web dependencies
setup:
    cargo install --locked cargo-nextest@0.9.148 cargo-deny@0.20.2
    cd web && bun install --frozen-lockfile

[private]
web-build:
    cd web && bun install --frozen-lockfile && bun run build
