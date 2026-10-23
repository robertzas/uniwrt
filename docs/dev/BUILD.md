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
- Dependencies are pinned to exact versions, and bun resolves only versions published at least 7 days ago, transitive ones included (`web/bunfig.toml`, D-027).
- `bun run dev` serves the UI with live reload and proxies `/api`, WebSockets included, to `UNIWRT_DEV_API` (default `http://127.0.0.1:8080`, where `uniwrt serve` listens).
- `cargo test` regenerates the TypeScript types of API DTOs in `web/src/api/gen/` (ts-rs; the export directory is set in `.cargo/config.toml`). Commit them together with the Rust change.
- Fonts are never inlined as `data:` URIs, because the CSP allows fonts only from `'self'`.

## Runtime contract

| Item | Value |
|---|---|
| User | 65532:65532 |
| Data | `/data` volume (`UNIWRT_DATA_DIR`), owned by 65532 with mode 0700. A new named volume inherits both; a bind-mounted host directory must be writable by UID 65532. |
| Ports | 8443/tcp HTTPS UI and API (from M1.6) · 8080/tcp HTTP (serves the UI in M0, redirects later) · 5514/tcp+udp syslog |
| Networking | `--network host` is recommended (L2 discovery, simple syslog). Bridge mode works with published ports. |
| Health | `HEALTHCHECK CMD ["/uniwrt","healthcheck"]` every 30 s, and every 1 s during the first 30 s, with a 5 s timeout: `GET /api/v1/health` on loopback must answer 200 within 3 s |
| Config | Flag or env: `--http-listen`/`UNIWRT_HTTP_LISTEN` (default `0.0.0.0:8080`), `--data-dir`/`UNIWRT_DATA_DIR` (default `./data`, created at start), `--log`/`UNIWRT_LOG` (tracing `EnvFilter` syntax, default `info`; logs go to stderr) |
| Shutdown | SIGTERM or SIGINT: stop accepting, let in-flight requests finish for up to 5 s, then exit 0. A second signal closes everything at once. |

The root `Dockerfile` builds the image from prebuilt binaries, so no stage runs under emulation:

- A `busybox` prep stage (pinned by digest, running on the build platform) picks `dist/uniwrt-${TARGETARCH}${TARGETVARIANT}` and creates `/data` with mode 0700.
- The final stage is `FROM scratch`: `/uniwrt` and `/data` (owned by 65532), `USER 65532:65532`, `ENV UNIWRT_DATA_DIR=/data`, `VOLUME /data`, the ports above, the HEALTHCHECK, OCI labels (`source` links the GHCR package to the repo), `ENTRYPOINT ["/uniwrt"]` and `CMD ["serve"]`.
- `.dockerignore` admits only `dist/uniwrt-*`, so the build context stays small.

## Cross-compilation

- **CI:** rustup targets + zig + cargo-zigbuild on ubuntu x86_64 runners: `cargo zigbuild --release --locked --target <target>`. Use the zig version of the local image (0.16.0), so both build the same way.
- **Local:** native builds need only rustup (D-026) and bun. Cross builds run as the host user in `ghcr.io/rust-cross/cargo-zigbuild` (`scripts/cross.sh` pins 0.23.4 by digest; it ships zig 0.16.0), so the host needs no zig and no musl targets. rustup installs the `rust-toolchain.toml` toolchain and the targets inside the container. The toolchain (about 1.3 GB with the four targets), the crate registry, zig's cache and the build output persist in `target/cross/`, so only the first run downloads; the image itself is about 6.8 GB because it bundles SDKs for other platforms.
- One cargo invocation builds all four targets in parallel (about 1.5 min from scratch with fat LTO) and writes `dist/uniwrt-{amd64,386,armv7,arm64}`, the names the Dockerfile expects. `just cross` and `just image` rebuild the web UI first, so cargo re-embeds it and relinks (about 30 s) even when nothing changed.
- zig's linker reports two harmless flags through rustc's `linker_messages` lint: `ignoring deprecated linker optimization setting '1'` and an unused `-nostartfiles`.
- Local multi-arch images need buildx and the containerd image store (`docker info` shows the `io.containerd.snapshotter.v1` driver type), so `--load` keeps all four platforms. Without binfmt, only the amd64 and 386 images run on an x86_64 host; arm binaries can be smoke-tested with qemu-user, as CI does.
- Image assembly copies the matching prebuilt binary for each platform, so builds never use QEMU emulation.

