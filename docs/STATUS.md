# Status

**Milestone:** M0 — Foundation
**Next WP:** M0.5 — CI/CD
**Last done:** M0.4 — Build tooling & image (2026-10-22)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M0 (M0.5) and `docs/dev/BUILD.md` (CI/CD, Versioning, Cross-compilation, Runtime contract).
2. Reuse M0.4's pieces: `scripts/check.sh` (the gated steps), `scripts/gen-map.sh --check` (MAP freshness), `scripts/cross.sh` (target → `dist/uniwrt-<arch>` names), the root `Dockerfile` and `.dockerignore` (the context is `dist/uniwrt-*` only).
3. Inputs for ci.yml:
   - Build job: zig 0.16.0, the version in the local cargo-zigbuild 0.23.4 image, so CI and local builds match.
   - Rust job: after the tests, `git diff --exit-code web/src/api/gen` catches stale ts-rs types. Ubuntu runners ship shellcheck, so lint `scripts/*.sh` there.
   - Image job: download the four binaries into `dist/` under the Dockerfile's names (amd64, 386, armv7, arm64).

## Open questions for the owner

- Router models and current OpenWrt versions of the gateway and the 3 nodes, and the current mesh/backhaul setup (batman? 802.11s? wired?). These shape the M1.3 fixtures and the test matrix.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust comes from rustup (D-026). cargo-nextest 0.9.149 and cargo-deny 0.20.2 are in `~/.cargo/bin` (`just setup`); cargo finds them there although it isn't on PATH.
- Installed: `just`, bun 1.4.4, node 26, Docker 29 with buildx 0.38 and the containerd image store, Google Chrome. Not installed: shellcheck (run `koalaman/shellcheck:stable` in Docker) and qemu binfmt (run arm binaries with qemu-user in a `debian:trixie-slim` container).
- The agent's tool shell is zsh: quote globs, and keep multi-step job control in bash scripts.
- UI check without a browser window: `google-chrome-stable --headless=new --screenshot=<png> --window-size=1280,800 [--force-dark-mode] <url>`. With `--enable-logging=stderr`, CSP violations show up as `CONSOLE` lines.
- `gh` is authenticated as `robertzas` with `repo` and `workflow` scopes.

## Measurements

- M0.2, native x86_64 release: binary 4.4 MB with a 1.5 MB sample UI; idle RSS 5 MB; `version` 1 ms; `healthcheck` 5 ms; release build 35 s.
- M0.3, web: initial JS 132.7 KB (44.9 KB gzip; budget 150 KB); CSS 5.3 KB gzip; fonts are woff2 subsets loaded on demand. Headless Chrome shows no CSP violations.
- M0.4, static musl binaries with the UI: amd64 2.87 MB, 386 2.77 MB, armv7 2.62 MB, arm64 2.63 MB; the 4-platform image is 6.45 MB in total. The container turns healthy in about 1 s and uses 1.5 MiB. Cross build of all four from scratch 1 min 29 s, relink 32 s. Health endpoint: 7 µs CPU per request on one core (D-028).

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-22 — M0.4: justfile, `scripts/{check,cross,gen-map}.sh`, docs/MAP.md, the scratch Dockerfile; the local 4-arch image verified (amd64 and 386 natively, arm under qemu-user). D-028: opt-level 3 with musl's malloc.
- 2026-10-22 — M0.3: web skeleton (Vite 8, Solid 1.9, TypeScript 7, Tailwind 4 tokens with `light-dark()`, Biome, Vitest); a shell with sidebar, mobile drawer, theme toggle and a health badge typed by ts-rs; 8 web tests. D-027: npm 7-day minimum release age.
- 2026-10-22 — M0.2: Rust workspace, `uniwrt serve|healthcheck|version`, precompressed embedded UI, security headers, graceful shutdown. D-025: datasets live in their crate. D-026: rustup.
- 2026-10-21 — M0.1: plan, directives, feature specs, decisions, roadmap; repo created at github.com/robertzas/uniwrt. Owner chose SSH + ephemeral ucode agent, observe-first roadmap, home-server host, MIT OR Apache-2.0.
