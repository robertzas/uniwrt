# Status

**Milestone:** M0 — Foundation
**Next WP:** M0.3 — Web skeleton
**Last done:** M0.2 — Rust workspace + server skeleton (2026-10-22)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M0 (M0.3–M0.5), `docs/features/ui.md`, the "Embedded UI" section of `docs/dev/BUILD.md`, and the TypeScript section of `docs/dev/CONVENTIONS.md`.
2. Constraints that M0.2 sets for M0.3: Vite's hashed output stays under `assets/` (immutable caching and the SPA 404 rule depend on it); no inline scripts or styles (the CSP in security.md); the badge reads `GET /api/v1/health` → `{status, version, commit, built_at}`. Add ts-rs to the DTO in `crates/server/src/http/health.rs` (D-017).
3. Goal of M0: every push to `main` builds, tests and releases a 4-arch scratch image plus binaries.

## Open questions for the owner

- Router models and current OpenWrt versions of the gateway and the 3 nodes, and the current mesh/backhaul setup (batman? 802.11s? wired?). These shape the M1.3 fixtures and the test matrix.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.

## Environment notes (owner's machine)

- **Rust: switching to rustup (D-026).** The owner runs `sudo pacman -S rustup && rustup default stable`; `rust-toolchain.toml` then installs the pinned 1.99.1 with clippy and rustfmt. Until then the Arch package is active, and its default linker (`x86_64-linux-gnu-gcc`) doesn't exist: run cargo with `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=cc`. Check with `rustup --version`.
- Cross builds run in the cargo-zigbuild container (`just cross`); CI uses rustup with the pinned 1.99.1.
- `just`, bun 1.4.3, node 26 and Docker 29 are installed; there is no pnpm and no buildx (`pacman -S docker-buildx`). Not installed: cargo-nextest, cargo-deny, cargo-zigbuild (M0.4/M0.5 need them).
- `gh` is authenticated as `robertzas` with `repo` and `workflow` scopes.

## Measurements (native x86_64 gnu release, M0.2)

- Binary 4.4 MB with a 1.5 MB sample UI embedded; idle RSS 5 MB; `version` 1 ms; `healthcheck` 5 ms; release build 35 s (fat LTO).

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-22 — M0.2: workspace (toolchain pinned to 1.99.1, pedantic clippy, release profile); `uniwrt serve|healthcheck|version`; embedded UI precompressed at build time with ETags, negotiation and an SPA fallback; security headers; graceful shutdown; 34 tests. D-025: datasets live in their crate. D-026: the owner moves to rustup (Arch's rust package has a broken default linker).
- 2026-10-22 — Checkpoints made prompt-cache aware for Opus 5.5 (D-024): stop for a `/clear` only when it pays off, otherwise continue into the next WP (owner's choice).
- 2026-10-21 — M0.1: plan, directives, feature specs, decisions, roadmap; repo created at github.com/robertzas/uniwrt. Owner chose SSH + ephemeral ucode agent, observe-first roadmap, home-server host, MIT OR Apache-2.0.
