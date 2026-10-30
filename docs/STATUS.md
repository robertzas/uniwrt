# Status

**Milestone:** M1 — Core engine and read-only adoption
**Next WP:** M1.8 — App shell & design system v1 (read ROADMAP M1; docs/features/ui.md; DECISIONS O-5; decision gates first: Solid 2.0 readiness, 3 rendered mockups for the owner)
**Last done:** M1.7b — discovery and batch adoption (2026-10-30)

## Open questions for the owner

- Router models and OpenWrt versions of the gateway and the 3 nodes, and the mesh/backhaul setup (batman? 802.11s? wired?). Captures from them would complement the hwsim fixtures.
- Open options O-1 … O-9 in `docs/DECISIONS.md`. Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust from rustup (D-026); cargo-nextest and cargo-deny in `~/.cargo/bin`. Installed: `just`, bun, node 26, Docker 29 (buildx, containerd store), Chrome, python3, openssl. Not installed: shellcheck (CI runs it), qemu binfmt, `bc`.
- The tool shell is zsh: quote globs; keep job control in bash scripts. Name probe containers and stop them by name (`docker rm -f "$var"` inside `sh -c` is refused by the safety check: use literal names). OpenWrt containers boot with `docker run --cap-add NET_ADMIN <image> /sbin/init`. If containers fail on docker0, `sudo systemctl restart docker`.
- The kernel has mac80211_hwsim and batman-adv; `scripts/wifi-lab.sh` runs the lab (privileged containers straight from the shell are refused). CI runs it too (job `wifi`, D-043).
- docker0 vanishes after lab runs (again on 2026-10-30; `sudo systemctl restart docker` fixes it, and only the owner can run it). M1.7b's banner check in `just test-dropbear` was verified in CI only.
- `gh` is authenticated as `robertzas`. GHCR `ghcr.io/robertzas/uniwrt` is public.
- `scripts/crate-age.sh` allows crypto-bigint 0.7.9 and ctutils 0.4.6 (D-033); drop both after 2026-11-01.
- Cargo's sparse index can lag: `cargo info <crate>` refreshes it. Node 26's `WebSocket` takes `{ headers }` for driving `/api/v1/ws` against `serve --demo`.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-30 — M1.7b done: `GET v1/discovered` (leases, LAN neighbors and batman, probed by SSH banner and host key), `POST v1/adopt/batch`, at most 4 adoptions at once. D-044.
- 2026-10-30 — CI runs every suite (D-043): the hwsim lab as job `wifi`, `ui` apart from `web`, Rust caches saved from main only and shared by the container jobs.
- 2026-10-30 — M1.7 done: crate `uniwrt-jobs`, `POST v1/adopt[/probe]`, `GET v1/jobs`, WS `jobs`, backups sealed in the DB. D-042. Split: M1.7b (discovery, batch), M1.8b (wizard).
- 2026-10-30 — M1.6c done: `uniwrt_engine::Fleet`, `api::nodes`, `api::ws`, the `ts` feature; HTTP/2 extended CONNECT for WebSockets. D-041.
- 2026-10-30 — M1.6b done: crate `uniwrt-api`, auth, sessions, CSRF, audit. D-040.
