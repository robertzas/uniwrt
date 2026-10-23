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
- **Release profile:** `lto = "fat"`, `codegen-units = 1`, `strip = true`. Keep `panic = "unwind"` so one task's panic can't take down the server. Compare `opt-level` 3 against `"s"` in M0.4. The compressors used by the build script (`brotli`, `miniz_oxide`) are built at `opt-level = 3` in every profile; unoptimized, they would take minutes.
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

## Runtime contract

| Item | Value |
|---|---|
| User | 65532:65532 |
| Data | `/data` volume (`UNIWRT_DATA_DIR`) |
| Ports | 8443/tcp HTTPS UI and API (from M1.6) · 8080/tcp HTTP (serves the UI in M0, redirects later) · 5514/tcp+udp syslog |
| Networking | `--network host` is recommended (L2 discovery, simple syslog). Bridge mode works with published ports. |
| Health | `HEALTHCHECK CMD ["/uniwrt","healthcheck"]`: `GET /api/v1/health` on loopback must answer 200 within 3 s |
| Config | Flag or env: `--http-listen`/`UNIWRT_HTTP_LISTEN` (default `0.0.0.0:8080`), `--data-dir`/`UNIWRT_DATA_DIR` (default `./data`, created at start), `--log`/`UNIWRT_LOG` (tracing `EnvFilter` syntax, default `info`; logs go to stderr) |
| Shutdown | SIGTERM or SIGINT: stop accepting, let in-flight requests finish for up to 5 s, then exit 0. A second signal closes everything at once. |

Dockerfile shape (prebuilt binaries, no emulation):

```dockerfile
# syntax=docker/dockerfile:1
FROM --platform=$BUILDPLATFORM busybox:stable AS prep
ARG TARGETARCH
ARG TARGETVARIANT
COPY dist/ /dist/
RUN mkdir -p /out/data && cp /dist/uniwrt-${TARGETARCH}${TARGETVARIANT} /out/uniwrt && chmod 0755 /out/uniwrt

FROM scratch
COPY --from=prep /out/uniwrt /uniwrt
COPY --from=prep --chown=65532:65532 /out/data /data
USER 65532:65532
ENV UNIWRT_DATA_DIR=/data
VOLUME ["/data"]
EXPOSE 8080/tcp 8443/tcp 5514/tcp 5514/udp
HEALTHCHECK CMD ["/uniwrt", "healthcheck"]
ENTRYPOINT ["/uniwrt"]
CMD ["serve"]
```

## Cross-compilation

- **CI:** rustup targets + zig + cargo-zigbuild on ubuntu x86_64 runners: `cargo zigbuild --release --locked --target <target>`.
- **Local:** native builds need only cargo and bun. Cross builds run inside the cargo-zigbuild container image (`just cross <target>`; verify the image name in M0.4), so the host needs neither rustup nor zig. Local multi-arch images need the buildx plugin (`pacman -S docker-buildx`).
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
| `just dev` | Backend (`cargo run -- serve`) plus the Vite dev server with an API proxy |
| `just check` | Everything CI gates on, with summarized output |
| `just test` | Rust and web tests |
| `just gen` | Regenerate TypeScript types from Rust |
| `just build` | Web build plus a native release binary |
| `just cross <target>` | A static musl binary, built in the zigbuild container |
| `just image` | The 4 cross binaries plus a local multi-arch image |
| `just map` | Regenerate `docs/MAP.md` |
| `just demo` | Run against the simulated fleet (from M1.5) |
