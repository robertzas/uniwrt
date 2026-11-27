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
- **Subcommands:** `serve` (default), `healthcheck` (for Docker HEALTHCHECK, since scratch has no curl; it starts no async runtime), `version` (`--json` for scripts), `restore FILE` (stages a controller backup for the next start, D-129), `reset-admin [USERNAME]` (a forgotten admin password replaced by a random one, beside a running controller; D-153).
- **Release profile:** `lto = "fat"`, `codegen-units = 1`, `strip = true`. Keep `panic = "unwind"` so one task's panic can't take down the server. `opt-level = 3` for the hot crates (D-028), `"s"` for the binary crate (which also sets fat LTO's pipeline) and the cold crates listed in `Cargo.toml` (D-149: 45.0 → 39.1 MB with ICF, API CPU within noise; `"s"` everywhere would be 29.8 MB at 5–10 % more CPU per request). The binary's override repeats `strip = true`: cargo drops the profile's `strip` for an overridden root package. The four musl targets link with mold's `--icf=safe` (identical functions whose address nothing takes are folded: 1.5 MB). Two more profiles (D-149): `bench` keeps symbols (line tables) for the flamegraphs and links with thin LTO at 16 codegen units, as a fat-LTO link of each bench binary takes minutes; `profiling` is `release` with its symbols kept, for `just profile serve`. pprof and inferno (CDDL-1.0, deny.toml's one licence exception) are in neither release build: the benches and the server's `profiling` feature only. Release builds (`just build`, `just cross`, CI's `build`) build the binary's package alone (`-p uniwrt`), so the other members' dependencies (the benches' among them) neither build nor unify features into it. The compressors used by the build script (`brotli`, `miniz_oxide`) are built at `opt-level = 3` in every profile; unoptimized, they would take minutes.
- **Allocator:** mimalloc v3 (D-128; `default-features = false`, no debug modes), set in `crates/server/src/main.rs`. On the demo's workload it costs about a fifth less CPU per request than musl's malloc, for about twice the memory (4 cores: 63 vs 31 MB idle, 111 vs 71 MB peak); jemalloc saved as much CPU but took more memory still. No transparent huge pages (D-149 M9.2d): mimalloc madvised its arenas for them, which doubled the heap, so it is built with `no_thp` (no madvise) and `main` turns them off for the process (`PR_SET_THP_DISABLE`); on a host whose THP is `always`, the few MB faulted before `main` stay huge (4 MB locally). Its purge delay stays at the default 1 s (purging at once saves 7 MB idle for ~10 % more CPU per API read).
- **Async workers:** one per core, at most 4 (`serve.rs`, D-149 M9.2d): each worker keeps allocator pages of its own. `TOKIO_WORKER_THREADS` overrides the cap.
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
- **The user guide (D-151):** `web/guide/plugin.ts`, a Vite plugin, renders `docs/guide/*.md` with marked at build time (`web/guide/render.ts`; the UI ships HTML, no Markdown parser): GitHub's heading anchors as `data-anchor`, links between chapters rewritten to `/help/…`, raw HTML escaped, images as links to the Pages copy (`web/guide/site.ts` renders that copy with the same renderer: `id`s, `.html` links, the screenshots shown, a missing or unused screenshot failing the build). A link to a missing chapter or section, or one that is neither a chapter nor a web address, fails the build and the tests. The dev server reloads on a changed chapter.
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
| Config | Flag or env: `--http-listen`/`UNIWRT_HTTP_LISTEN` (default `0.0.0.0:8080`), `--https-listen`/`UNIWRT_HTTPS_LISTEN` (default `0.0.0.0:8443`), `--data-dir`/`UNIWRT_DATA_DIR` (default `./data`, created at start), `--log`/`UNIWRT_LOG` (tracing `EnvFilter` syntax, default `info`; logs go to stderr), `--tls-names`/`UNIWRT_TLS_NAMES` (more certificate names, comma-separated), `--trusted-proxies`/`UNIWRT_TRUSTED_PROXIES` (networks, comma-separated), `--master-key-file`/`UNIWRT_MASTER_KEY_FILE` (default `<data dir>/master.key`) or the key itself in `UNIWRT_MASTER_KEY` (features/security.md), `--syslog-listen`/`UNIWRT_SYSLOG_LISTEN` (the syslog receiver over UDP and TCP, default `0.0.0.0:5514`, `off` to disable; off in demo mode unless given), `--iperf3-listen`/`UNIWRT_IPERF3_LISTEN` (the controller's iperf3 server, bound only while a node ↔ controller throughput test runs, default `[::]:5201`, `off` to disable, port 0 for a free port per test; D-142), `--asu-url`/`UNIWRT_ASU_URL` (the Attended Sysupgrade service that builds zero-touch images, default `https://sysupgrade.openwrt.org`, `off` to disable; D-104), `--self-upgrade`/`UNIWRT_SELF_UPGRADE` (`on` by default, `off` when a package manager owns the binary; D-123) |
| Processes | In a release build, `serve` is a small launcher that runs the controller as its child (D-121, D-123): the newest verified binary staged in `<data>/bin` while it is newer than the image's own, else its own binary with the same arguments (`UNIWRT_SUPERVISED=1` in the child's environment). It forwards SIGTERM and SIGINT and exits with the child's code. A staged binary's first start is a trial: the database is copied to `bin/rollback/` first, and unless the controller answers `/api/v1/health` within 3 minutes and still 10 s later, the trial is stopped, the database restored, the binary rejected and the previous binary started. The data directory must allow executing files (a `noexec` bind mount keeps the image's binary: the staged one fails to start and is rejected). Development builds, demo mode and `--self-upgrade off` serve directly in one process. An install (M7.1b2) stops the controller with exit code 75, which makes the launcher start it again; a stop signal arriving meanwhile wins (the launcher exits 0). |
| Shutdown | SIGTERM or SIGINT: stop accepting, let in-flight requests finish for up to 5 s, then exit 0. A second signal closes everything at once. |

The root `Dockerfile` builds the image from prebuilt binaries, so no stage runs under emulation:

- A `busybox` prep stage (pinned by digest, running on the build platform) picks `dist/uniwrt-${TARGETARCH}${TARGETVARIANT}` and creates `/data` with mode 0700.
- The final stage is `FROM scratch`: `/uniwrt` and `/data` (owned by 65532), `USER 65532:65532`, `ENV UNIWRT_DATA_DIR=/data`, `VOLUME /data`, the ports above, the HEALTHCHECK, OCI labels (`source` links the GHCR package to the repo), `ENTRYPOINT ["/uniwrt"]` and `CMD ["serve"]`.
- `.dockerignore` admits only `dist/uniwrt-*`, so the build context stays small.

## Cross-compilation

- **CI:** rustup targets + zig + cargo-zigbuild on ubuntu x86_64 runners: `cargo zigbuild --release --locked --target <target>`. zig 0.17.0, the same as `scripts/cross.sh`, so both build the same way.
- **Local:** native builds need rustup (D-026), bun, a C compiler that takes `-fuse-ld=mold` (gcc 12.1+ or clang) and mold (`just setup`). Cross builds run as the host user in `ghcr.io/rust-cross/cargo-zigbuild` (`scripts/cross.sh` pins 0.24.1 by digest). The image ships zig 0.16.0, so the script downloads CI's zig 0.17.0 once, checks its SHA-256, and points cargo-zigbuild at it (`CARGO_ZIGBUILD_ZIG_PATH`); the host needs no zig and no musl targets. rustup installs the `rust-toolchain.toml` toolchain and the targets inside the container. The toolchain (about 1.3 GB with the four targets), the crate registry, zig, its cache and the build output persist in `target/cross/`, so only the first run downloads; the image itself is about 6.8 GB because it bundles SDKs for other platforms.
- One cargo invocation builds all four targets in parallel (about 1.5 min from scratch with fat LTO) and writes `dist/uniwrt-{amd64,386,armv7,arm64}`, the names the Dockerfile expects. `just cross` and `just image` rebuild the web UI first, so cargo re-embeds it and relinks (about 30 s) even when nothing changed.
- **Linker (D-134):** mold 3.0.0 links every Rust build, set in `.cargo/config.toml`. Whatever is built for the host (native builds and tests, and a `--target` build's build scripts and proc macros) links through `scripts/cc-mold` (`cc … -fuse-ld=mold`, the flag last so it beats rustc's bundled rust-lld); cargo's `linker` setting is the one that reaches host artifacts, which rustflags don't. The four musl release builds call mold directly (`linker-flavor=ld`, `-m` naming the target) against rustc's self-contained musl, overriding cargo-zigbuild's linker while zig still compiles the C (ring, SQLite); armv7's `-latomic` (asked for by `libmimalloc-sys`) finds the empty `.cargo/armv7/libatomic.a`, the atomics coming from Rust's `compiler_builtins`. CI, `scripts/cross.sh` and `just setup` install mold's release build with `scripts/mold.sh` (pinned, checked against its SHA-256; `<dir>/mold` and `<dir>/ld.mold`). Check a binary's linker with `readelf -p .comment`.
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
| ui | all | `bun install --frozen-lockfile`, `vite build`, then the initial JS budget (`scripts/perf.sh js`, D-149) → artifact `web-dist`; apart from web's checks, so the builds start sooner |
| rust | all | `cargo fmt --check`, MAP.md freshness, shellcheck and the script tests, `arch-check`, `clippy -D warnings` (and again on the server with its `profiling` feature), `cargo nextest run`, the benches once each (`cargo test` on the bench targets, no timing gate; D-149), committed ts-rs types, `cargo deny check`, then the scaffold's test (`scripts/new-feature.test.sh`, about a minute on the warm target) |
| audit | all | The security scanners (D-150): zizmor over `.github/` (online audits with the job's token; `.github/zizmor.yml` turns off its Dependabot cooldown rule, D-049) and osv-scanner over `web/bun.lock` (cargo-deny covers the crates in rust). The image waits for it. |
| fuzz | all | `scripts/fuzz.sh run 20`: each fuzz target (TESTING.md, Fuzzing; D-150) for 20 s on the nightly the script pins, from the corpus cached as `fuzz-corpus-<run>` (the newest restored, saved from `main`); a finding fails the job and uploads its input and libFuzzer's log (`fuzz-finding`). About 5 minutes beside the builds. The image waits for it. |
| transport | all | `scripts/uci-conformance.sh --check` (the uci crate's fixtures against `uci` of both releases), then `scripts/test-dropbear.sh`: the `node` crate against dropbear in OpenWrt rootfs containers (24.10.8, 25.12.6): adoption with a password, key login, quoting through busybox ash, stdin, uploads, a jump host, keepalives and a ucode agent; packet captures and iperf3 (the image's iperf3 also runs on the runner's network against the controller's iperf3 server on loopback). The image waits for it. |
| agent | all | `scripts/test-agent.sh`: the node agent in OpenWrt rootfs containers (24.10.8, 25.12.6) booted with procd: the bundle's install by hash, the ops in both loop modes, collect sets parsed into facts, the engine watching the node (and the agent's peak RSS against its budget, D-149), and adoption. The image waits for it. |
| wifi | all | `scripts/wifi-lab.sh test`: the hwsim lab (mac80211_hwsim and batman-adv from the runner's kernel, installing `linux-modules-extra`, `wireless-regdb` and `iw` if missing; D-043) with the Wi-Fi and mesh tests, then the system harness's render of the lab (`crates/system/tests/wifi.rs`). The image waits for it. |
| system (one per release) | all | `scripts/test-system.sh <release>`, the two releases in parallel jobs (each about 6 min; together they were the run's longest path): rendered configs on OpenWrt 24.10.8 and 25.12.6 rootfs containers linked by veth pairs (an ISP, a gateway, an AP; D-068), loading `8021q` and `pppoe` (`linux-modules-extra` if missing): uci reads back what render wrote, `fw4 check`, netifd's WAN (DHCP untagged and on VLAN 201, static, PPPoE), `br-lan.<vid>`, the AP's lease and its route out. The image waits for it. |
| build ×4 | all (after ui) | `cargo auditable zigbuild` with the embedded UI, version and commit → smoke test: the embedded dependency list, `version --json`, and `serve` answering `healthcheck` (arm under qemu-user) → the size budget, and the cold start budget on x86_64 and i686 (D-149) → the binary and a reproducible tarball as artifacts |
| perf | all (after build) | The runtime budgets (D-149) on the x86_64 release binary: `scripts/perf.sh idle` (a demo of 10 access points and 200 clients: peak RSS and CPU over a minute) then `scripts/perf.sh api` (50 and 1,000: the p99 of 340 reads over one connection). The image waits for it. |
| compat | all (after build) | The compatibility promise with real binaries (D-152; TESTING.md, Fixtures): `scripts/compat.sh run` on the x86_64 binary: the previous release's tarball (and, from 1.0, its major's first release's; `gh release download` with the job's token, checked against SHA256SUMS) writes a data directory, a controller backup and a site file through its API with `scripts/compat/write.sh` of its own tag, and this build upgrades the directory in place, restores the backup and imports the file; logs uploaded on failure (`compat-logs`). About a minute. The image waits for it. |
| e2e | all (after build) | `web/e2e` with Playwright in the runner's Chrome against the x86_64 release binary serving `--demo --demo-seed 42` and, on a second controller, `--demo --demo-factory` (the first run); traces uploaded on failure. The image waits for it. |
| image | pushes to `main` | buildx 4-platform image from the binaries → GHCR (`X.Y.Z`, `X.Y`, from 1.0 `X`, `latest`, `sha-<7>`) with SBOM and provenance attestations; cosign keyless signature; then every platform runs `version --json` from the registry, pulled by its own manifest digest (the runner's classic image store can't hold two platforms of one index digest), and must report its target and version |
| release | pushes to `main` | SHA256SUMS and build provenance attestations for the tarballs; GitHub Release `vX.Y.Z` (creating the tag) with generated notes |
| pages | after release; a manual run on `main` | The user guide on GitHub Pages (D-151): `web/shots` takes the screenshots with Playwright in the runner's Chrome against the x86_64 release binary's demo mode (1280×800 at 2×, and the first-run wizard on a `--demo-factory` controller), `web/guide/build-site.ts` renders `docs/guide` with them (named after the release, or the last tag on a manual run), and the site becomes the Pages artifact; `pages-deploy` (`pages: write`, `id-token: write`, the `github-pages` environment) publishes it at https://robertzas.github.io/uniwrt/guide/ |

- **Critical path** (about 8 minutes on `main`): ui → build ×4 (the fat-LTO release compile, D-028) → e2e (about a minute) → image → release. The other test jobs run beside the builds and finish before them, so they add coverage, not time; splitting them further would only add cache downloads.
- **Permissions:** none at the workflow level and read-only per job, except `packages: write` and `id-token: write` (cosign) for image, `contents: write`, `id-token: write` and `attestations: write` for release, and `pages: write` and `id-token: write` for pages-deploy.
- **Secrets:** none beyond `GITHUB_TOKEN`.
- **Weekly fuzzing** (`.github/workflows/fuzz.yml`, D-150): Sundays, or by hand with the seconds per target: one job per target for 30 minutes from the corpus CI's `fuzz` job keeps, then `cmin`; a last job saves the minimized corpora as the next `fuzz-corpus-<run>`. A finding uploads `finding-<target>` and opens an issue (`Fuzzing: a finding in <target>`) unless one is open. Not a gate on releases.
- **CodeQL** (`.github/workflows/codeql.yml`, D-150): GitHub's static analysis of the Rust, TypeScript and workflow code (`security-extended` queries, no build), on pushes to `main`, pull requests and weekly; findings are code scanning alerts (the repository's Security tab) and a check on pull requests, not a gate on `main`'s release.
- **Hygiene:** actions are pinned by SHA to their newest releases (D-049), and Dependabot proposes updates, which the agent folds into one commit per checkpoint (D-030). Tools are pinned as well: bun 1.4.4 and zig 0.17.0 in the workflow's `env`; cargo-nextest 0.9.152, cargo-deny 0.21.0, cargo-zigbuild 0.24.1, cargo-auditable 0.7.7, zizmor 1.33.1, osv-scanner 2.7.1 and cargo-fuzz 0.13.2 through taiki-e/install-action, which verifies checksums. Keep them in line with `just setup` and `scripts/cross.sh`. The fuzz targets' nightly (`nightly-2026-11-25`) is pinned in `scripts/fuzz.sh` alone: CI asks the script for it. `run` steps use bash with `pipefail`. Caches: Swatinem/rust-cache per job and target (agent, transport and wifi share agent's, `containers`), saved from `main` only, since the account's cache storage (10 GB) and download volume are limited: a refused restore ("Egress is over the account limit") rebuilds everything, about 13 minutes per build; plus bun's install cache, the fuzz targets' build (`shared-key: fuzz`) and their corpus.
- **SBOM:** release binaries embed their dependency list (cargo-auditable, D-029), so the image SBOM lists every crate and `cargo audit bin` can scan a binary. The web UI's npm packages are not listed yet.
- **Verifying a release:** for the image (cosign v3 or later; v2 can't find the v3 signature format and reports "no signatures found"), `cosign verify ghcr.io/robertzas/uniwrt:<version> --certificate-identity https://github.com/robertzas/uniwrt/.github/workflows/ci.yml@refs/heads/main --certificate-oidc-issuer https://token.actions.githubusercontent.com`; for a tarball, `gh attestation verify <file> --repo robertzas/uniwrt` and `sha256sum -c SHA256SUMS`.
- **One-time manual step:** after the first image push, make the GHCR package public if it didn't inherit the repo's visibility.

## Versioning (D-008)

- Every push to `main` is a release, tagged `vX.Y.Z`. The first release is v0.1.0, and versions stay at 0.x until 1.0 (M9.4).
- The bump is computed from the Conventional Commits since the last tag:
  - breaking (`!` or `BREAKING CHANGE`) → major, but only minor while on 0.x
  - `feat` → minor
  - anything else → patch
- `Release-As: X.Y.Z` alone on a line of a commit body asks for that version: it must be above the last tag and not below what the commits call for (the highest such line since the last tag wins).
- From 1.0, a breaking change fails the version job (and so the run) unless a commit asks for the next major with `Release-As`: 1.x keeps the compatibility promise (D-152; features/lifecycle.md, Compatibility).
- Release notes are GitHub's generated notes.
- **Releasing 1.0** (after the owner's hardware run on the gateway and 3 APs, D-151): the docs say 1.0 where they say 0.x, `just check` is green, and the last release's CI run is green; then push a commit such as `chore(release): 1.0.0` whose body has `Release-As: 1.0.0`. CI releases it as any other: tag `v1.0.0`, the image's `1.0.0`, `1.0` and `1`, the tarballs, the guide on Pages.
- `scripts/next-version.sh` starts from the highest `vX.Y.Z` tag reachable from HEAD (other tags don't count), and refuses a shallow clone. `scripts/next-version.test.sh` covers the rules.

## Local commands (justfile)

| Recipe | What it does |
|---|---|
| `just setup` | Install cargo-nextest, cargo-deny, cargo-insta, zizmor and cargo-fuzz at the versions CI pins, the fuzz targets' nightly, the mold linker into `~/.local/bin`, and the web dependencies |
| `just dev` | `uniwrt serve` on 127.0.0.1 (HTTP 8080, HTTPS 8443) plus the Vite dev server, which proxies `/api` to the HTTP port; loopback is a trusted proxy, so it gets the app rather than a redirect. Ctrl-C stops both |
| `just check` | Everything CI gates on, one line per step (`scripts/check.sh`): rustfmt, clippy, the Rust tests (nextest; they also refresh `web/src/api/gen`), the code map (regenerated when stale), cargo-deny, the script tests, the web check, and shellcheck when installed. A failing step prints the first 150 lines of its output; full logs stay in `target/check/`. |
| `just test` | Rust and web tests, with full output |
| `just test-dropbear [image…]` | The transport tests against dropbear in OpenWrt rootfs containers (needs Docker); the CI job `transport` runs the same script |
| `just uci-conformance [--check] [image…]` | Runs `uci export` on the uci crate's fixtures in OpenWrt rootfs containers, requires the releases to agree and writes what it printed as `<fixture>.out` (`--check` compares instead; the CI job `transport` runs it) |
| `just test-agent [image…]` | The agent tests in OpenWrt rootfs containers booted with procd (needs Docker); the CI job `agent` runs the same script |
| `just test-system [release…]` | The system harness (`scripts/test-system.sh`; needs Docker): renders sites onto an ISP, a gateway and an AP in OpenWrt rootfs containers and checks uci, fw4 and netifd; without the kernel's 8021q, pppoe or nftables modules, their tests skip with a note; the CI job `system` runs the same script |
| `just test-wifi [nextest args…]` | The Wi-Fi and mesh tests in the hwsim lab (`scripts/wifi-lab.sh test`; needs Docker and a kernel with mac80211_hwsim and batman-adv, which it loads through a privileged container). It starts the lab unless one runs, and stops it after; local only (D-035) |
| `just e2e` | The Playwright smoke test (`web/e2e`): the web build, a debug binary with the UI embedded, then Playwright, which starts `serve --demo` on ports 18080/18443 and `serve --demo --demo-factory` on 18081/18444, each in a temporary data directory, and drives the installed Chrome |
| `just fuzz [seconds] [target…]` | The fuzz targets (`scripts/fuzz.sh run`, D-150): seeds the corpora, builds on the pinned nightly, then fuzzes each target (all by default) for `seconds` (60), one line each; it stops at a finding, whose input is in `fuzz/artifacts/<target>/` and log in `fuzz/logs/` (`scripts/fuzz.sh repro <target> <file>` replays it; `cmin` minimizes the corpora) |
| `just fmt` · `just lint` | rustfmt and Biome, writing fixes · clippy and Biome |
| `just gen` | Regenerate TypeScript types from Rust |
| `just shots` | The guide's screenshots (`web/shots`, D-151): the web build, a debug binary with the UI, then Playwright against `serve --demo` (18280/18643) and `--demo-factory` (18281/18644) into `web/shots-out`, about a minute |
| `just pages` | The guide's site from `web/shots-out` into `web/site` (open `web/site/guide/index.html`), named after the last tag |
| `just build` | Web build plus a native release binary (`target/release/uniwrt`) |
| `just perf [bin]` | The performance budgets (D-149) on `web/dist` and a binary (default `target/release/uniwrt`): initial JS, size, cold start; CI checks the release builds |
| `just profile bench [filter]` | CPU flamegraphs of the criterion benches (D-149, `scripts/profile.sh`): each bench sampled by pprof in process for `UNIWRT_PROFILE_S` s (10) under criterion's `--profile-time`, a graph in `target/criterion/<group>/<bench>/profile/flamegraph.svg`; the filter is criterion's regex (`ingest_50x1000/parse`). `cargo bench -p uniwrt-engine --bench ingest` (or `-p uniwrt-render --bench render`, `-p uniwrt-syslog --bench logs`) measures; name the bench target, as the libraries' own test harness refuses criterion's flags |
| `just profile serve [bin]` | A flamegraph of the controller (D-149): without `bin`, builds the `profiling` profile (release with its symbols, about 8 minutes) with the `profiling` feature, serves the 50 / 1,000 demo with `--profile-cpu`, lets it settle (`UNIWRT_PERF_SETTLE_S`, 60 s), reads `scripts/perf.sh`'s API set for `UNIWRT_PROFILE_S` s (30), stops it and leaves `target/profile/serve.svg`: sampled from start to exit, every thread |
| `just compat [tag…]` | The compatibility job (D-152) on a debug build: the previous release (and, from 1.0, its major's first) or the tags given write their data, which the build upgrades, restores and imports; needs `gh`, about a minute per release |
| `just perf-runtime [bin]` | The runtime budgets (D-149) on a release binary, about 4 minutes: `serve --demo --demo-access-points 10 --demo-clients 200` idle (peak RSS and CPU over a minute, after a minute to settle), then 50 and 1,000 under the API's reads (p99); the agent's RSS is checked by `just test-agent` |
| `just cross [target…]` | Static musl binaries built in the zigbuild container, written to `dist/uniwrt-<arch>`; all four targets by default |
| `just image [tag]` | The four cross binaries plus a local multi-arch image (default tag `uniwrt:dev`) |
| `just map` | Regenerate `docs/MAP.md` (`scripts/gen-map.sh`; with `--check` it only reports staleness) |
| `just arch-check` | The layering rules (`scripts/arch-check.sh`): crates per ARCHITECTURE.md's table, pure L0 crates, the web import directions; part of `just check` |
| `just sigstore-root` | Downloads Sigstore's `trusted_root.json` (Fulcio's CAs, Rekor's keys) into `crates/firmware/data/`, the trust root embedded to verify the controller's own releases (D-123). Review the diff: every key in it is trusted for later upgrades |
| `just new-feature <area>` | Scaffolds a feature area along the recipe (`scripts/new-feature.sh`; CONVENTIONS.md) |
| `just test-new-feature` | Runs the scaffold in a throwaway worktree and holds it to CI's checks; CI runs it, `just check` doesn't (it takes about a minute) |
| `just demo [seed]` | `just dev` against the simulated fleet (`serve --demo`, D-038; log in as `demo` / `demo`); `just dev <args>` passes any arguments to `serve` |
