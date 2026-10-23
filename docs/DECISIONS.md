# Decisions

This is the index of accepted decisions (lightweight ADRs) and the open options awaiting the owner. Agents: read the tables, and open an entry only if your task touches it. When you have a new good idea, add it under "Open" with options and a recommended default, ask the owner, then move it to "Accepted".

## Accepted

| ID | Decision | Alternatives considered |
|---|---|---|
| D-001 | Backend in Rust (stable, edition 2024): tokio, axum, rusqlite (bundled), russh, rustls (ring), tracing | Go, Node, Elixir |
| D-002 | Frontend: SolidJS + Vite + TypeScript + Tailwind v4 + Kobalte. Solid 1.9 now; Solid 2.0 is still a release candidate, so revisit at the M1.8 gate. | Svelte 5, Leptos (WASM), React |
| D-003 | **Node control: SSH + ephemeral ucode agent** (JSON-lines RPC with push events); logs via logd remote syslog; a persistent footprint only in managed mode (transaction guard). *Owner's choice.* | native Rust agent; ubus JSON-RPC over HTTPS |
| D-004 | **Roadmap: observe first, then manage.** *Owner's choice.* | manage first |
| D-005 | **Primary host target: home server/NAS/mini-PC container** (x86_64/arm64). Pi, armv7 and i686 remain supported. *Owner's choice.* | Pi-first; running on the router |
| D-006 | **License: MIT OR Apache-2.0.** *Owner's choice.* | AGPL-3.0, GPL-3.0 |
| D-007 | Distribution: static musl binaries plus a `FROM scratch` multi-arch image (amd64, 386, arm/v7, arm64) on GHCR, with everything embedded. *Owner's directive.* | distroless, alpine |
| D-008 | Every push to `main` is a release, versioned with semver derived from Conventional Commits. CI never commits. | CalVer; an edge channel plus manual stable releases |
| D-009 | Persistence: one SQLite database (WAL) for config, inventory, events, metrics rollups and logs (FTS5) | Postgres, redb, an external TSDB |
| D-010 | Cross builds with cargo-zigbuild; the image is assembled from prebuilt binaries, with no QEMU during builds | cross-rs, buildx emulation |
| D-011 | Config ownership: the controller is the source of truth for each node's managed packages; unmanaged packages are never touched; raw overrides are the escape hatch; brownfield import is lossless | section-level ownership |
| D-012 | Apply protocol: a per-node transaction with a node-side dead-man rollback and a boot guard; confirm only over a fresh connection, after health checks | rpcd `uci apply`/`confirm` |
| D-013 | Mesh: batman-adv over 802.11s (SAE), batman over wired uplinks when MTU allows, bridge loop avoidance on | 802.11s only, WDS, EasyMesh |
| D-014 | Roaming: 802.11r/k/v. The controller pushes 11k neighbor reports. 11r defaults to off on WPA3 SSIDs because of a known OpenWrt 25.12 issue. | static neighbor lists |
| D-015 | Firmware: ASU builds with a declarative package set per node; rolling upgrades from leaf to gateway, with health gates | official images plus reinstalling packages |
| D-016 | Supported nodes: OpenWrt 25.12 (primary), 24.10 (best effort), behind an apk/opkg abstraction | — |
| D-017 | API: REST plus one WebSocket (topic subscriptions, deltas); TypeScript types generated from Rust (ts-rs) | OpenAPI codegen, GraphQL |
| D-018 | Web tooling: bun (installs and scripts), Biome (lint and format), Vitest, Playwright | pnpm, ESLint + Prettier |
| D-019 | Task runner: just | make, cargo xtask |
| D-020 | A simulated fleet and `--demo` mode are first-class, used for development, e2e tests and screenshots | developing on hardware only |
| D-021 | Controller TLS: self-signed by default, custom certificates supported, ACME DNS-01 later | HTTP only |
| D-022 | ZTP images never contain secrets (ASU is a public service); adopting a device needs the owner's approval | a token in the image for auto-adoption |
| D-023 | Trunk-based development: commit directly to `main` (owner's directive); `main` must stay green | PR flow |
| D-024 | **Prompt-cache-aware checkpoints.** At a WP end the agent stops for a `/clear` only when clearing pays off ("Clear or keep" in AGENTS.md); otherwise it continues into the next WP. Basis, checked 2026-10-22: Claude Code on Opus 5.5 with a 1-hour cache TTL; per MTok, input $4, cache write $5 (5 min) or $8 (1 h), cache read $0.20 (0.05×), output $20. Re-derive the thresholds when the model or prices change. *Owner's directive; continuing is the owner's choice.* | always clear at a WP end (the old rule); stop at every WP end with a keep/clear line; continue only within a milestone |
| D-025 | Embedded datasets (ISP presets, log patterns, the OUI subset) live in the crate that embeds them, under `crates/<crate>/data/`. The repo-root `data/` is the runtime data dir of local runs (`UNIWRT_DATA_DIR` defaults to `./data`) and stays gitignored. *Agent's call (reversible), M0.2.* | a top-level `data/` for datasets, which collided with the runtime default and the `.gitignore` |
| D-026 | **Local Rust toolchain: rustup**, which honors `rust-toolchain.toml`, so local builds, clippy and tests run the exact toolchain CI pins. Prompted by Arch's `rust 1:1.99.1-1` defaulting to a nonexistent linker (`x86_64-linux-gnu-gcc`). *Owner's choice, 2026-10-22.* | `linker = "cc"` in `~/.cargo/config.toml`; a symlink for the missing linker; waiting for a fixed Arch package |
| D-027 | **npm supply-chain guard:** web dependencies are pinned to exact versions, and bun resolves only versions published at least 7 days ago, transitive ones included (`minimumReleaseAge` in `web/bunfig.toml`). Most malicious npm releases are caught within days. *Agent's call (reversible), M0.3.* | caret ranges; pinning direct dependencies only |

## Open (awaiting the owner's choice; the default applies until decided)

**O-1 · How to carve the 10.0.0.0/16 address space** (needed by M4.2)
- a) A /24 per network, with the third octet equal to the VLAN ID: LAN 10.0.0.0/24 (untagged), IoT 10.0.30.0/24, Guest 10.0.40.0/24. UniFi-like and easy to remember. ← *recommended*
- b) Flat: the LAN is the whole 10.0.0.0/16 (gateway 10.0.0.1), and extra networks live outside it.
- c) A larger LAN at 10.0.0.0/22 (1,022 hosts), with /24 VLANs at 10.0.<VLAN>.0 for VLAN IDs ≥ 4.