## CI/CD: `.github/workflows/ci.yml`

**Triggers:** push (all branches), pull_request, workflow_dispatch.
**Concurrency:** one group per ref. On `main`, runs queue instead of cancelling, so releases stay in order. GitHub keeps only the newest pending run, so several rapid pushes can coalesce into one release.

| Job | Runs for | What it does |
|---|---|---|
| version | all | On `main`: the next semver from commits (`scripts/next-version.sh`). Otherwise: `0.0.0-dev.<run>+<sha7>`. |
| web | all | `bun install --frozen-lockfile`, `biome ci`, `tsc --noEmit`, `vitest run`, `vite build` → artifact |
| rust | all | `cargo fmt --check`, `clippy -D warnings`, `cargo nextest run`, `cargo deny check`, MAP.md freshness check |
| agent (M1.3+) | all | node-agent tests inside OpenWrt rootfs containers (24.10, 25.12) |
| build ×4 | all | zigbuild with the embedded UI and version → smoke-test `uniwrt version` under qemu-user → artifact |
| image | pushes to `main` | buildx 4-platform image from the artifacts → GHCR; SBOM + provenance; cosign keyless signing |
| release | pushes to `main` | tag `vX.Y.Z`; GitHub Release with tarballs, SHA256SUMS and generated notes |

- **Permissions:** least privilege per job: `contents: write` only for release, `packages: write` for image, `id-token: write` and `attestations: write` for signing.
- **Secrets:** none beyond `GITHUB_TOKEN`.
- **Hygiene:** actions are pinned by SHA (Dependabot updates them). Caches: Swatinem/rust-cache per target, plus the bun cache.
- **One-time manual step:** after the first image push, make the GHCR package public if it didn't inherit the repo's visibility.

## Versioning (D-008)

- Every push to `main` is a release, tagged `vX.Y.Z`. The first release is v0.1.0, and versions stay at 0.x until 1.0 (M9.4).
- The bump is computed from the Conventional Commits since the last tag:
  - breaking (`!` or `BREAKING CHANGE`) → major, but only minor while on 0.x
  - `feat` → minor
  - anything else → patch
- Release notes are GitHub's generated notes.

## Local commands (justfile)

| Recipe | What it does |
|---|---|
| `just setup` | Install cargo-nextest and cargo-deny, plus the web dependencies |
| `just dev` | `uniwrt serve` on 127.0.0.1:8080 plus the Vite dev server, which proxies `/api` to it; Ctrl-C stops both |
| `just check` | Everything CI gates on, one line per step (`scripts/check.sh`): rustfmt, clippy, the Rust tests (nextest; they also refresh `web/src/api/gen`), the code map (regenerated when stale), the web check, and shellcheck when installed. A failing step prints the first 150 lines of its output; full logs stay in `target/check/`. |
| `just test` | Rust and web tests, with full output |
| `just fmt` · `just lint` | rustfmt and Biome, writing fixes · clippy and Biome |
| `just gen` | Regenerate TypeScript types from Rust |
| `just build` | Web build plus a native release binary (`target/release/uniwrt`) |
| `just cross [target…]` | Static musl binaries built in the zigbuild container, written to `dist/uniwrt-<arch>`; all four targets by default |
| `just image [tag]` | The four cross binaries plus a local multi-arch image (default tag `uniwrt:dev`) |
| `just map` | Regenerate `docs/MAP.md` (`scripts/gen-map.sh`; with `--check` it only reports staleness) |
| `just demo` | Run against the simulated fleet (a stub until M1.5) |
