# uniwrt tasks (D-019). `just` lists the recipes; docs/dev/BUILD.md explains the build.

set shell := ["bash", "-euo", "pipefail", "-c"]

# The release targets (BUILD.md, Artifacts). Docker platforms map to them in scripts/cross.sh.
release_targets := "x86_64-unknown-linux-musl i686-unknown-linux-musl armv7-unknown-linux-musleabihf aarch64-unknown-linux-musl"
platforms := "linux/amd64,linux/386,linux/arm/v7,linux/arm64"

# List the recipes
default:
    @just --list --unsorted

# Backend on 127.0.0.1 (HTTP 8080, HTTPS 8443) plus the Vite dev server, which proxies /api to
# the HTTP port as a trusted proxy (D-039); args go to serve
dev *args:
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
    UNIWRT_HTTP_LISTEN=127.0.0.1:8080 UNIWRT_HTTPS_LISTEN=127.0.0.1:8443 \
        UNIWRT_TRUSTED_PROXIES=127.0.0.1/32,::1/128 cargo run --quiet -- serve {{ args }} </dev/null &
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

# Rewrites the uci crate's expected outputs from `uci` in OpenWrt rootfs containers; --check compares (needs Docker)
uci-conformance *args:
    @scripts/uci-conformance.sh {{ args }}

# The node agent tests in OpenWrt rootfs containers booted with procd (needs Docker)
test-agent *images:
    @scripts/test-agent.sh {{ images }}

# Rendered configs on OpenWrt 24.10 and 25.12: an ISP, a gateway and an AP in rootfs containers (needs Docker)
test-system *releases:
    @scripts/test-system.sh {{ releases }}

# The Playwright smoke test: a debug binary with the UI embedded serves the simulated fleet to Chrome
e2e: web-build
    UNIWRT_WEB_DIST=web/dist cargo build --locked
    cd web && node_modules/.bin/playwright test

# The Wi-Fi and mesh tests in the hwsim lab (needs Docker; loads mac80211_hwsim and batman-adv)
test-wifi *args:
    @scripts/wifi-lab.sh test {{ args }}

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

# The performance budgets (D-149, scripts/perf/budgets.json) on web/dist and a binary; CI checks a
# release build's
perf bin="target/release/uniwrt":
    scripts/perf.sh js web/dist
    scripts/perf.sh binary {{ bin }}
    scripts/perf.sh cold-start {{ bin }}

# The runtime budgets (D-149) on a release binary: sized demo fleets, idle and under the API's
# reads (about 4 minutes; CI's `perf` job). The agent's memory is checked by `just test-agent`.
perf-runtime bin="target/release/uniwrt":
    scripts/perf.sh idle {{ bin }}
    scripts/perf.sh api {{ bin }}

# CPU flamegraphs (D-149, pprof in process): `bench [filter]` samples the criterion benches (ingest,
# render, logs); `serve [bin]` the controller under the 50-node demo's API reads (scripts/profile.sh)
profile what="bench" *args:
    scripts/profile.sh {{ what }} {{ args }}

# Static musl binaries built in the zigbuild container: dist/uniwrt-<arch> (default: all targets)
cross *targets=release_targets: web-build
    scripts/cross.sh {{ targets }}

# The cross binaries plus a local multi-arch image
image tag="uniwrt:dev": cross
    docker buildx build --platform {{ platforms }} --tag {{ tag }} --load .

# Scaffold a feature area (CONVENTIONS.md, Feature recipe), wired in and compiling: <area> [--path /page] [--telemetry]
new-feature area *args:
    @scripts/new-feature.sh {{ area }} {{ args }}

# The layering rules: crate dependencies per ARCHITECTURE.md, pure L0 crates, web import directions
arch-check:
    @scripts/arch-check.sh

# The scaffold's test: new-feature in a throwaway worktree, held to CI's checks (about a minute)
test-new-feature:
    @scripts/new-feature.test.sh

# Rebuild the embedded vendor table from IEEE's MA-L, MA-M and MA-S registries (D-053); an
# optional directory holds oui.csv, mam.csv and oui36.csv instead of downloading them
oui *dir:
    python3 scripts/oui.py {{dir}}

# Regenerate docs/MAP.md
map:
    @scripts/gen-map.sh

# `just dev` against the simulated fleet (a gateway, 3 access points, 30 clients); the seed is optional
demo seed="": (dev "--demo" (if seed == "" { "" } else { "--demo-seed " + seed }))

# Install the cargo tools (the versions CI pins) and the web dependencies
setup:
    cargo install --locked cargo-nextest@0.9.152 cargo-deny@0.21.0 cargo-insta@1.49.0
    scripts/mold.sh "$HOME/.local/bin"
    cd web && bun install --frozen-lockfile

[private]
web-build:
    cd web && bun install --frozen-lockfile && bun run build

# Refresh Sigstore's trust root (Fulcio's CAs, Rekor's keys) that verifies the controller's own
# releases (D-123); review the diff: a new key here is trusted for every later upgrade
sigstore-root:
    curl -fsSL https://raw.githubusercontent.com/sigstore/root-signing/main/targets/trusted_root.json -o crates/firmware/data/sigstore-trusted-root.json

# Rebuild the embedded time-zone table (IANA name → POSIX TZ) from the host's tzdata
tz *dir:
    python3 scripts/tz.py {{dir}}
