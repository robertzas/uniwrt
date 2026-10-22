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
- **UI assets:** embedded and precompressed (brotli + gzip) at build time. Hashed filenames are served with `Cache-Control: immutable`. If `web/dist` is missing, a placeholder page is embedded, so Rust-only work never needs Node.
- **Subcommands:** `serve` (default), `healthcheck` (for Docker HEALTHCHECK, since scratch has no curl), `version`; later `reset-admin` and `backup`.
- **Release profile:** `lto = "fat"`, `codegen-units = 1`, `strip = true`. Keep `panic = "unwind"` so one task's panic can't take down the server. Compare `opt-level` 3 against `"s"` in M0.4.
- **Version:** CI injects `UNIWRT_VERSION` and `UNIWRT_COMMIT` at build time. Versions in Cargo.toml stay `0.0.0`, so nothing is committed back.

## Runtime contract

| Item | Value |
|---|---|
| User | 65532:65532 |
| Data | `/data` volume (`UNIWRT_DATA_DIR`) |
| Ports | 8443/tcp HTTPS UI and API (from M1.6) · 8080/tcp HTTP (serves the UI in M0, redirects later) · 5514/tcp+udp syslog |
| Networking | `--network host` is recommended (L2 discovery, simple syslog). Bridge mode works with published ports. |
| Health | `HEALTHCHECK CMD ["/uniwrt","healthcheck"]` |

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
