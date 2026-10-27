# Status

**Milestone:** M1 — Core engine and read-only adoption
**Next WP:** M1.2 — SSH transport (`node` crate)
**Last done:** M1.1 — core, model (facts), store (2026-10-27)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M1 (M1.2), `docs/ARCHITECTURE.md` §Node protocol and §Failure modes, `docs/features/adoption.md` §Identity, credentials, trust, `docs/features/security.md` §Secrets vault, and `docs/dev/TESTING.md`. Read the `//!` contracts of `crates/core`, `crates/model` and `crates/store` (lib.rs), not their implementations.
2. russh must use the ring crypto provider: `deny.toml` bans aws-lc-sys, which recent russh enables by default. Check the features with `cargo info russh@<version>`, and run `just crate-age` after any `Cargo.lock` change (versions must be ≥ 7 days old, D-031).
3. Host-key pinning (TOFU) and the vault stub need a store migration `0002_*.sql` plus repository methods; follow the `NodeRepo`/`FactsRepo` pattern (trait in `repo/mod.rs`, SQL in `repo/<concern>.rs`, temp-DB tests).
4. The dropbear-in-OpenWrt-rootfs test is a new CI job (BUILD.md); keep unit tests on an in-process russh server.

## Open questions for the owner

- Router models and current OpenWrt versions of the gateway and the 3 nodes, and the current mesh/backhaul setup (batman? 802.11s? wired?). These shape the M1.3 fixtures and the test matrix.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust comes from rustup (D-026). cargo-nextest 0.9.149 and cargo-deny 0.20.2 are in `~/.cargo/bin` (`just setup`); cargo finds them there although it isn't on PATH.
- Installed: `just`, bun 1.4.4, node 26, Docker 29 with buildx 0.38 and the containerd image store, Google Chrome, python3. Not installed: shellcheck (run `koalaman/shellcheck:stable` in Docker) and qemu binfmt (run arm binaries with qemu-user in a `debian:trixie-slim` container).
- The agent's tool shell is zsh: quote globs, and keep multi-step job control in bash scripts.
- UI check without a browser window: `google-chrome-stable --headless=new --screenshot=<png> --window-size=1280,800 [--force-dark-mode] <url>`. With `--enable-logging=stderr`, CSP violations show up as `CONSOLE` lines.
- `gh` is authenticated as `robertzas` with `repo` and `workflow` scopes. The GHCR package `ghcr.io/robertzas/uniwrt` is public (anonymous pulls work).

## Measurements

- M0.3, web: initial JS 132.7 KB (44.9 KB gzip; budget 150 KB); CSS 5.3 KB gzip. Headless Chrome shows no CSP violations.
- M0.4: the container turns healthy in about 1 s and uses 1.5 MiB; health endpoint 7 µs CPU per request (D-028).
- M0.5, CI (cold caches): web 37 s, rust 58 s, build 1.5–2 min per target, image about 1 min; a push to `main` releases in about 5 min.
- M1.1, static musl binaries with UI and SQLite: amd64 4.63 MB, 386 4.51 MB, armv7 4.38 MB, arm64 4.33 MB (SQLite adds about 1.7 MB). Native idle RSS with the database open: 3.9 MB. Cross build of all four: 1 min 08 s. All four run `serve` (arm under qemu-user): migrated database, mode 0600, clean shutdown with the WAL folded in.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-27 — M1.1 done: crates `uniwrt-core` (NodeId, MacAddr, Timestamp, Clock, re-exported ipnet), `uniwrt-model` (NodeFacts with board/system/capabilities/interfaces/devices/radios/stations, the Node record, Wi-Fi vocabulary, a gateway fixture) and `uniwrt-store` (WAL, a batching writer thread with savepoints, a read pool, migration 0001, NodeRepo/FactsRepo); `serve` opens `<data-dir>/uniwrt.db`. D-031 (core types, store engine, the crate cooldown, Zlib allowed). `just crate-age`. Dependabot batch: rust-cache 2.9.3, solid-query 5.104.5, lucide-solid 1.60.0.
- 2026-10-27 — M0.5 done: the first main run failed the image smoke test (the runner's classic image store rejects a second platform pulled by the same index digest); each platform now runs by its own manifest digest. v0.1.0 released and verified. D-030: the agent batches Dependabot updates.
- 2026-10-22 — M0.5: ci.yml (version → web, rust → build ×4 → image → release), `scripts/next-version.sh` with tests, deny.toml, Dependabot with a 7-day cooldown (D-029), cargo-auditable binaries.
- 2026-10-22 — M0.4: justfile, `scripts/{check,cross,gen-map}.sh`, docs/MAP.md, the scratch Dockerfile; the local 4-arch image verified. D-028: opt-level 3 with musl's malloc.
- 2026-10-22 — M0.3: web skeleton (Vite 8, Solid 1.9, TypeScript 7, Tailwind 4 tokens, Biome, Vitest); D-027: npm 7-day minimum release age.
