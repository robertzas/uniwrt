# Status

**Milestone:** M1 — Core engine and read-only adoption
**Next WP:** M1.1 — core, model (facts), store
**Last done:** M0.5 — CI/CD (2026-10-27)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M1 (M1.1), then `docs/ARCHITECTURE.md` §Crates and layering, §Persistence and §Performance budgets, `docs/dev/CONVENTIONS.md` §Architecture rules and §Rust, and `docs/dev/TESTING.md` §Fixtures.
2. M1.1 adds the first library crates (`core`, `model`, `store`) beside `crates/server`. Pure crates use no tokio and do no IO; give each a `//!` contract header in `lib.rs`, and run `just map` so `docs/MAP.md` stays fresh (CI checks it).
3. New dependencies must pass `cargo deny check` (`deny.toml`: no OpenSSL, native-tls or aws-lc). For SQLite, prefer a bundled build that cross-compiles under cargo-zigbuild for all four musl targets.

## Open questions for the owner

- Router models and current OpenWrt versions of the gateway and the 3 nodes, and the current mesh/backhaul setup (batman? 802.11s? wired?). These shape the M1.3 fixtures and the test matrix.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.
- Five Dependabot PRs (#1–#5) are open; how to handle them is to be settled with the owner.

## Environment notes (owner's machine)

- Rust comes from rustup (D-026). cargo-nextest 0.9.149 and cargo-deny 0.20.2 are in `~/.cargo/bin` (`just setup`); cargo finds them there although it isn't on PATH.
- Installed: `just`, bun 1.4.4, node 26, Docker 29 with buildx 0.38 and the containerd image store, Google Chrome. Not installed: shellcheck (run `koalaman/shellcheck:stable` in Docker) and qemu binfmt (run arm binaries with qemu-user in a `debian:trixie-slim` container).
- The agent's tool shell is zsh: quote globs, and keep multi-step job control in bash scripts.
- UI check without a browser window: `google-chrome-stable --headless=new --screenshot=<png> --window-size=1280,800 [--force-dark-mode] <url>`. With `--enable-logging=stderr`, CSP violations show up as `CONSOLE` lines.
- `gh` is authenticated as `robertzas` with `repo` and `workflow` scopes. The GHCR package `ghcr.io/robertzas/uniwrt` is public (anonymous pulls work).

## Measurements

- M0.2, native x86_64 release: binary 4.4 MB with a 1.5 MB sample UI; idle RSS 5 MB; `version` 1 ms; `healthcheck` 5 ms; release build 35 s.
- M0.3, web: initial JS 132.7 KB (44.9 KB gzip; budget 150 KB); CSS 5.3 KB gzip; fonts are woff2 subsets loaded on demand. Headless Chrome shows no CSP violations.
- M0.4, static musl binaries with the UI: amd64 2.87 MB, 386 2.77 MB, armv7 2.62 MB, arm64 2.63 MB; the 4-platform image is 6.45 MB in total. The container turns healthy in about 1 s and uses 1.5 MiB. Cross build of all four from scratch 1 min 29 s, relink 32 s. Health endpoint: 7 µs CPU per request on one core (D-028).
- M0.5, CI (cold caches): web 37 s, rust 58 s, build 1.5–2 min per target (parallel), image about 1 min; a push to `main` releases in about 5 min.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-27 — M0.5 done: the first main run failed the image smoke test (the runner's classic image store rejects a second platform pulled by the same index digest); each platform now runs by its own manifest digest and must report its target.
- 2026-10-22 — M0.5: ci.yml (version → web, rust → build ×4 → image → release), `scripts/next-version.sh` with tests, deny.toml, Dependabot with a 7-day cooldown (D-029), cargo-auditable binaries.
- 2026-10-22 — M0.4: justfile, `scripts/{check,cross,gen-map}.sh`, docs/MAP.md, the scratch Dockerfile; the local 4-arch image verified (amd64 and 386 natively, arm under qemu-user). D-028: opt-level 3 with musl's malloc.
- 2026-10-22 — M0.3: web skeleton (Vite 8, Solid 1.9, TypeScript 7, Tailwind 4 tokens with `light-dark()`, Biome, Vitest); a shell with sidebar, mobile drawer, theme toggle and a health badge typed by ts-rs; 8 web tests. D-027: npm 7-day minimum release age.
- 2026-10-22 — M0.2: Rust workspace, `uniwrt serve|healthcheck|version`, precompressed embedded UI, security headers, graceful shutdown. D-025: datasets live in their crate. D-026: rustup.
