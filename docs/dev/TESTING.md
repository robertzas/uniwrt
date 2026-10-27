# Testing strategy

| Layer | What | Tooling | Where | When |
|---|---|---|---|---|
| Unit | Pure crates (uci, render, model, rf, topo, isp) | cargo nextest, proptest | `crates/*/src` | every commit |
| Golden | Rendered UCI per role × feature × capability set | insta snapshots | `crates/render/tests` | every commit |
| Agent | ucode collectors, transaction/rollback, protocol | OpenWrt rootfs containers (real ucode/uci; ubus where it can run) | `node-agent/tests` | every commit (M1.3+) |
| Transport | SSH exec/stream/upload/jump, links (reconnect, quarantine, agent restarts) | an in-process russh server on loopback that runs commands with the local `sh`; dropbear in OpenWrt rootfs containers (`just test-dropbear`, the CI job `transport`) | `crates/node/tests` | every commit |
| Engine/API | Telemetry ingest, jobs, routes | sim fleet, axum test client | `crates/*/tests` | every commit |
| UI | Components with logic, stores | Vitest + Testing Library | `web/src/**/*.test.tsx` | every commit |
| E2E | User flows | Playwright against `uniwrt serve --demo --seed 42` | `web/e2e` | `main` + nightly |
| System | Rendered configs accepted by real OpenWrt; netifd, fw4 and Wi-Fi come up (mac80211_hwsim) | OpenWrt x86-64 images (24.10, 25.12) under QEMU | `tests/system` | nightly, and before releasing config features |
| Hardware | The owner's real network | Manual checklist at the end of each milestone | `docs/testplans/` | milestone end |
| Performance | Ingest/render benchmarks; RSS with a sim fleet of 50 nodes / 1,000 clients | criterion, sim | `benches/` | performance WPs, M9.2 |

## Fixtures

- Real OpenWrt configs and agent outputs, collected per device family and OpenWrt version under `testdata/openwrt/<version>/<device>/`.
- They are collected from the owner's nodes in observe mode (with consent) and anonymized by a script that rewrites MACs, SSIDs, keys and public IPs.
- Canonical model fixtures, written from the specs, live in `crates/model/fixtures/facts/*.json`. Each must round-trip through `NodeFacts` without losing a field, and they double as wire-format examples for the simulator and the UI.

## Rules

- Every bug fix comes with a regression test.
- Review snapshot changes; never accept them blindly.
- Tests reach no network beyond loopback. Randomness that a test depends on uses fixed seeds.
- A test that needs real hardware gets a sim or fixture equivalent, so CI covers it.
- Tests against dropbear run at most two at a time: dropbear drops a client's sixth concurrent unauthenticated connection (`MAX_UNAUTH_PER_IP`), and all tests connect from one address.
