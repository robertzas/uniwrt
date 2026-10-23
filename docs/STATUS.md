# Status

**Milestone:** M0 — Foundation
**Next WP:** M0.4 — Build tooling & image
**Last done:** M0.3 — Web skeleton (2026-10-22)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M0 (M0.4–M0.5) and `docs/dev/BUILD.md` (Dockerfile shape, cross-compilation, local commands, web build).
2. Inputs for the justfile:
   - `just check`: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, the Rust tests (nextest), then `bun run check` in `web/`.
   - `just gen`: `cargo test export_bindings`.
   - `just dev`: `uniwrt serve` on 127.0.0.1:8080, plus `bun run dev`, which proxies `/api` to it.
   - `just build`: the web build, then the release binary with `UNIWRT_WEB_DIST=web/dist`, so a missing UI fails the build.
   - `scripts/gen-map.sh` reads the first `//!` line of each crate's `lib.rs` or `main.rs`.
3. Goal of M0: every push to `main` builds, tests and releases a 4-arch scratch image plus binaries.

## Open questions for the owner

- Router models and current OpenWrt versions of the gateway and the 3 nodes, and the current mesh/backhaul setup (batman? 802.11s? wired?). These shape the M1.3 fixtures and the test matrix.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust comes from rustup (D-026), so `rust-toolchain.toml` (1.99.1) applies locally, as in CI. Cross builds run in the cargo-zigbuild container (`just cross`).
- Installed: `just`, bun 1.4.4, node 26, Docker 29 and Google Chrome. Missing: buildx (`pacman -S docker-buildx`), cargo-nextest, cargo-deny and cargo-zigbuild (`cargo install --locked` works without sudo).
- UI check without a browser window: `google-chrome-stable --headless=new --screenshot=<png> --window-size=1280,800 [--force-dark-mode] <url>`. With `--enable-logging=stderr`, CSP violations show up as `CONSOLE` lines.
- `gh` is authenticated as `robertzas` with `repo` and `workflow` scopes.

## Measurements

- M0.2, native x86_64 release: binary 4.4 MB with a 1.5 MB sample UI; idle RSS 5 MB; `version` 1 ms; `healthcheck` 5 ms; release build 35 s.
- M0.3, web: initial JS 132.7 KB (44.9 KB gzip; budget 150 KB); CSS 5.3 KB gzip; fonts are woff2 subsets loaded on demand. Headless Chrome shows no CSP violations.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-22 — M0.3: web skeleton (Vite 8, Solid 1.9, TypeScript 7, Tailwind 4 tokens with `light-dark()`, Biome, Vitest); a shell with sidebar, mobile drawer, theme toggle and a health badge typed by ts-rs; 8 web tests. D-027: npm 7-day minimum release age.
- 2026-10-22 — M0.2: Rust workspace, `uniwrt serve|healthcheck|version`, precompressed embedded UI, security headers, graceful shutdown. D-025: datasets live in their crate. D-026: rustup.
- 2026-10-22 — Checkpoints made prompt-cache aware for Opus 5.5 (D-024): stop for a `/clear` only when it pays off, otherwise continue into the next WP (owner's choice).
- 2026-10-21 — M0.1: plan, directives, feature specs, decisions, roadmap; repo created at github.com/robertzas/uniwrt. Owner chose SSH + ephemeral ucode agent, observe-first roadmap, home-server host, MIT OR Apache-2.0.
