# Build, distribution & release

This is the source of truth for how uniwrt is built and shipped. It is implemented in M0.4–M0.5.

## Artifacts

| Artifact | Targets | Published to |
|---|---|---|
| Static binary `uniwrt` (musl, UI embedded) | x86_64-unknown-linux-musl · i686-unknown-linux-musl · armv7-unknown-linux-musleabihf · aarch64-unknown-linux-musl | GitHub Release: `uniwrt-<version>-<target>.tar.gz` + `SHA256SUMS` |
| Image `ghcr.io/robertzas/uniwrt` (`FROM scratch`) | linux/amd64 · linux/386 · linux/arm/v7 · linux/arm64 | GHCR tags `X.Y.Z`, `X.Y`, `latest`, `sha-<7 chars>` |

## A scratch-ready binary

- **Linking:** fully static (musl, with SQLite bundled). No shell or external tools; it writes only under the data dir.
- **TLS:** rustls with the `ring` provider, which cross-compiles cleanly to i686 and armv7. Mozilla CA roots are embedded (webpki-roots).
- **Time zones:** an embedded tzdb, because scratch has no `/usr/share/zoneinfo`.
- **SSH:** the client is in-process (russh); uploads stream over exec channels.
- **UI assets:** embedded and precompressed (brotli + gzip) at build time; see [Embedded UI](#embedded-ui). If `web/dist` is missing, a placeholder page is embedded, so Rust-only work never needs Node.
- **Subcommands:** `serve` (default), `healthcheck` (for Docker HEALTHCHECK, since scratch has no curl; it starts no async runtime), `version` (`--json` for scripts); later `reset-admin` and `backup`.
- **Release profile:** `lto = "fat"`, `codegen-units = 1`, `strip = true`. Keep `panic = "unwind"` so one task's panic can't take down the server. `opt-level = 3` (D-028): against `"s"` it adds 0.5 MB (2.87 vs 2.37 MB on x86_64) and saves 13–25 % CPU per request. The compressors used by the build script (`brotli`, `miniz_oxide`) are built at `opt-level = 3` in every profile; unoptimized, they would take minutes.
- **Allocator:** musl's built-in malloc (D-028). mimalloc saved only 3–5 % CPU per request at four cores and nearly tripled peak RSS (18.5 vs 6.8 MB); re-measure once real workloads exist.
- **Version:** CI injects `UNIWRT_VERSION` and `UNIWRT_COMMIT` at build time (see [Build inputs](#build-inputs)). Versions in Cargo.toml stay `0.0.0`, so nothing is committed back.
- **Toolchain:** `rust-toolchain.toml` pins an exact stable release with clippy and rustfmt, so a new Rust release can't break `main` through new lints. Bump it deliberately. A system Rust without rustup ignores the file.

## Build inputs

The build script (`crates/server/build/`) reads these environment variables. All are optional for local builds.

| Variable | Effect | Default |
|---|---|---|
| `UNIWRT_VERSION` | Reported by `version`, `--version` and `/api/v1/health` | `0.0.0-dev` |
| `UNIWRT_COMMIT` | The commit SHA, reported alongside the version | `unknown` |
| `SOURCE_DATE_EPOCH` | The build time (`built_at`), for reproducible builds | the time the build script last ran |
| `UNIWRT_WEB_DIST` | The web build to embed, relative to the workspace root. Without `index.html` the build fails, so CI sets it and a release can never ship the placeholder. | `web/dist` if it has `index.html`, else the placeholder |

Values must be at most 64 printable ASCII characters. While `web/dist` is missing, cargo reruns the build script on every build (about 0.4 s), so the first web build is always picked up.

## Embedded UI

- **Files:** everything in the web build except dotfiles (such as `.vite/`) and `.br`/`.gz` copies. Text-like files (HTML, JS, CSS, JSON, SVG, ICO, TTF/OTF) also get a brotli (quality 11) and a gzip (level 9) variant, each kept only if it saves at least 10%. Bytes are served from the binary without copying.
- **Negotiation:** `Accept-Encoding` q-values decide, with br > gzip > identity on ties; identity ranks last unless the client lists it. Compressible files carry `Vary: Accept-Encoding`. Browsers offer `br` only over HTTPS, so plain HTTP gets gzip.
- **Validation:** each variant has a strong `ETag` derived from the SHA-256 of the original (`"<16 hex>"`, `"…-br"`, `"…-gz"`), and a matching `If-None-Match` gets a 304.
- **Caching:** files under `assets/` are `public, max-age=31536000, immutable`, because Vite writes content-hashed names there (keep `build.assetsDir` at its default). Everything else, including `index.html`, is `no-cache` and revalidated by `ETag`.
- **SPA fallback** (like Vite's dev server, so dev and production route alike): a GET/HEAD miss that accepts HTML, or sends no `Accept`, gets `index.html`. Misses under `assets/` and `favicon.ico` stay 404, so a stale chunk never turns into HTML. Misses under `/api` are RFC 9457 problem+json 404s.

## Web build

- `web/` uses bun for installs and scripts. `bun run check` runs what CI gates on: `biome ci`, `tsc` (TypeScript 7) for the app and for the Vite config, `vitest run` and `vite build`.
- Dependencies are pinned to exact versions (`exact` in `web/bunfig.toml`, D-027) and updated to the newest release, with no cooldown (D-049).
- `bun run dev` serves the UI with live reload and proxies `/api`, WebSockets included, to `UNIWRT_DEV_API` (default `http://127.0.0.1:8080`, where `uniwrt serve` listens).
- `cargo test` regenerates the TypeScript types of API DTOs in `web/src/api/gen/` (ts-rs; the export directory is set in `.cargo/config.toml`). Commit them together with the Rust change.
- Fonts are never inlined as `data:` URIs, because the CSP allows fonts only from `'self'`.

## Runtime contract

| Item | Value |
|---|---|
| User | 65532:65532 |
| Data | `/data` volume (`UNIWRT_DATA_DIR`), owned by 65532 with mode 0700. A new named volume inherits both; a bind-mounted host directory must be writable by UID 65532. |
| Ports | 8443/tcp HTTPS UI and API · 8080/tcp HTTP: redirects to HTTPS, except the health check and trusted proxies (D-039) · 5514/tcp+udp syslog. The redirect keeps the host and uses the HTTPS listener's port, so publish 8443 as 8443. |
| Networking | `--network host` is recommended (L2 discovery, simple syslog). Bridge mode works with published ports. |
| Health | `HEALTHCHECK CMD ["/uniwrt","healthcheck"]` every 30 s, and every 1 s during the first 30 s, with a 5 s timeout: `GET /api/v1/health` on loopback must answer 200 within 3 s |
| Config | Flag or env: `--http-listen`/`UNIWRT_HTTP_LISTEN` (default `0.0.0.0:8080`), `--https-listen`/`UNIWRT_HTTPS_LISTEN` (default `0.0.0.0:8443`), `--data-dir`/`UNIWRT_DATA_DIR` (default `./data`, created at start), `--log`/`UNIWRT_LOG` (tracing `EnvFilter` syntax, default `info`; logs go to stderr), `--tls-names`/`UNIWRT_TLS_NAMES` (more certificate names, comma-separated), `--trusted-proxies`/`UNIWRT_TRUSTED_PROXIES` (networks, comma-separated), `--master-key-file`/`UNIWRT_MASTER_KEY_FILE` (default `<data dir>/master.key`) or the key itself in `UNIWRT_MASTER_KEY` (features/security.md) |
| Shutdown | SIGTERM or SIGINT: stop accepting, let in-flight requests finish for up to 5 s, then exit 0. A second signal closes everything at once. |

The root `Dockerfile` builds the image from prebuilt binaries, so no stage runs under emulation:

- A `busybox` prep stage (pinned by digest, running on the build platform) picks `dist/uniwrt-${TARGETARCH}${TARGETVARIANT}` and creates `/data` with mode 0700.
- The final stage is `FROM scratch`: `/uniwrt` and `/data` (owned by 65532), `USER 65532:65532`, `ENV UNIWRT_DATA_DIR=/data`, `VOLUME /data`, the ports above, the HEALTHCHECK, OCI labels (`source` links the GHCR package to the repo), `ENTRYPOINT ["/uniwrt"]` and `CMD ["serve"]`.
- `.dockerignore` admits only `dist/uniwrt-*`, so the build context stays small.

## Cross-compilation

- **CI:** rustup targets + zig + cargo-zigbuild on ubuntu x86_64 runners: `cargo zigbuild --release --locked --target <target>`. zig 0.17.0, the same as `scripts/cross.sh`, so both build the same way.
- **Local:** native builds need only rustup (D-026) and bun. Cross builds run as the host user in `ghcr.io/rust-cross/cargo-zigbuild` (`scripts/cross.sh` pins 0.24.0 by digest). The image ships zig 0.16.0, so the script downloads CI's zig 0.17.0 once, checks its SHA-256, and points cargo-zigbuild at it (`CARGO_ZIGBUILD_ZIG_PATH`); the host needs no zig and no musl targets. rustup installs the `rust-toolchain.toml` toolchain and the targets inside the container. The toolchain (about 1.3 GB with the four targets), the crate registry, zig, its cache and the build output persist in `target/cross/`, so only the first run downloads; the image itself is about 6.8 GB because it bundles SDKs for other platforms.
- One cargo invocation builds all four targets in parallel (about 1.5 min from scratch with fat LTO) and writes `dist/uniwrt-{amd64,386,armv7,arm64}`, the names the Dockerfile expects. `just cross` and `just image` rebuild the web UI first, so cargo re-embeds it and relinks (about 30 s) even when nothing changed.
- zig's linker reports two harmless flags through rustc's `linker_messages` lint: `ignoring deprecated linker optimization setting '1'` and an unused `-nostartfiles`.
- Local multi-arch images need buildx and the containerd image store (`docker info` shows the `io.containerd.snapshotter.v1` driver type), so `--load` keeps all four platforms. Without binfmt, only the amd64 and 386 images run on an x86_64 host; arm binaries can be smoke-tested with qemu-user, as CI does.
- Image assembly copies the matching prebuilt binary for each platform, so builds never use QEMU emulation.

## CI/CD: `.github/workflows/ci.yml`

**Triggers:** pushes to any branch except Dependabot's (those get CI through their pull requests), pull requests, and workflow_dispatch.
**Concurrency:** one group per ref. On `main`, runs queue instead of cancelling, so releases stay in order. GitHub keeps only the newest pending run, so several rapid pushes can coalesce into one release.

| Job | Runs for | What it does |
|---|---|---|
| version | all | On a push to `main`: the next version from the commits (`scripts/next-version.sh`), or no release if HEAD is already tagged. Otherwise: `0.0.0-dev.<run>+<sha7>`. |
| web | all | `bun install --frozen-lockfile`, `biome ci` (with annotations), `tsc`, `vitest run`. The image waits for it. |
| ui | all | `bun install --frozen-lockfile`, `vite build` → artifact `web-dist`; apart from web's checks, so the builds start sooner |
| rust | all | `cargo fmt --check`, MAP.md freshness, shellcheck and the script tests, `clippy -D warnings`, `cargo nextest run`, committed ts-rs types, `cargo deny check` |
| transport | all | `scripts/test-dropbear.sh`: the `node` crate against dropbear in OpenWrt rootfs containers (24.10.8, 25.12.6): adoption with a password, key login, quoting through busybox ash, stdin, uploads, a jump host, keepalives and a ucode agent. The image waits for it. |
| agent | all | `scripts/test-agent.sh`: the node agent in OpenWrt rootfs containers (24.10.8, 25.12.6) booted with procd: the bundle's install by hash, the ops in both loop modes, collect sets parsed into facts, the engine watching the node, and adoption. The image waits for it. |
| wifi | all | `scripts/wifi-lab.sh test`: the hwsim lab (mac80211_hwsim and batman-adv from the runner's kernel, installing `linux-modules-extra`, `wireless-regdb` and `iw` if missing; D-043) with the Wi-Fi and mesh tests. The image waits for it. |
| build ×4 | all (after ui) | `cargo auditable zigbuild` with the embedded UI, version and commit → smoke test: the embedded dependency list, `version --json`, and `serve` answering `healthcheck` (arm under qemu-user) → the binary and a reproducible tarball as artifacts |
| e2e | all (after build) | `web/e2e` with Playwright in the runner's Chrome against the x86_64 release binary serving `--demo --demo-seed 42`; traces uploaded on failure. The image waits for it. |
| image | pushes to `main` | buildx 4-platform image from the binaries → GHCR (`X.Y.Z`, `X.Y`, `latest`, `sha-<7>`) with SBOM and provenance attestations; cosign keyless signature; then every platform runs `version --json` from the registry, pulled by its own manifest digest (the runner's classic image store can't hold two platforms of one index digest), and must report its target and version |
| release | pushes to `main` | SHA256SUMS and build provenance attestations for the tarballs; GitHub Release `vX.Y.Z` (creating the tag) with generated notes |

- **Critical path** (about 8 minutes on `main`): ui → build ×4 (the fat-LTO release compile, D-028) → e2e (about a minute) → image → release. The other test jobs run beside the builds and finish before them, so they add coverage, not time; splitting them further would only add cache downloads.
- **Permissions:** none at the workflow level and read-only per job, except `packages: write` and `id-token: write` (cosign) for image, and `contents: write`, `id-token: write` and `attestations: write` for release.
- **Secrets:** none beyond `GITHUB_TOKEN`.
- **Hygiene:** actions are pinned by SHA to their newest releases (D-049), and Dependabot proposes updates, which the agent folds into one commit per checkpoint (D-030). Tools are pinned as well: bun 1.4.4 and zig 0.17.0 in the workflow's `env`; cargo-nextest 0.9.150, cargo-deny 0.20.2, cargo-zigbuild 0.24.0 and cargo-auditable 0.7.7 through taiki-e/install-action, which verifies checksums. Keep them in line with `just setup` and `scripts/cross.sh`. `run` steps use bash with `pipefail`. Caches: Swatinem/rust-cache per job and target (agent, transport and wifi share agent's, `containers`), saved from `main` only, since the account's cache storage (10 GB) and download volume are limited: a refused restore ("Egress is over the account limit") rebuilds everything, about 13 minutes per build; plus bun's install cache.
- **SBOM:** release binaries embed their dependency list (cargo-auditable, D-029), so the image SBOM lists every crate and `cargo audit bin` can scan a binary. The web UI's npm packages are not listed yet.
- **Verifying a release:** for the image (cosign v3 or later; v2 can't find the v3 signature format and reports "no signatures found"), `cosign verify ghcr.io/robertzas/uniwrt:<version> --certificate-identity https://github.com/robertzas/uniwrt/.github/workflows/ci.yml@refs/heads/main --certificate-oidc-issuer https://token.actions.githubusercontent.com`; for a tarball, `gh attestation verify <file> --repo robertzas/uniwrt` and `sha256sum -c SHA256SUMS`.
- **One-time manual step:** after the first image push, make the GHCR package public if it didn't inherit the repo's visibility.

## Versioning (D-008)

- Every push to `main` is a release, tagged `vX.Y.Z`. The first release is v0.1.0, and versions stay at 0.x until 1.0 (M9.4).
- The bump is computed from the Conventional Commits since the last tag:
  - breaking (`!` or `BREAKING CHANGE`) → major, but only minor while on 0.x
  - `feat` → minor
  - anything else → patch
- Release notes are GitHub's generated notes.
- `scripts/next-version.sh` starts from the highest `vX.Y.Z` tag reachable from HEAD (other tags don't count), and refuses a shallow clone. `scripts/next-version.test.sh` covers the rules.

## Local commands (justfile)

| Recipe | What it does |
|---|---|
| `just setup` | Install cargo-nextest and cargo-deny at the versions CI pins, plus the web dependencies |
| `just dev` | `uniwrt serve` on 127.0.0.1 (HTTP 8080, HTTPS 8443) plus the Vite dev server, which proxies `/api` to the HTTP port; loopback is a trusted proxy, so it gets the app rather than a redirect. Ctrl-C stops both |
| `just check` | Everything CI gates on, one line per step (`scripts/check.sh`): rustfmt, clippy, the Rust tests (nextest; they also refresh `web/src/api/gen`), the code map (regenerated when stale), cargo-deny, the script tests, the web check, and shellcheck when installed. A failing step prints the first 150 lines of its output; full logs stay in `target/check/`. |
| `just test` | Rust and web tests, with full output |
| `just test-dropbear [image…]` | The transport tests against dropbear in OpenWrt rootfs containers (needs Docker); the CI job `transport` runs the same script |
| `just test-agent [image…]` | The agent tests in OpenWrt rootfs containers booted with procd (needs Docker); the CI job `agent` runs the same script |
| `just test-wifi [nextest args…]` | The Wi-Fi and mesh tests in the hwsim lab (`scripts/wifi-lab.sh test`; needs Docker and a kernel with mac80211_hwsim and batman-adv, which it loads through a privileged container). It starts the lab unless one runs, and stops it after; local only (D-035) |
| `just e2e` | The Playwright smoke test (`web/e2e`): the web build, a debug binary with the UI embedded, then Playwright, which starts `serve --demo` on ports 18080/18443 in a temporary data directory and drives the installed Chrome |
| `just fmt` · `just lint` | rustfmt and Biome, writing fixes · clippy and Biome |
| `just gen` | Regenerate TypeScript types from Rust |
| `just build` | Web build plus a native release binary (`target/release/uniwrt`) |
| `just cross [target…]` | Static musl binaries built in the zigbuild container, written to `dist/uniwrt-<arch>`; all four targets by default |
| `just image [tag]` | The four cross binaries plus a local multi-arch image (default tag `uniwrt:dev`) |
| `just map` | Regenerate `docs/MAP.md` (`scripts/gen-map.sh`; with `--check` it only reports staleness) |
| `just demo [seed]` | `just dev` against the simulated fleet (`serve --demo`, D-038; log in as `demo` / `demo`); `just dev <args>` passes any arguments to `serve` |
