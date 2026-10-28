# Status

**Milestone:** M1 — Core engine and read-only adoption
**Next WP:** M1.3c — node-agent: streams (split from M1.3b)
**Last done:** M1.3b — node-agent: wireless, clients and mesh sets (2026-10-27)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M1 (M1.3c), D-034 and D-035 in `docs/DECISIONS.md`, `docs/openwrt/REFERENCE.md` (§ucode, §ubus objects) and `docs/features/observability.md` (§Telemetry pipeline, §Logs). Read the `//!` contracts of `crates/node` (lib.rs, `proto.rs`, `proto/ops.rs`) and the headers of `node-agent/main.uc` and `ops.uc`.
2. M1.3c adds `subscribe`/`unsubscribe`, `log_follow` and `set_interval` to `proto/ops.rs` and `ops.uc`, under the event loop only (`caps.events`); the synchronous mode answers `unsupported`. Events are `{"ev":…}` lines and ticks `{"tick":…}` (`proto::AgentEvent`); a tick can reuse `Collected`'s shape.
3. To build on: ucode's ubus subscribers need the uloop. `hostapd.<ifname>` objects come and go with `wifi reload`, so re-subscribe on `ubus.object.add`. hostapd notifies probe requests too; drop them in the agent. logd's `log read` (`{"lines":N,"stream":false}`) returns entries with an `id` sequence, `priority` and ms `time` in both releases. Polling it by id is an alternative to running `logread -f` in a task: offer the owner the choice (directive 3).
4. Test in the hwsim lab: `scripts/wifi-lab.sh up` (about 45 s), then `just test-wifi`. Kicking client c (`ubus call hostapd.phy1-ap0 del_client`) gives disassociation and association events. `scripts/wifi-lab.sh down` stops the lab and unloads the modules.

## Open questions for the owner

- Router models and OpenWrt versions of the gateway and the 3 nodes, and the mesh/backhaul setup (batman? 802.11s? wired?). Captures from them would complement the hwsim fixtures.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust comes from rustup (D-026). cargo-nextest 0.9.149 and cargo-deny 0.20.2 are in `~/.cargo/bin` (`just setup`); cargo finds them there although it isn't on PATH.
- Installed: `just`, bun 1.4.4, node 26, Docker 29 with buildx 0.38 and the containerd image store, Google Chrome, python3. Not installed: shellcheck (run `koalaman/shellcheck:stable` in Docker), qemu binfmt, `bc`.
- The agent's tool shell is zsh: quote globs, and keep multi-step job control in bash scripts. Name probe containers (`--name`) and stop them by name. OpenWrt containers boot with `docker run --cap-add NET_ADMIN <image> /sbin/init` (REFERENCE.md, Containers as test nodes; `scripts/test-agent.sh`).
- The kernel (CachyOS) has mac80211_hwsim and batman-adv; the lab loads them through a privileged container (D-035), and the host's own Wi-Fi is phy0, which the lab never touches. NetworkManager and iwd run on the host. The host's regulatory domain is US, which the lab's `country` matches.
- `gh` is authenticated as `robertzas` with `repo` and `workflow` scopes. The GHCR package `ghcr.io/robertzas/uniwrt` is public (anonymous pulls work).
- `scripts/crate-age.sh` allows crypto-bigint 0.7.9 and ctutils 0.4.6 (D-033); drop both entries after 2026-11-01.

## Measurements

- M0.4–M0.5: the container turns healthy in about 1 s and uses 1.5 MiB (D-028). CI with cold caches: web 37 s, rust 58 s, build 1.5–2 min per target, image about 1 min; a push to `main` releases in about 5 min.
- M1.2, with russh and the controller key: amd64 5.01 MB, 386 4.89 MB, armv7 4.74 MB, arm64 4.67 MB; idle RSS 4.5 MB. Node tests: against dropbear, 8 per release in about 5 s.
- M1.3: the agent bundle is 20 KB of ucode. On x86_64 the agent's RSS is 1.4 MB, plus 1.0 MB for the uloop reader task (mostly shared pages). A container boots to SSH in about 6 s; agent tests: 6 per release in about 4.3 s.
- M1.3b, per collection on the lab's two-radio nodes: radios 1.7 KB, stations 1.3 KB, neighbors 1.6 KB, phys 33 KB (tri-band hwsim phys). The lab comes up in about 45 s, and its 2 tests take 3 s.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-27 — M1.3b done (split: the streams moved to M1.3c): sets phys, radios, stations, leases, neighbors (with the FDB) and batman, with model types (`PhyFacts` replaces `RadioFacts.capabilities`) and parsers; the hwsim lab (`scripts/wifi-lab.sh`, `just test-wifi`) and its fixtures; `data-encoding` as a direct dependency. D-035.
- 2026-10-27 — M1.3 done (split: wireless and streams moved to M1.3b): `node-agent/` (ucode; event loop with uloop, else synchronous), typed ops, `Bundle`, `facts` parsers (board, system, capabilities, interfaces, devices). CI job `agent`, `just test-agent`. D-034.
- 2026-10-27 — M1.2 done: crate `uniwrt-node` (keys, `probe`, Session, the envelope and `Agent` multiplexer, `NodeLink`, `SshLink`); store migration 0002 (`vault` stub, `node_host_keys`). CI job `transport`, `just test-dropbear`. D-032, D-033.
- 2026-10-27 — M1.1 done: crates `uniwrt-core`, `uniwrt-model` and `uniwrt-store` (WAL, a batching writer, a read pool, migration 0001, NodeRepo/FactsRepo); `serve` opens `<data-dir>/uniwrt.db`. D-031, `just crate-age`.
