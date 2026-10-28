# Status

**Milestone:** M1 — Core engine and read-only adoption
**Next WP:** M1.3b — node-agent: wireless, clients and streams (split from M1.3)
**Last done:** M1.3 — node-agent v0: core, bootstrap, system facts (2026-10-27)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M1 (M1.3b), D-034 in `docs/DECISIONS.md`, `docs/openwrt/REFERENCE.md` (§ucode, §ubus objects, §Wireless option names) and `docs/features/{observability,wifi,mesh}.md` for the collectors. Read the `//!` contracts of `crates/node` (lib.rs, `proto/ops.rs`, `facts/mod.rs`) and `crates/model` (`facts/`), plus `node-agent/main.uc`'s header.
2. A new set = `node-agent/collect/<set>.uc` + a line in `collect.uc` + a `Set` variant + `facts/<set>.rs` + a fixture. Sets return raw sources (ubus replies, sysfs text); Rust parses. New ops go in `proto/ops.rs` and `ops.uc`, plus the bundle's `FILES` list for new files (a test checks it).
3. M1.3b adds model types (radios and stations exist; leases, neighbors, batman don't) and, under the event loop only, subscriptions, `log_follow` and ticks. `caps.events` tells whether the agent can push; the 24.10 rootfs has no uloop, the 25.12 one has.
4. No Wi-Fi in containers: wireless fixtures come from specs (mark ⚠️) until the owner's nodes are captured. `crates/node/fixtures/collect/gateway.json` must keep parsing into `crates/model/fixtures/facts/gateway.json`; extend both together.

## Open questions for the owner

- Router models and OpenWrt versions of the gateway and the 3 nodes, and the mesh/backhaul setup (batman? 802.11s? wired?). Captures from them would replace the spec-based wireless fixtures.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust comes from rustup (D-026). cargo-nextest 0.9.149 and cargo-deny 0.20.2 are in `~/.cargo/bin` (`just setup`); cargo finds them there although it isn't on PATH.
- Installed: `just`, bun 1.4.4, node 26, Docker 29 with buildx 0.38 and the containerd image store, Google Chrome, python3. Not installed: shellcheck (run `koalaman/shellcheck:stable` in Docker), qemu binfmt, `bc`.
- The agent's tool shell is zsh: quote globs, and keep multi-step job control in bash scripts. Name probe containers (`--name`) and stop them by name.
- OpenWrt containers: `docker run --cap-add NET_ADMIN <image> /sbin/init` boots procd with ubus and netifd (REFERENCE.md, Containers as test nodes); `scripts/test-agent.sh` shows the setup.
- `gh` is authenticated as `robertzas` with `repo` and `workflow` scopes. The GHCR package `ghcr.io/robertzas/uniwrt` is public (anonymous pulls work).
- `scripts/crate-age.sh` allows crypto-bigint 0.7.9 and ctutils 0.4.6 (D-033); drop both entries after 2026-11-01.

## Measurements

- M0.4: the container turns healthy in about 1 s and uses 1.5 MiB; health endpoint 7 µs CPU per request (D-028).
- M0.5, CI (cold caches): web 37 s, rust 58 s, build 1.5–2 min per target, image about 1 min; a push to `main` releases in about 5 min.
- M1.2, with russh and the controller key: amd64 5.01 MB, 386 4.89 MB, armv7 4.74 MB, arm64 4.67 MB; idle RSS 4.5 MB. Node tests: against dropbear, 8 per release in about 5 s.
- M1.3: the agent bundle is 20 KB of ucode. On x86_64 the agent's RSS is 1.4 MB, plus 1.0 MB for the uloop reader task (mostly shared pages). A container boots to SSH in about 6 s; agent tests: 6 per release in about 4.3 s.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-27 — M1.3 done (split: wireless and streams moved to M1.3b): `node-agent/` (ucode; event loop with uloop, else synchronous), typed ops (`proto/ops.rs`: ping, collect, ubus, exec, read_file), `Bundle` (tar + SHA256SUMS, installed by hash under flock; `AgentLaunch::Bundle` checks the hello's hash), `facts` parsers (board, system, capabilities, interfaces, devices) with container and canonical fixtures; lossy UTF-8 in the envelope. CI job `agent`, `just test-agent`. D-034.
- 2026-10-27 — M1.2 done: crate `uniwrt-node` (keys, `probe`, Session, the envelope and `Agent` multiplexer, `NodeLink`, `SshLink`); store migration 0002 (`vault` stub, `node_host_keys`). CI job `transport`, `just test-dropbear`. D-032, D-033.
- 2026-10-27 — M1.1 done: crates `uniwrt-core`, `uniwrt-model` and `uniwrt-store` (WAL, a batching writer, a read pool, migration 0001, NodeRepo/FactsRepo); `serve` opens `<data-dir>/uniwrt.db`. D-031, `just crate-age`.
- 2026-10-27 — M0.5 done: the image smoke test runs each platform by its own manifest digest. v0.1.0 released and verified. D-030: the agent batches Dependabot updates.
