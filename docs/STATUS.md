# Status

**Milestone:** M3 — Config engine (M2 is done; the owner's acceptance of M1.7b/M1.8b waits for their hardware)
**Next WP:** M3.4a2b — end-to-end applies (M3.4's parts: a1 ✅ | a2a ✅ | a2b | b; D-069, D-070). Read ROADMAP M3 (the a2b line); docs/features/lifecycle.md (Render → diff → apply); D-070; the contracts of `crates/jobs` (`apply/`: `survey.rs` logs in and plans, `run.rs` the phases, `tests/fleet.rs` the scripted fleet) and `crates/system` (`docker.rs`: `Node` over `docker exec`; `scripts/test-system.sh` the topology). Build a `docker exec` `Dial` (exec with stdin; collect by running the working tree's agent, as `Node::facts` does), install the footprint in the containers, and drive `Jobs::apply` there: a confirmed apply, a forced health failure, a lost node rolling back at its deadline. Then flip the ⚠️ in docs/openwrt/REFERENCE.md (CLI: `nslookup`, `init.d … enabled`).
**Last done:** M3.4a2a — the apply job and its API (2026-11-04; D-070): `crates/jobs/src/apply/` (survey, plan, order, health, address, run), `Jobs::plan`/`Jobs::apply`, `Watch::moved`; `uniwrt_node::txn::current` (one-command reads of packages and services); `uniwrt_sim::txn::Tool` (the tool's model, in the demo and the jobs' tests); `model::apply` (`ApplyPlan`, `AppliedRevision`), `Job.revision` and `Job.nodes`, new steps and error codes; migration 0014 (`jobs.revision_id`, `jobs.nodes`, `node_applied`); `GET v1/apply/plan`, `POST v1/apply`, `GET v1/apply/applied` (admins; audited).

## Open questions for the owner

- Router models and OpenWrt versions of the gateway and the 3 nodes, and the mesh/backhaul setup (batman? 802.11s? wired?). Captures from them would complement the hwsim fixtures.
- Open options O-1 … O-4 and O-6 … O-9 in `docs/DECISIONS.md` (O-5 became D-046). Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust from rustup (D-026); cargo-nextest and cargo-deny in `~/.cargo/bin`. Installed: `just`, bun, node 26, Docker 29 (buildx, containerd store), Chrome (`/usr/bin/google-chrome-stable`, which Playwright uses), python3, openssl. Not installed: shellcheck (CI runs it; `docker run --rm --network none -v "$PWD:/mnt" -w /mnt koalaman/shellcheck:stable scripts/*.sh` works), qemu binfmt, `bc`. Scripts in the scratchpad (under /tmp) must run through `bash`: it is mounted noexec.
- The tool shell is zsh: quote globs; keep job control in bash scripts. Name probe containers and stop them by name. Never `pkill -f` a pattern that the running shell's own command line contains (it kills the shell): run a demo server as a background task and stop it with TaskStop, or `kill <pid>`. OpenWrt containers boot with `docker run --cap-add NET_ADMIN <image> /sbin/init`. If containers fail on docker0, `sudo systemctl restart docker`.
- The running kernel (7.3.0-4) lost its module tree to the 7.3.1 upgrade: until the owner reboots, 8021q, pppoe, nftables' expression modules, mac80211_hwsim and batman-adv can't load, Docker can't map ports, so `just test-system` skips its VLAN, PPPoE and firewall checks, and `just test-wifi` and `just test-agent` can't run; CI runs them. Container-only work iterates on a temporary branch (CI runs on every branch; delete it after landing). docker0 vanishes after lab runs; only the owner can restart Docker. A `--network none` container booted with `/sbin/init` still works through `docker exec` (BusyBox has `pgrep` but no `pkill`).
- `gh` is authenticated as `robertzas`. GHCR `ghcr.io/robertzas/uniwrt` is public.
- Web on Solid 2 RC (rc.20, D-045, D-049): see D-047/D-048 for the RC's traps (reconcile replaces items; store writes in owned scopes throw; pending async holds navigations; constant `style={{…}}` is folded into a CSP-blocked attribute). A component that reads an endpoint needs a stub in every test that renders it.
- Visual checks: `just e2e` builds the debug binary with the UI; for screenshots run it with `serve --demo --demo-seed 42 --http-listen 127.0.0.1:18080 --https-listen 127.0.0.1:18443 --data-dir <tmp>` in the background and drive Chrome from a script in `web/` (`ignoreHTTPSErrors`, `channel: "chrome"`). API probes: `curl -k -c jar … /api/v1/auth/login` with `x-uniwrt-csrf: 1`.
- Cargo's sparse index can lag: `cargo info <crate>` refreshes it. `just gen` exports only types reachable from a `ts(export)`: model types returned directly need `ts(export)` themselves (`ApplyPlan`).

## Follow-ups

- Demo mode: the site's defaults (10.0.0.0/24) differ from the sim's 192.168.1.0/24 and nodes manage no packages by default, so a demo apply does nothing (or would renumber): M3.4b seeds the demo's first revision from the sim and lets users choose managed packages.
- CI's vitest logs Solid dev diagnostics a local run doesn't (`STRICT_READ_UNTRACKED` in ThemeToggle's SegmentedControl, `createGatewayLeases.gateway`, `ASYNC_OUTSIDE_LOADING_BOUNDARY`): find why, then fix.
- e2e flakes: the adoption test's fingerprint step, and the events test's 30 s wait for a roam (CI's serial retry can't help: the rerun counts 4 devices). Wait for a roam the demo is sure to make; let the first test tolerate an adopted device.
- `uniwrt_notify::http` could word transport errors for people (refused, DNS, TLS, timeout).
- Dependabot #10 (tokio-tungstenite 0.30) fails CI: axum 0.8.9 still brings 0.29.

## Session log (newest first; keep ≤ 5 entries)

- 2026-11-04 — M3.4a2a done (owner: split a2a | a2b, health relative to a baseline, follow renumbered nodes; D-070). Render refuses a gateway without a WAN port in board.json even when it manages nothing, and a node's `managed` set is empty by default: tests give the container fixture a WAN (`eth1`) and manage every package. A failed job already raises the `job-failed` alert, so a partial apply alerts without new rules. Store writes run in a savepoint each: one op is all or nothing without `conn.transaction()`.
- 2026-11-04 — M3.4a1 done (D-069). A rollback stays `rolling_back` until its reload ends; logd starts at S12 (guard at S13); `fw4 check` reads only `/etc/config`; `reload_config` signals only packages already in its md5 file (a package new to the node gets no reload trigger).
- 2026-11-03 — M3.3b done (D-068). A stock AP's dnsmasq makes the gateway's refuse to start (`force '1'`); procd's reload triggers fire ~1 s after `reload_config` (stop services first); checks after apply must wait for the new values. Run `just map` after editing a script's header. A push to a branch cancels its running CI.
- 2026-11-03 — M3.3a done (D-067). Stock 24.10/25.12 differ in odhcpd paths and `timezone`: render carries the node's values over. New insta snapshots: `INSTA_UPDATE=new`, reviewed by hand.
- 2026-11-03 — M3.2 done. libuci's source settled the parser's rules; a booted rootfs container with `--network none` gives real configs without docker0.
