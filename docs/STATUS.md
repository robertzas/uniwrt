# Status

**Milestone:** M1 — Core engine and read-only adoption
**Next WP:** M1.6c — REST and the WS hub
**Last done:** M1.6b — api crate, auth and sessions (2026-10-30)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M1 (M1.6c), D-017, D-037, D-038 and D-040 in `docs/DECISIONS.md`, ARCHITECTURE.md §API and §Data flows, and the `//!` contracts of `crates/api` (lib.rs), `crates/engine` (lib.rs) and `crates/server` (main.rs).
2. M1.6c: node and fleet routes and the WS hub (`/api/v1/ws`: `{"sub":[...]}`, a snapshot then batched deltas, at most 10/s per topic; viewing a node calls `Engine::focus`), DTOs with ts-rs, tested against `serve --demo`. Every new route takes `CurrentUser` (only health, tls and the auth routes are public); state-changing ones pass the CSRF layer by themselves. The WS upgrade is a GET: check `Origin` against `Client::origin` there explicitly.
3. Design point first: the API needs the engine, which the server runs on `FleetLink` (`server/src/fleet_link.rs`), and `api` cannot depend on `server`. Either make `Api` generic over `L: NodeLink`, or move `FleetLink` into a crate below `api`. Offer the owner the choice (directive 3).

## Open questions for the owner

- Router models and OpenWrt versions of the gateway and the 3 nodes, and the mesh/backhaul setup (batman? 802.11s? wired?). Captures from them would complement the hwsim fixtures.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust comes from rustup (D-026). cargo-nextest 0.9.149 and cargo-deny 0.20.2 are in `~/.cargo/bin` (`just setup`); cargo finds them there although it isn't on PATH. Installed: `just`, bun 1.4.4, node 26, Docker 29 with buildx 0.38 and the containerd image store, Google Chrome, python3, openssl. Not installed: shellcheck (run `koalaman/shellcheck:stable` in Docker), qemu binfmt, `bc`.
- The agent's tool shell is zsh: quote globs, and keep multi-step job control in bash scripts. Name probe containers (`--name`) and stop them by name. OpenWrt containers boot with `docker run --cap-add NET_ADMIN <image> /sbin/init` (REFERENCE.md, Containers as test nodes; `scripts/test-agent.sh`). When piping requests into an agent by hand, send node commands' output to /dev/null: it would land in the agent's stdin. If containers on the default network fail ("adding interface … to bridge docker0 failed"), `sudo systemctl restart docker` brings `docker0` back.
- The kernel (CachyOS) has mac80211_hwsim and batman-adv; the lab loads them through a privileged container (D-035), and the host's own Wi-Fi is phy0, which the lab never touches. The host's regulatory domain is US, which the lab's `country` matches. The permission classifier refuses privileged containers run straight from the agent's shell; `scripts/wifi-lab.sh` runs its helper fine.
- `gh` is authenticated as `robertzas` with `repo` and `workflow` scopes. The GHCR package `ghcr.io/robertzas/uniwrt` is public (anonymous pulls work).
- `scripts/crate-age.sh` allows crypto-bigint 0.7.9 and ctutils 0.4.6 (D-033); drop both entries after 2026-11-01.

## Measurements

- M0.4–M0.5: the container turns healthy in about 1 s and uses 1.5 MiB (D-028). A push to `main` releases in about 5 min.
- M1.2–M1.3, with russh: amd64 5.01 MB, arm64 4.67 MB; idle RSS 4.5 MB. Agent tests: 7 per release in about 5 s; a container boots to SSH in about 6 s.
- M1.3b–c: phys 33 KB per collection on tri-band hwsim phys; the bundle is 47 KB of ucode; the agent's RSS 1.9 MB plus 1.2 MB for its reader.
- M1.4–M1.5: the engine's tests run in 0.1 s on the paused clock; `serve --demo` has its 4 nodes online about 50 ms after start.
- M1.6: first start (master key, SSH key, CA and leaf) takes about 5 ms in a debug build. M1.6b: a login (Argon2id at 19 MiB, t=2) answers in about 19 ms on the dev machine, an unknown user's too; the api crate's 26 tests take 0.07 s at the cheapest cost.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-30 — M1.6b done: crate `uniwrt-api` (`Api`, `router`, `ApiError`/problem+json, `security` headers and CSRF, `Client`, `CurrentUser`, `Certificate`); `auth/` (password, limit, session, setup); migration 0003 and `UserRepo`/`SessionRepo`/`AuditRepo`; `model::user`, `UserId`; the server sets `Client` per listener (`http::https`, `plain.rs` with forwarded headers); demo user `demo`/`demo`. D-040.
- 2026-10-30 — M1.6 done (split, owner's choice: auth → M1.6b, REST/WS → M1.6c): `uniwrt_store::{Vault, MasterKey}` (XChaCha20-Poly1305, resealing the stub's rows), `server/src/master_key.rs`; `server/src/tls/` (a name-constrained local CA and leaf via rcgen, rustls with a hot-swapped resolver, daily renewal, `TlsListener`/`PlainListener`/`Peer`); `/api/v1/tls`, `/api/v1/tls/ca.crt`; `http/plain.rs` (307 redirect, health, `--trusted-proxies`); flags `--https-listen`, `--tls-names`, `--trusted-proxies`, `--master-key-file`; CI smoke test over HTTPS. D-039.
- 2026-10-30 — M1.5 done: crate `uniwrt-sim` (a seeded world, the agent's raw shapes, `SimLink` and `SimFleet`); `serve --demo [--demo-seed]` with `FleetLink` and a temporary database; `just demo`. D-038.
- 2026-10-28 — M1.4 done: crate `uniwrt-engine` (`Engine`, a supervised actor per node, `Live`/`NodeView`, the bus's `Event`s, `Intervals` with focus, the poller, `RoamTracker`, `Snapshots`); `serve` watches the adopted nodes. D-037.
- 2026-10-28 — M1.3c done: ops `subscribe`/`unsubscribe`, `log_follow` and `set_interval`; `node::events` with `model::event` and `model::log`; the lab capture `fixtures/events/`. D-036.
