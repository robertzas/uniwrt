# Testing strategy

| Layer | What | Tooling | Where | When |
|---|---|---|---|---|
| Unit | Pure crates (uci, render, model, rf, topo, isp) | cargo nextest, proptest | `crates/*/src` | every commit |
| Golden | Rendered UCI per role × feature × capability set | insta snapshots | `crates/render/tests` | every commit |
| Agent | The bundle's install, the ops in both loop modes, collect sets parsed into facts, the pushes (ticks, both log modes, netifd's events); the engine watching the node in both loop modes; adoption (only `authorized_keys` changes); transactions and rollback from M3 | OpenWrt rootfs containers booted with procd, so ubus, netifd, logd and dropbear are real; NET_ADMIN lets netifd configure the container (`just test-agent`, the CI job `agent`) | `crates/node/tests/agent.rs`, `crates/engine/tests/agent.rs`, `crates/jobs/tests/agent.rs` | every commit |
| Wi-Fi lab | The Wi-Fi and mesh sets against real hostapd, wpa_supplicant, nl80211, netifd, dnsmasq, odhcpd and batman-adv; the pushes of hostapd and dnsmasq as the client is kicked and its radio restarts; the tests run one at a time | mac80211_hwsim radios and batman-adv from the host's kernel, in OpenWrt 25.12 and 24.10 containers booted with procd: a gateway, an AP meshed with it over Wi-Fi and a veth backhaul, and a client of both (`scripts/wifi-lab.sh`, `just test-wifi`; local only, D-035) | `crates/node/tests/wifi.rs` | WPs that touch the Wi-Fi or mesh sets |
| Transport | SSH exec/stream/upload/jump, links (reconnect, quarantine, agent restarts) | an in-process russh server on loopback that runs commands with the local `sh`; dropbear in OpenWrt rootfs containers (`just test-dropbear`, the CI job `transport`) | `crates/node/tests` | every commit |
| Sim | The simulated world's dynamics and determinism; every set and push it renders, through the node crate's parsers | cargo nextest; the engine over the sim fleet on tokio's paused clock | `crates/sim/src/*/tests.rs`, `crates/sim/tests/engine.rs` | every commit |
| Engine/API | Telemetry ingest, status, intervals, roams, jobs, routes | a scripted link on tokio's paused clock (`crates/engine/src/testing.rs`), the sim fleet, axum test client | `crates/engine/src/actor/tests.rs`, `crates/*/tests` | every commit |
| UI | Components with logic, stores | Vitest + Testing Library | `web/src/**/*.test.tsx` | every commit |
| E2E | User flows | Playwright against `uniwrt serve --demo --demo-seed 42` | `web/e2e` | `main` + nightly |
| System | Rendered configs accepted by real OpenWrt; netifd, fw4 and Wi-Fi come up (mac80211_hwsim) | OpenWrt x86-64 images (24.10, 25.12) under QEMU | `tests/system` | nightly, and before releasing config features |
| Hardware | The owner's real network | Manual checklist at the end of each milestone | `docs/testplans/` | milestone end |
| Performance | Ingest/render benchmarks; RSS with a sim fleet of 50 nodes / 1,000 clients | criterion, sim | `benches/` | performance WPs, M9.2 |

## Fixtures

- Real OpenWrt configs and agent outputs, collected per device family and OpenWrt version under `testdata/openwrt/<version>/<device>/`.
- They are collected from the owner's nodes in observe mode (with consent) and anonymized by a script that rewrites MACs, SSIDs, keys and public IPs.
- Agent output lives in `crates/node/fixtures/collect/*.json`: collect answers captured from the rootfs containers (sanitized: the host's model, CPU and kernel), and `gateway.json`, the raw output behind the canonical model fixture. Parsing `gateway.json` must give exactly that fixture's board, system, capabilities, interfaces and devices. To capture, mount `node-agent/` (with a `VERSION` file) at `/tmp/uniwrt/agent` in a container booted with procd, and pipe requests into `docker exec -i <container> ucode /tmp/uniwrt/agent/main.uc`.
- `hwsim-<role>-<release>.json` are collect answers from the hwsim lab's gateway (a) and AP (b), sanitized the same way plus the client's hostname (its container ID); `hwsim-a-<release>-neighbors.json` holds the neighbors set with and without rtnetlink, taken one after the other. `scripts/wifi-lab.sh capture` rewrites them from a running lab; `facts/tests/hwsim.rs` checks them against the lab's topology.
- `crates/node/fixtures/events/hwsim-a-<release>.jsonl` is what the lab gateway's agent pushed, line by line, while the client was kicked, loopback restarted and a radio restarted, with the log polled, then streamed (`scripts/wifi-lab.sh capture events`); `events/tests.rs` parses every push in it.
- Canonical model fixtures, written from the specs, live in `crates/model/fixtures/facts/*.json`. Each must round-trip through `NodeFacts` without losing a field, and they double as wire-format examples for the simulator and the UI.

## Rules

- Every bug fix comes with a regression test.
- Review snapshot changes; never accept them blindly.
- Tests reach no network beyond loopback. Randomness that a test depends on uses fixed seeds.
- A test that needs real hardware gets a sim or fixture equivalent, so CI covers it.
- Tests against dropbear run at most two at a time: dropbear drops a client's sixth concurrent unauthenticated connection (`MAX_UNAUTH_PER_IP`), and all tests connect from one address.
