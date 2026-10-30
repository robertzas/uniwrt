# Status

**Milestone:** M1 — Core engine and read-only adoption
**Next WP:** M1.6b — api crate, auth and sessions
**Last done:** M1.6 — vault and TLS (2026-10-30)

## Start here (next session)

1. Read `docs/ROADMAP.md` §M1 (M1.6b), D-017 and D-039 in `docs/DECISIONS.md`, ARCHITECTURE.md §API and §Crates and layering, `docs/features/security.md` (authentication and sessions, headers, threat model), and the `//!` contracts of `crates/server` (main.rs) and `crates/store` (lib.rs).
2. M1.6b creates the `api` crate (`just crate-age` for new crates): move `crates/server/src/http/` (routes, `problem.rs`, `security.rs`, `plain.rs`, `tls.rs`) there, keeping the embedded UI, TLS and the composition root in `server`. Then migrations for `users`/`sessions`, first-run admin setup, login and sessions with Argon2id (argon2 0.6 is already in the tree through ssh-key), CSRF (SameSite=Strict, an Origin check, a custom header on mutating requests), login rate limiting and lockout, and the audit of logins.
3. To build on: both listeners serve with `ConnectInfo<Peer>` (`tls/listener.rs`); requests from `--trusted-proxies` reach the app over plain HTTP, so cookies and Origin checks must honor `X-Forwarded-Proto`/`-For` only from them. Secrets go through `uniwrt_store::Vault` (`VaultRepo`); `Store` no longer implements it.

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
- M1.6: first start (master key, SSH key, CA and leaf) takes about 5 ms in a debug build; the server crate's 65 tests take 0.2 s.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-30 — M1.6 done (split, owner's choice: auth → M1.6b, REST/WS → M1.6c): `uniwrt_store::{Vault, MasterKey}` (XChaCha20-Poly1305, resealing the stub's rows), `server/src/master_key.rs`; `server/src/tls/` (a name-constrained local CA and leaf via rcgen, rustls with a hot-swapped resolver, daily renewal, `TlsListener`/`PlainListener`/`Peer`); `/api/v1/tls`, `/api/v1/tls/ca.crt`; `http/plain.rs` (307 redirect, health, `--trusted-proxies`); flags `--https-listen`, `--tls-names`, `--trusted-proxies`, `--master-key-file`; CI smoke test over HTTPS. D-039.
- 2026-10-30 — M1.5 done: crate `uniwrt-sim` (a seeded world, the agent's raw shapes, `SimLink` and `SimFleet`); `serve --demo [--demo-seed]` with `FleetLink` and a temporary database; `just demo`. D-038.
- 2026-10-28 — M1.4 done: crate `uniwrt-engine` (`Engine`, a supervised actor per node, `Live`/`NodeView`, the bus's `Event`s, `Intervals` with focus, the poller, `RoamTracker`, `Snapshots`); `serve` watches the adopted nodes. D-037.
- 2026-10-28 — M1.3c done: ops `subscribe`/`unsubscribe`, `log_follow` and `set_interval`; `node::events` with `model::event` and `model::log`; the lab capture `fixtures/events/`. D-036.
- 2026-10-27 — M1.3b done: sets phys, radios, stations, leases, neighbors and batman, with model types and parsers; the hwsim lab (`just test-wifi`). D-035.
