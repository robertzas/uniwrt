# Roadmap

Strategy: **observe first, then manage** (D-004). Each work package (WP) is sized for one focused session. The next milestones have detailed acceptance criteria; later ones are refined just in time, and the first task of every milestone is to refine its WPs.
Legend: `[x]` done · `[ ]` to do · (S/M/L) size · → dependencies

## M0 — Foundation: walking skeleton and delivery pipeline

Goal: a real (if empty) product ships. Every push to `main` builds, tests and releases binaries plus a scratch image for 4 architectures.

- [x] **M0.1 Plan & directives** (M): README, AGENTS/CLAUDE, docs, license, GitHub repo.
- [x] **M0.2 Rust workspace + server skeleton** (M) → M0.1
  - Cargo workspace with `crates/server` (package `uniwrt`, bin `uniwrt`), `rust-toolchain.toml` pinned to stable, workspace lints, and the release profile from BUILD.md.
  - `uniwrt serve` (axum): `GET /api/v1/health` returns `{status, version, commit, built_at}`. Serves the embedded UI with precompressed assets and an SPA fallback. When `web/dist` is absent, a placeholder page is embedded instead, so the backend builds without Node.
  - `uniwrt healthcheck` (exit code 0/1, for Docker) and `uniwrt version`. Config through flags or env: `UNIWRT_HTTP_LISTEN` (default `0.0.0.0:8080`) and `UNIWRT_DATA_DIR` (default `./data`). Tracing, and graceful shutdown on SIGTERM/SIGINT.
  - Tests: the health handler, asset serving (content-encoding negotiation, cache headers) and the SPA fallback.
- [x] **M0.3 Web skeleton** (M) → M0.1
  - `web/`: bun, Vite, Solid 1.9, strict TypeScript, Tailwind v4 (`tokens.css` with dark and light themes), Biome, Vitest with Testing Library.
  - Placeholder app shell (wordmark, sidebar skeleton, a health badge fed by `/api/v1/health`) and a Vite proxy to the backend.
  - At least one component test. The build writes hashed assets to `web/dist/assets/` (BUILD.md, Embedded UI) and uses no inline scripts or styles (the CSP in security.md).
  - The health badge's type is generated from the Rust DTO (`crates/server/src/http/health.rs`) with ts-rs (D-017); M0.4 wraps the export in `just gen`.
- [x] **M0.4 Build tooling & image** (M) → M0.2, M0.3
  - justfile (dev, check, test, fmt, lint, build, cross, image, map, plus a demo stub) and `scripts/gen-map.sh` → `docs/MAP.md`.
  - Cross builds for the 4 musl targets with cargo-zigbuild; locally they run inside the cargo-zigbuild container image.
  - `FROM scratch` Dockerfile per BUILD.md: prebuilt binary chosen by TARGETARCH/TARGETVARIANT, UID 65532, `/data`, HEALTHCHECK.
  - Local verification: the image runs and `/api/v1/health` answers. Record binary sizes in STATUS.
- [x] **M0.5 CI/CD** (M) → M0.4
  - `.github/workflows/ci.yml` per BUILD.md: version → web → rust → build ×4 (with qemu smoke tests) → image (GHCR, SBOM and provenance, cosign) → release (tag, assets, notes) on `main`. PRs only build and test. CI fails if `docs/MAP.md` is stale.
  - `scripts/next-version.sh` with a test script, cargo-deny config, Dependabot (cargo, npm, actions), and CI/release badges in the README.
  - Acceptance: a push to `main` produces release v0.1.0 with 4 tarballs and SHA256SUMS plus a 4-platform image, and `docker run --platform linux/arm/v7 ghcr.io/robertzas/uniwrt:latest version` works.

## M1 — Core engine and read-only adoption ("see your network")

Goal: adopt the owner's existing nodes read-only and watch them live in a polished app shell.

- [x] **M1.1 core, model (facts), store** (M) → M0
  - `core`: IDs, MacAddr, network types, error conventions, a `Clock` trait. `model::facts`: NodeFacts, Capabilities, RadioFacts, StationFacts and InterfaceFacts (serde, with fixtures).
  - `store`: SQLite in WAL mode, embedded migrations, a batching writer actor and repository traits. Tests use a temporary DB.
- [x] **M1.2 SSH transport** (L) → M1.1
  - `node` crate: russh client; a controller ed25519 key (generated and kept in a vault stub); TOFU host-key pinning; a `NodeLink` trait (call/exec/stream/upload/events); a connection actor with keepalive and jittered backoff; jump hosts through `direct-tcpip`.
  - Tests against an in-process russh server, plus dropbear in an OpenWrt rootfs container (CI job).
  - Also done here (D-032): the agent envelope in `proto.rs` (requests, answers, events, ticks, hello, the protocol range) and the RPC multiplexer, which `NodeLink::call` needs.
