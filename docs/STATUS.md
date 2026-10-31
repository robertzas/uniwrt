# Status

**Milestone:** M2 — Observability (M1 is done; the owner's acceptance of M1.7b/M1.8b waits for their hardware)
**Next WP:** M2.3 — Clients (M; read ROADMAP M2; docs/features/observability.md "Clients"; docs/features/ui.md "Information architecture" (client page tabs) and "Frontend structure"; docs/dev/CONVENTIONS.md "TypeScript / Solid" and "Feature recipe"). Inventory, OUI vendor, device icons, history and roams, experience score v1, actions (rename, kick, block). Roams are inferred already (`Event::Roam`, `crates/engine/src/roam.rs`); the topology has every client as `client:<mac>` with its AP, band, signal and traffic (`GET v1/topology`), which the inventory can start from. Names come from DHCP leases. `just new-feature clients --path /clients` scaffolds the page. Kick and block change nodes: they need management mode (directive 8) and M3's transactions, so they may wait or stay read-only (ask the owner).
**Last done:** M2.2 — the topology view (2026-10-30)

## Open questions for the owner

- Router models and OpenWrt versions of the gateway and the 3 nodes, and the mesh/backhaul setup (batman? 802.11s? wired?). Captures from them would complement the hwsim fixtures.
- Open options O-1 … O-4 and O-6 … O-9 in `docs/DECISIONS.md` (O-5 became D-046). Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust from rustup (D-026); cargo-nextest and cargo-deny in `~/.cargo/bin`. Installed: `just`, bun, node 26, Docker 29 (buildx, containerd store), Chrome (`/usr/bin/google-chrome-stable`, which Playwright uses), python3, openssl. Not installed: shellcheck (CI runs it; `docker run --rm --network none -v "$PWD:/mnt" -w /mnt koalaman/shellcheck:stable scripts/*.sh` works), qemu binfmt, `bc`. Scripts in the scratchpad (under /tmp) must run through `bash`: it is mounted noexec.
- The tool shell is zsh: quote globs; keep job control in bash scripts. Name probe containers and stop them by name. Never `pkill -f` a pattern that the running shell's own command line contains (it kills the shell): stop a demo server with `for p in $(pgrep -x uniwrt); do kill $p; done`. OpenWrt containers boot with `docker run --cap-add NET_ADMIN <image> /sbin/init`. If containers fail on docker0, `sudo systemctl restart docker`.
- The kernel has mac80211_hwsim and batman-adv; `scripts/wifi-lab.sh` runs the lab. CI runs it too (job `wifi`, D-043). docker0 vanishes after lab runs; only the owner can restart Docker.
- `gh` is authenticated as `robertzas`. GHCR `ghcr.io/robertzas/uniwrt` is public.
- Web on Solid 2 RC (rc.20, D-045, D-049): raise the RC set by hand. In rc.16 `reconcile(…, key)` on an array replaces items (D-047): merge rows in place. Store writes in a component body or `createRoot` body throw (owned scope); tests create sockets before `render`, and a lazy page's test sends its topic's snapshot only once the page has subscribed (`ws.sent` holds the `sub`). Pending async data on a page (a solid-query that is often in flight) holds navigations away from it: poll slow endpoints into a signal (D-048). Kobalte dialogs call `onOpenChange(false)` when they unmount. Reading props in `onSettled` or an effect's apply phase warns `STRICT_READ_UNTRACKED`: keep the values the effects were handed in plain variables (`TopologyCanvas`). `createMemo(fn, { equals })` takes two arguments.
- Visual checks: `just e2e` builds the debug binary with the UI; for screenshots run that binary with `serve --demo --demo-seed 42 --http-listen 127.0.0.1:18080 --https-listen 127.0.0.1:18443 --data-dir <tmp>` in the background and drive Chrome with a script in `web/` importing `@playwright/test` (`ignoreHTTPSErrors`, `channel: "chrome"`); wait for the Devices table after signing in before navigating. The dev gallery is at `/dev/ui?skin=…&theme=…` under `vite`.
- Cargo's sparse index can lag: `cargo info <crate>` refreshes it.
- Follow-up: CI's vitest logs Solid dev diagnostics that a local run doesn't (since rc.16 at least): `STRICT_READ_UNTRACKED` in ThemeToggle's SegmentedControl (Kobalte's SegmentedControl reads its props in its body: ViewSwitch logs it locally too, though both stay checked correctly), `createGatewayLeases.gateway`, a `<Show>` condition and effect callbacks, and `ASYNC_OUTSIDE_LOADING_BOUNDARY`. Each may be UI that doesn't update; find why they're CI-only, then fix them.
- e2e: the adoption test flaked once locally (the fingerprint step, line 77); CI retries once.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-30 — M2.2 done, unsplit (owner): Canvas renderer (tokens, Lucide paths, flows, status/CPU rings, pan/zoom/pinch, keyboard), d3-force in a worker, tree/RF/clients views with client groups, details panel, pins kept on the controller (owner), 24 h replay from a recorded history (hourly keyframes, 30 s deltas). D-052. e2e covers it. tokio-tungstenite 0.30 still held (axum 0.8.9).
- 2026-10-30 — M2.1 done: pure `topo` crate (WAN, FDB segments and inferred switches, 802.11s/batman mesh, Wi-Fi, translation tables, neighbor-cache fallback; tree by cheapest path; rates; diff/apply), rebuilt by the engine 1 s after changes (`Engine::add` takes an `Identity`), `GET v1/topology` and WS `topology`. D-051.
- 2026-10-30 — M1.9 done: `just arch-check` and `just new-feature` (tested in CI); Dialog and Drawer return focus to their opener. D-050. Before it, the owner's version sweep with no cooldown (D-049).
- 2026-10-30 — M1.8b done: wizard dialog over Devices (`?adopt=1`), Discovered card and batch dialog, failures acted on by code; the demo leaves its last AP unadopted and adopts it through the real job over `DemoDial` (owner's choices). D-048.
- 2026-10-30 — M1.8c done: sign-in gate, live socket and stores, Devices list and overview, ⌘K devices, Playwright e2e job gating the image. D-047.
