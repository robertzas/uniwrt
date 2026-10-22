# Status

**Milestone:** M0 — Foundation
**Next WP:** M0.2 — Rust workspace + server skeleton
**Last done:** M0.1 — Plan & directives (2026-10-21)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M0 (M0.2–M0.5), `docs/dev/BUILD.md` and `docs/dev/CONVENTIONS.md`.
2. Implement M0.2 → M0.5 in order. Checkpoint after each WP; M0.2 and M0.3 may share a session.
3. Goal of M0: every push to `main` builds, tests and releases a 4-arch scratch image plus binaries.

## Open questions for the owner

- Router models and current OpenWrt versions of the gateway and the 3 nodes, and the current mesh/backhaul setup (batman? 802.11s? wired?). These shape the M1.3 fixtures and the test matrix.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust 1.99.1 from the Arch package (no rustup). Cross builds run in the cargo-zigbuild container (`just cross`); CI uses rustup.
- bun 1.4.3 and node 26 are installed; there is no pnpm.
- Docker 29 without buildx. Install `docker-buildx` for local multi-arch images.
- `just` is not installed yet: `pacman -S just`.
- `gh` is authenticated as `robertzas` with `repo` and `workflow` scopes.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-21 — M0.1: plan, directives, feature specs, decisions, roadmap; repo created at github.com/robertzas/uniwrt. Owner chose SSH + ephemeral ucode agent, observe-first roadmap, home-server host, MIT OR Apache-2.0.
