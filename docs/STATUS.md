# Status

**Milestone:** M1 — Core engine and read-only adoption
**Next WP:** M1.7 — Adopt existing node (observe mode)
**Last done:** M1.6c — REST and the WS hub (2026-10-30)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M1 (M1.7), `docs/features/adoption.md` §Path A, §Identity, credentials, trust and §Discovery, D-032 (host keys, the vault stub), D-040 and D-041 in `docs/DECISIONS.md`, and the `//!` contracts of `crates/api` (lib.rs), `crates/node` (lib.rs) and `crates/server/src/fleet.rs`.
2. Ordering point first (directive 3): M1.7 includes a UI wizard, but the app shell and design system come in M1.8. Offer the owner: M1.7's API and job now with the wizard after M1.8 (recommended), M1.8 first, or a bare wizard now.
3. M1.7 adds records to the engine at run time: the WS `fleet` topic reads a node's record when it first sees the node, so a rename or other record change needs an event (D-041).

## Open questions for the owner

- Router models and OpenWrt versions of the gateway and the 3 nodes, and the mesh/backhaul setup (batman? 802.11s? wired?). Captures from them would complement the hwsim fixtures.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust comes from rustup (D-026). cargo-nextest 0.9.149 and cargo-deny 0.20.2 are in `~/.cargo/bin` (`just setup`); cargo finds them there although it isn't on PATH. Installed: `just`, bun 1.4.4, node 26, Docker 29 with buildx 0.38 and the containerd image store, Google Chrome, python3, openssl. Not installed: shellcheck (run `koalaman/shellcheck:stable` in Docker), qemu binfmt, `bc`.
- The agent's tool shell is zsh: quote globs, and keep multi-step job control in bash scripts. Name probe containers (`--name`) and stop them by name. OpenWrt containers boot with `docker run --cap-add NET_ADMIN <image> /sbin/init` (REFERENCE.md, Containers as test nodes; `scripts/test-agent.sh`). When piping requests into an agent by hand, send node commands' output to /dev/null: it would land in the agent's stdin. If containers on the default network fail ("adding interface … to bridge docker0 failed"), `sudo systemctl restart docker` brings `docker0` back.
- The kernel (CachyOS) has mac80211_hwsim and batman-adv; the lab loads them through a privileged container (D-035), and the host's own Wi-Fi is phy0, which the lab never touches. The host's regulatory domain is US, which the lab's `country` matches. The permission classifier refuses privileged containers run straight from the agent's shell; `scripts/wifi-lab.sh` runs its helper fine.
- `gh` is authenticated as `robertzas` with `repo` and `workflow` scopes. The GHCR package `ghcr.io/robertzas/uniwrt` is public (anonymous pulls work).
- `scripts/crate-age.sh` allows crypto-bigint 0.7.9 and ctutils 0.4.6 (D-033); drop both entries after 2026-11-01.
- Cargo's sparse index cache can lag: if a resolve says a recent version doesn't exist, `cargo info <crate>` refreshes that crate's entry. Node 26's `WebSocket` takes `{ headers }` (cookie, origin), so a script can drive `/api/v1/ws` against `serve --demo` with `NODE_TLS_REJECT_UNAUTHORIZED=0`.

## Measurements

- M0.4–M0.5: the container turns healthy in about 1 s and uses 1.5 MiB (D-028). A push to `main` releases in about 5 min.
- M1.2–M1.3, with russh: amd64 5.01 MB, arm64 4.67 MB; idle RSS 4.5 MB. Agent tests: 7 per release in about 5 s; a container boots to SSH in about 6 s.
- M1.3b–c: phys 33 KB per collection on tri-band hwsim phys; the bundle is 47 KB of ucode; the agent's RSS 1.9 MB plus 1.2 MB for its reader.
- M1.4–M1.5: the engine's tests run in 0.1 s on the paused clock; `serve --demo` has its 4 nodes online about 50 ms after start. M1.6: first start takes about 5 ms in a debug build; a login (Argon2id at 19 MiB, t=2) about 19 ms. M1.6c: a node's detail is 25 KB of JSON; a WS holding `fleet`, a focused node and a log gets about 60 KB in its first 8 s, snapshots included; the api crate's 48 tests take 4.3 s (the simulated fleet runs in real time).

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-30 — M1.6c done: `uniwrt_engine::Fleet` (owner's choice) and `Engine::interests`; `api::nodes` (`NodeSummary`, `NodeDetail`); `api::ws` (`topic`, `message`, `subs`, `conn`); the `ts` feature of `core`/`model`; the server builds the API after the fleet and closes WebSockets on drain; axum `ws`. Found and fixed: the server's HTTP/2 advertises extended CONNECT, so browsers open WebSockets with CONNECT (the route and the CSRF layer now allow it). D-041.
- 2026-10-30 — M1.6b done: crate `uniwrt-api` (`Api`, `router`, `ApiError`/problem+json, `security` headers and CSRF, `Client`, `CurrentUser`, `Certificate`); `auth/` (password, limit, session, setup); migration 0003 and `UserRepo`/`SessionRepo`/`AuditRepo`; `model::user`, `UserId`; the server sets `Client` per listener (`http::https`, `plain.rs` with forwarded headers); demo user `demo`/`demo`. D-040.
- 2026-10-30 — M1.6 done (split, owner's choice: auth → M1.6b, REST/WS → M1.6c): `uniwrt_store::{Vault, MasterKey}`, `server/src/master_key.rs`; `server/src/tls/` (a name-constrained local CA and leaf via rcgen, rustls with a hot-swapped resolver, daily renewal); `/api/v1/tls`, `/api/v1/tls/ca.crt`; `http/plain.rs`; CI smoke test over HTTPS. D-039.
- 2026-10-30 — M1.5 done: crate `uniwrt-sim` (a seeded world, the agent's raw shapes, `SimLink` and `SimFleet`); `serve --demo [--demo-seed]` with `FleetLink` and a temporary database; `just demo`. D-038.
- 2026-10-28 — M1.4 done: crate `uniwrt-engine` (`Engine`, a supervised actor per node, `Live`/`NodeView`, the bus's `Event`s, `Intervals` with focus, the poller, `RoamTracker`, `Snapshots`); `serve` watches the adopted nodes. D-037.