**O-2 · Client steering** (needed by M5.5)
- a) usteer with controller-managed profiles ← *recommended for phase 1*
- b) Controller-driven smart steering using global client history (phase 2, built on a)
- c) None; rely on 11k/v hints only

**O-3 · batman-adv routing algorithm** (needed by M5.3; M2 observe data can compare)
- a) BATMAN_IV: packet-loss metric, the most proven ← *recommended*
- b) BATMAN_V: throughput metric, better for mixed wired/wireless meshes when drivers report expected throughput

**O-4 · 32-bit ARM variants**
- a) armv7 (linux/arm/v7) only ← *recommended*
- b) Also armv6 (Raspberry Pi Zero/1, linux/arm/v6)

**O-5 · UI visual direction** (decided from rendered mockups at M1.8)
- a) "Midnight glass": dark, translucent layers, neon band accents
- b) "Graphite pro": neutral and crisp, Linear/Vercel-like ← *tentative*
- c) "UniFi-familiar": light and airy, blue accent

**O-6 · Extra distribution channels** (default: none until M9)
- a) An OpenWrt package for running the controller on an arm64/x86_64 gateway
- b) A Home Assistant add-on
- c) Unraid/TrueNAS templates and a Proxmox LXC

**O-7 · Optional AI troubleshooting assistant** (default: backlog)
An opt-in panel that explains incidents from logs, metrics and topology.
- a) Bring your own API key (for example, Claude)
- b) A local model (Ollama)
- c) Not at all

**O-8 · PPSK: per-device Wi-Fi passwords that map devices to VLANs** (default: backlog, after M5.1)

**O-9 · Home Assistant/MQTT integration: presence from client joins, alerts** (default: backlog)