- [x] **M1.3 node-agent v0: core, bootstrap, system facts** (L) → M1.2
  - A ucode agent with two loop modes (D-034): hello and caps, `ping`, `collect` (sets board, system, capabilities, interfaces, devices), `ubus`, `exec` (argv, timeout, bounded output), `read_file`. The ops are typed in `proto/ops.rs` and mirrored in `proto.uc`/`ops.uc`; the agent exits when its stdin closes.
  - Upload-by-hash bootstrap into `/tmp/uniwrt/agent`; parsing into `model::facts`, tested with fixtures captured from rootfs containers and with the canonical gateway.
  - Agent tests in OpenWrt rootfs containers booted with procd (24.10 and 25.12; the CI job `agent`).
- [x] **M1.3b node-agent: wireless, clients and mesh sets** (L) → M1.3 (split from M1.3; the streams moved to M1.3c)
  - Collect sets phys, radios, stations, leases, neighbors (with the bridge FDB) and batman, with their model types and parsers: nl80211 through ucode-mod-nl80211, netifd and hostapd over ubus, rtnetlink with a /proc fallback, dnsmasq and odhcpd, and batctl's JSON queries (D-035).
  - The hwsim lab (`just test-wifi`): virtual radios and batman-adv in OpenWrt 24.10 and 25.12 containers. Its sanitized captures are the wireless fixtures until the owner's nodes are captured.
- [ ] **M1.3c node-agent: streams** (M) → M1.3b
  - Under the event loop: `subscribe`/`unsubscribe` (hostapd, netifd and dnsmasq over ubus, re-subscribing as objects come and go), `log_follow` and `set_interval` ticks. The synchronous mode answers them `unsupported`, and the engine polls instead.
  - Tested in the hwsim lab: association events from the client, log lines, ticks.
- [ ] **M1.4 engine** (M) → M1.3c
  - A NodeActor per node, the live state store, the event bus, subscription-driven adaptive intervals, snapshot persistence, and node status (online/degraded/offline) with reasons.
- [ ] **M1.5 Simulator + demo mode** (M) → M1.4
  - `sim` crate implementing `NodeLink`: a gateway, 3 APs and about 30 clients, with roaming, signal drift, events and logs. Deterministic seed. `uniwrt serve --demo`.
- [ ] **M1.6 API foundation, auth, TLS** (M) → M1.4
  - axum route skeleton, problem+json errors, a WS hub with topics and deltas, ts-rs codegen (`just gen`), first-run admin setup, login and sessions (Argon2id), a self-signed TLS certificate (rcgen) on 8443 that the user can download, and the real vault: XChaCha20-Poly1305 under the master key, resealing the stub's `alg = 'plain'` rows (D-032).
- [ ] **M1.7 Adopt existing node (observe mode)** (M) → M1.6
  - API and UI wizard: IP + root password → confirm the host-key fingerprint → install the key → inventory and config backup → live view. Batch-adopt the rest from the gateway's DHCP leases.
  - Acceptance: the owner's 4 nodes are adopted, with zero config changes other than `authorized_keys`.
- [ ] **M1.8 App shell & design system v1** (L) → M1.6
  - Decision gates: is Solid 2.0 GA, and is the ecosystem ready? Settle the UI direction (O-5) by showing the owner 3 rendered mockups.
  - Tokens and primitives (Button, Card, Badge, virtualized Table, Tabs, Dialog, Drawer, Tooltip, Toast, Stat, Sparkline), layout, sidebar, top bar, ⌘K palette, theme toggle, plus a Devices list and a live Device overview.
- [ ] **M1.9 Dev accelerators** (S) → M1.6
  - `just new-feature <area>` (scaffolds the files in the feature recipe) and `just arch-check` (enforces the layering rules from ARCHITECTURE.md), both wired into `just check` and CI.

## M2 — Observability ("understand your network")

- [ ] **M2.1 Topology engine** (M): the pure `topo` crate builds the graph from facts; API and WS topic; fixture-based tests.
- [ ] **M2.2 Topology view** (L): Canvas renderer, d3-force in a worker, tree/RF/clients views, live flows, pin/drag, details panel, 24 h replay slider.
- [ ] **M2.3 Clients** (M): inventory, OUI vendor, device icons, history and roams, experience score v1, actions (rename, kick, block).
- [ ] **M2.4 Metrics & dashboard** (M): rollups, retention, LTTB downsampling, uPlot charts, dashboard v1.
- [ ] **M2.5 Logs** (L): syslog receiver (UDP/TCP), logread fallback, FTS5 search, live-tail UI, pattern annotations (`log-patterns.toml`).
- [ ] **M2.6 Events & alerts** (M): event timeline, alert rules, notifiers (ntfy, webhook and email first).
- [ ] **M2.7 RF insight** (M): channel surveys, scheduled scans, the inter-node RSSI matrix, DFS events, spectrum view.

