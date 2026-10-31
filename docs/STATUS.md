# Status

**Milestone:** M1 — Core engine and read-only adoption
**Next WP:** M1.9 — Dev accelerators (read ROADMAP M1; ARCHITECTURE.md "Crates and layering"; docs/dev/CONVENTIONS.md "feature recipe"; justfile)
**Last done:** M1.8b — adoption wizard, Discovered card and batch adoption, demo adoption over `DemoDial` (2026-10-30)

## Open questions for the owner

- Router models and OpenWrt versions of the gateway and the 3 nodes, and the mesh/backhaul setup (batman? 802.11s? wired?). Captures from them would complement the hwsim fixtures.
- Open options O-1 … O-4 and O-6 … O-9 in `docs/DECISIONS.md` (O-5 became D-046). Defaults apply until the owner decides.

## Environment notes (owner's machine)

- Rust from rustup (D-026); cargo-nextest and cargo-deny in `~/.cargo/bin`. Installed: `just`, bun, node 26, Docker 29 (buildx, containerd store), Chrome (`/usr/bin/google-chrome-stable`, which Playwright uses), python3, openssl. Not installed: shellcheck (CI runs it), qemu binfmt, `bc`.
- The tool shell is zsh: quote globs; keep job control in bash scripts. Name probe containers and stop them by name. Never `pkill -f` a pattern that the running shell's own command line contains (it kills the shell). OpenWrt containers boot with `docker run --cap-add NET_ADMIN <image> /sbin/init`. If containers fail on docker0, `sudo systemctl restart docker`.
- The kernel has mac80211_hwsim and batman-adv; `scripts/wifi-lab.sh` runs the lab. CI runs it too (job `wifi`, D-043). docker0 vanishes after lab runs; only the owner can restart Docker.
- `gh` is authenticated as `robertzas`. GHCR `ghcr.io/robertzas/uniwrt` is public.
- Web on Solid 2 RC (D-045): raise the RC set by hand. In rc.16 `reconcile(…, key)` on an array replaces items (D-047): merge rows in place. Store writes in a component body or `createRoot` body throw (owned scope); tests create sockets before `render`. Pending async data on a page (a solid-query that is often in flight) holds navigations away from it: poll slow endpoints into a signal (D-048). Kobalte dialogs call `onOpenChange(false)` when they unmount.
- Visual checks: `just e2e` builds the debug binary with the UI; for screenshots run that binary with `serve --demo --http-listen 127.0.0.1:18080 --https-listen 127.0.0.1:18443` in the background and drive Chrome with a script in `web/` importing `@playwright/test` (`ignoreHTTPSErrors`, `channel: "chrome"`). The dev gallery is at `/dev/ui?skin=…&theme=…` under `vite`.
- Cargo's sparse index can lag: `cargo info <crate>` refreshes it.

## Session log (newest first; keep ≤ 5 entries)

- 2026-10-30 — M1.8b done: wizard dialog over Devices (`?adopt=1`), Discovered card and batch dialog, failures acted on by code; the demo leaves its last AP unadopted and adopts it through the real job over `DemoDial` (owner's choices). D-048. M1 is complete but M1.9.
- 2026-10-30 — M1.8c done: sign-in gate, live socket and stores, Devices list and overview, ⌘K devices, Playwright e2e job gating the image. D-047.
- 2026-10-30 — M1.8 done: Solid 2 RC (router 2, query 6, Kobalte 2 patched; D-045), three skins chosen per browser (D-046), primitives, shell, ⌘K, gallery. Split: M1.8c (sign-in, live devices).
- 2026-10-30 — M1.7b done: `GET v1/discovered` (leases, LAN neighbors and batman, probed by SSH banner and host key), `POST v1/adopt/batch`, at most 4 adoptions at once. D-044.
- 2026-10-30 — CI runs every suite (D-043): the hwsim lab as job `wifi`, `ui` apart from `web`, Rust caches saved from main only and shared by the container jobs.
