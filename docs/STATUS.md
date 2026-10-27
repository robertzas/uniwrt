# Status

**Milestone:** M1 — Core engine and read-only adoption
**Next WP:** M1.3 — node-agent v0 (ucode agent, ops, bootstrap, facts parsing)
**Last done:** M1.2 — SSH transport (`node` crate) (2026-10-27)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M1 (M1.3), `docs/ARCHITECTURE.md` §Node protocol, `docs/openwrt/REFERENCE.md` (ucode, ubus, collectors) and `docs/dev/TESTING.md`. Read the `//!` contracts of `crates/node` (lib.rs, `proto.rs`, `agent.rs`) and `crates/model` (lib.rs, `facts/`), not their implementations.
2. M1.2 already ships the envelope (`proto.rs`: requests, answers, events, ticks, hello, `SUPPORTED`) and the multiplexer (`Agent`); M1.3 adds the ops with typed args and results, `node-agent/proto.uc`, the bootstrap (`cat /tmp/uniwrt/VERSION`, `tar xzf -` over stdin, `sha256sum`) and the parsers into `model::facts`. `SshLink`'s `LinkConfig::agent` is the agent's argv.
3. Known facts about the OpenWrt rootfs (24.10.8, 25.12.6): ucode reads lines with `fs.stdin.read("line")` and needs `stdout.flush()`, and `sprintf("%J")` prints one-line JSON. `ECHO_AGENT` in `crates/node/tests/dropbear.rs` is a working minimal agent. BusyBox there has no `stat` and no `timeout`, so the agent enforces exec timeouts itself. The agent must exit when its stdin closes.
4. The agent tests are a new CI job (BUILD.md, `agent`); `scripts/test-dropbear.sh` shows the container pattern (a random root password, a pre-generated host key, at most 2 concurrent tests because of dropbear's `MAX_UNAUTH_PER_IP`).

## Open questions for the owner

- Router models and OpenWrt versions of the gateway and the 3 nodes, and the mesh/backhaul setup (batman? 802.11s? wired?). M1.3 wants sanitized outputs captured from them as fixtures; until then, fixtures come from rootfs containers and the specs.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust comes from rustup (D-026). cargo-nextest 0.9.149 and cargo-deny 0.20.2 are in `~/.cargo/bin` (`just setup`); cargo finds them there although it isn't on PATH.
- Installed: `just`, bun 1.4.4, node 26, Docker 29 with buildx 0.38 and the containerd image store, Google Chrome, python3. Not installed: shellcheck (run `koalaman/shellcheck:stable` in Docker) and qemu binfmt (run arm binaries with qemu-user in a `debian:trixie-slim` container).
- The agent's tool shell is zsh: quote globs, and keep multi-step job control in bash scripts.
- UI check without a browser window: `google-chrome-stable --headless=new --screenshot=<png> --window-size=1280,800 [--force-dark-mode] <url>`. With `--enable-logging=stderr`, CSP violations show up as `CONSOLE` lines.
- `gh` is authenticated as `robertzas` with `repo` and `workflow` scopes. The GHCR package `ghcr.io/robertzas/uniwrt` is public (anonymous pulls work).
- `scripts/crate-age.sh` allows crypto-bigint 0.7.9 and ctutils 0.4.6 (D-033); drop both entries after 2026-11-01.

## Measurements

- M0.4: the container turns healthy in about 1 s and uses 1.5 MiB; health endpoint 7 µs CPU per request (D-028).
- M0.5, CI (cold caches): web 37 s, rust 58 s, build 1.5–2 min per target, image about 1 min; a push to `main` releases in about 5 min.
- M1.1, static musl binaries with UI and SQLite: amd64 4.63 MB, 386 4.51 MB, armv7 4.38 MB, arm64 4.33 MB. Native idle RSS with the database open: 3.9 MB.
- M1.2, with russh and the controller key: amd64 5.01 MB, 386 4.89 MB, armv7 4.74 MB, arm64 4.67 MB (about +0.36 MB); idle RSS 4.5 MB. Node tests: 56 in 0.6 s (loopback server); against dropbear, 8 per release in about 5 s.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-27 — M1.2 done: crate `uniwrt-node` (ControllerKey, HostKey with `probe`, Session with exec/stream/upload over direct or `direct-tcpip` jumps, the `proto` envelope and the `Agent` multiplexer, `NodeLink`, `SshLink` with jittered backoff, quarantine and agent restarts); store migration 0002 (`vault` stub, `node_host_keys`) with VaultRepo and HostKeyRepo; `serve` generates the controller key on the first run. CI job `transport` (dropbear in OpenWrt 24.10.8 and 25.12.6), `just test-dropbear`. D-032 (transport design, ISC allowed), D-033 (the owner's security-fix exception to the cooldown: RUSTSEC-2026-0387/0385).
- 2026-10-27 — M1.1 done: crates `uniwrt-core`, `uniwrt-model` and `uniwrt-store` (WAL, a batching writer, a read pool, migration 0001, NodeRepo/FactsRepo); `serve` opens `<data-dir>/uniwrt.db`. D-031, `just crate-age`.
- 2026-10-27 — M0.5 done: the image smoke test runs each platform by its own manifest digest. v0.1.0 released and verified. D-030: the agent batches Dependabot updates.
- 2026-10-22 — M0.5: ci.yml (version → web, rust → build ×4 → image → release), `scripts/next-version.sh` with tests, deny.toml, Dependabot with a 7-day cooldown (D-029), cargo-auditable binaries.