## M3 — Config engine ("change safely")

- [ ] **M3.1 Desired-state model v1, revisions, vault** (M)
- [ ] **M3.2 `uci` crate** (M): AST, parser, serializer, semantic diff; proptest round-trips; fixtures from real configs.
- [ ] **M3.3 `render` v1** (L): system, network (DSA and swconfig), dhcp, firewall and wireless basics; capability-based; golden tests.
- [ ] **M3.4 Transactions & jobs** (L): agent transaction and boot guard, controller verify/confirm, the `jobs` crate (persisted, resumable), multi-node ordering.
- [ ] **M3.5 Brownfield import & drift** (L): lossless import, raw overrides, per-node managed-package toggles, drift detection.
- [ ] **M3.6 Review & apply UI** (M): pending-changes tray, per-node diffs, impact summary, live progress, history and rollback.
- [ ] **M3.7 System test harness** (M): OpenWrt x86-64 images (24.10, 25.12) under QEMU or as rootfs containers in CI, validating rendered configs (uci, `fw4 check`, netifd, wifi on mac80211_hwsim).

## M4 — Gateway: WAN, LAN and firewall ("own the primary router")

- [ ] **M4.1 ISP presets & WAN** (M): preset format and seed set; WAN types including VLAN, 6rd and DHCPv6-PD; wizard; live connectivity test.
- [ ] **M4.2 IPAM & networks** (M): carving the 10.0.0.0/16 space (O-1), networks/VLANs, DHCP pools and reservations, local DNS.
- [ ] **M4.3 Firewall policy** (M): zones from networks, the inter-network matrix, port forwards, UPnP.
- [ ] **M4.4 Factory gateway bootstrap** (M): first-run wizard with make-before-break renumbering.
- [ ] **M4.5 SQM & failover** (M): cake with speed-test autotuning; optional mwan3 failover.

## M5 — Wi-Fi and mesh ("build the mesh")

- [ ] **M5.1 WLANs** (M): security modes, VLAN binding, bands, node scoping, guest isolation, schedules.
- [ ] **M5.2 Roaming** (M): 802.11r (PSK and SAE key distribution), controller-pushed 11k neighbor reports, 11v, management of the full wpad package.
- [ ] **M5.3 Mesh backhaul** (L): batman-adv over 802.11s and wired uplinks (with an MTU probe), BLA/DAT, VLANs over the mesh, mesh health.
- [ ] **M5.4 Adoption paths** (L): ZTP via ASU with the pending-adoption inbox, the adoption port, the one-liner, the apk/opkg abstraction, dumb-AP conversion.
- [ ] **M5.5 Steering** (M): usteer profiles (O-2) and a steering/roaming timeline.

## M6 — RF optimization ("optimize the air")

- [ ] **M6.1 RF planner** (L): `rf` crate with constraints, scoring, search and explanations; simulation tests.
- [ ] **M6.2 Plan apply & scheduler** (M): rolling and synchronized cutovers with rollback; a nightly optimizer with hysteresis.
- [ ] **M6.3 Floor plans & heatmap** (L): floor-plan editor and a calibrated path-loss heatmap.

## M7 — Lifecycle ("keep it current")

- [ ] **M7.1 Firmware catalog & ASU** (M)
- [ ] **M7.2 Rolling upgrades** (L): leaf → gateway ordering, health gates, soak periods, canary, pause/resume, pipeline UI.
- [ ] **M7.3 Backups & restore** (M)
- [ ] **M7.4 Replace, re-adopt, decommission, and controller update notice** (M)

## M8 — Services and troubleshooting ("never SSH again")

- [ ] **M8.1 DNS suite, DDNS, NTP** (M)
- [ ] **M8.2 WireGuard VPN** (M)
- [ ] **M8.3 Other services & package manager UI** (M)
- [ ] **M8.4 Diagnostics toolkit, web terminal, UCI explorer** (L)
- [ ] **M8.5 Connection doctor, WAN quality monitor, support bundle** (L)

## M9 — Polish and 1.0

- [ ] **M9.1 UX polish, accessibility, mobile, onboarding** (L)
- [ ] **M9.2 Performance budgets enforced in CI; profiling** (M)
- [ ] **M9.3 Security review, fuzzing, threat model** (M)
- [ ] **M9.4 User guide, screenshots from demo mode, 1.0 release** (M)

## Backlog (unscheduled; see the open options in DECISIONS.md)

AI troubleshooting assistant (O-7) · PPSK (O-8) · Home Assistant/MQTT presence (O-9) · Prometheus exporter · multi-site · captive portal (openNDS) · controller package for gateways (O-6) · DNS query analytics · parental-controls UI · PWA push notifications · secondary gateways and multi-WAN at APs.
