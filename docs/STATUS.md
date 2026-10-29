# Status

**Milestone:** M1 — Core engine and read-only adoption
**Next WP:** M1.4 — engine
**Last done:** M1.3c — node-agent: streams (2026-10-28)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M1 (M1.4), D-031, D-032, D-034 and D-036 in `docs/DECISIONS.md`, `docs/ARCHITECTURE.md` (§Process structure, §Crates and layering, §Data flows, §Failure modes) and `docs/features/observability.md` (§Telemetry pipeline). Read the `//!` contracts of `crates/node` (lib.rs, `link.rs`, `proto/ops/streams.rs`, `events.rs`), `crates/store` and `crates/model` (lib.rs, `event.rs`).
2. M1.4 adds the `engine` crate: a NodeActor per node over `NodeLink`, the live state store, the event bus, intervals that follow subscriptions (15 s base, 2 s for what the UI has open), snapshots through `FactsRepo`, and node status (online/degraded/offline) with reasons.
3. To build on: after each hello (`LinkState::Up` with `AgentState::Running`), ask again for topics, intervals and the log follow: pushes end with the agent. Without `caps.events`, poll with `collect`. Parse pushes with `node::events::parse`: `Push::Tick` applies with `facts::apply`, a `Push::Event` carries a `model::event::NodeEvent`, and roams are the engine's to infer. The agent collects ticks in its loop, so keep slow sets (`batman`, `phys`) on long intervals.

## Open questions for the owner

- Router models and OpenWrt versions of the gateway and the 3 nodes, and the mesh/backhaul setup (batman? 802.11s? wired?). Captures from them would complement the hwsim fixtures.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust comes from rustup (D-026). cargo-nextest 0.9.149 and cargo-deny 0.20.2 are in `~/.cargo/bin` (`just setup`); cargo finds them there although it isn't on PATH.
- Installed: `just`, bun 1.4.4, node 26, Docker 29 with buildx 0.38 and the containerd image store, Google Chrome, python3. Not installed: shellcheck (run `koalaman/shellcheck:stable` in Docker), qemu binfmt, `bc`.
- The agent's tool shell is zsh: quote globs, and keep multi-step job control in bash scripts. Name probe containers (`--name`) and stop them by name. OpenWrt containers boot with `docker run --cap-add NET_ADMIN <image> /sbin/init` (REFERENCE.md, Containers as test nodes; `scripts/test-agent.sh`). When piping requests into an agent by hand, send node commands' output to /dev/null: it would land in the agent's stdin.
- The kernel (CachyOS) has mac80211_hwsim and batman-adv; the lab loads them through a privileged container (D-035), and the host's own Wi-Fi is phy0, which the lab never touches. NetworkManager and iwd run on the host. The host's regulatory domain is US, which the lab's `country` matches. The permission classifier refuses privileged containers run straight from the agent's shell; `scripts/wifi-lab.sh` runs its helper fine.
- `gh` is authenticated as `robertzas` with `repo` and `workflow` scopes. The GHCR package `ghcr.io/robertzas/uniwrt` is public (anonymous pulls work).
- `scripts/crate-age.sh` allows crypto-bigint 0.7.9 and ctutils 0.4.6 (D-033); drop both entries after 2026-11-01.

## Measurements

- M0.4–M0.5: the container turns healthy in about 1 s and uses 1.5 MiB (D-028). CI with cold caches: web 37 s, rust 58 s, build 1.5–2 min per target, image about 1 min; a push to `main` releases in about 5 min.
- M1.2–M1.3, with russh and the controller key: amd64 5.01 MB, 386 4.89 MB, armv7 4.74 MB, arm64 4.67 MB; idle RSS 4.5 MB. Node tests against dropbear: 8 per release in about 5 s. A container boots to SSH in about 6 s; agent tests: 7 per release in about 5 s.
- M1.3b, per collection on the lab's two-radio nodes: radios 1.7 KB, stations 1.3 KB, neighbors 1.6 KB, phys 33 KB (tri-band hwsim phys). The lab comes up in about 45 s; its 3 tests take 10 s, one at a time.
- M1.3c: the bundle is 47 KB of ucode. With every topic, 2 s ticks and a log follow, the agent's RSS is 1.9 MB plus 1.2 MB for its reader task (mostly shared pages); streaming the log adds a task and logread (2.2 MB), polling adds nothing. hostapd's notifications go out within a few ms of the kick; the gateway's capture pushed 44 lines (12.5 KB) in 25 s.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-28 — M1.3c done: ops `subscribe`/`unsubscribe` (topics hostapd, netifd, dhcp), `log_follow` (poll by default, or stream; owner's choice) and `set_interval` in `node-agent/{streams,logs,ticks}.uc` and `proto/ops/streams.rs`; `caps.topics`; `node::events` with `model::event` and `model::log`; the lab capture `fixtures/events/`, `wifi-lab.sh capture [collect|events]`; lab tests one at a time, pinging clients first (bridges forget idle MACs after 5 min). D-036.
- 2026-10-27 — M1.3b done (split: the streams moved to M1.3c): sets phys, radios, stations, leases, neighbors (with the FDB) and batman, with model types (`PhyFacts` replaces `RadioFacts.capabilities`) and parsers; the hwsim lab (`scripts/wifi-lab.sh`, `just test-wifi`) and its fixtures; `data-encoding` as a direct dependency. D-035.
- 2026-10-27 — M1.3 done (split: wireless and streams moved to M1.3b): `node-agent/` (ucode; event loop with uloop, else synchronous), typed ops, `Bundle`, `facts` parsers (board, system, capabilities, interfaces, devices). CI job `agent`, `just test-agent`. D-034.
- 2026-10-27 — M1.2 done: crate `uniwrt-node` (keys, `probe`, Session, the envelope and `Agent` multiplexer, `NodeLink`, `SshLink`); store migration 0002 (`vault` stub, `node_host_keys`). CI job `transport`, `just test-dropbear`. D-032, D-033.
- 2026-10-27 — M1.1 done: crates `uniwrt-core`, `uniwrt-model` and `uniwrt-store` (WAL, a batching writer, a read pool, migration 0001, NodeRepo/FactsRepo); `serve` opens `<data-dir>/uniwrt.db`. D-031, `just crate-age`.
