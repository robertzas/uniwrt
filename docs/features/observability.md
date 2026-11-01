# Observability & troubleshooting

Read this when working on telemetry, topology, clients, metrics, logs, events/alerts, the dashboard or diagnostics.
Milestones: M1.4 (telemetry), M2.1–M2.6, M8.3.

## Telemetry pipeline

The node agent writes JSON lines over SSH → the `node` crate parses them into `model::facts` → the engine updates live state in memory → the event bus feeds:
- WebSocket topics (deltas to the UI)
- the batched metrics writer
- the event and alert evaluators
- snapshots to SQLite: each node's latest facts, every 5 min when they changed and at shutdown (D-037)

**Intervals** (D-037; `Intervals` in `crates/engine/src/schedule.rs`):
- 15 s base for system, devices, interfaces, radios and stations; 30 s for neighbors and batman; 60 s for leases; 1 h for capabilities and phys; board only when the agent starts.
- 2 s for system, devices, radios and stations, and 10 s for batman, while the UI has the node open (`Engine::focus`, driven by subscriptions).
- Push events go out immediately (D-036): hostapd's association steps and disassociations, netifd's interface and device changes (links up and down), and DHCP acks and releases. The engine collects the set an event touches again 500 ms later (the events in that time share one collect), and infers roams: a station authorized at another AP while it is still connected to the first, or within 15 s of leaving it. hostapd's DFS radar notifications are forwarded but not parsed yet.
- An agent without an event loop is polled on the same intervals.
- Telemetry that stays away for three of its fastest intervals (at least 30 s) makes the node degraded, and the engine asks the agent for its pushes again.

**Collectors** (one small module each, in `node-agent/collect/`, parsed by `crates/node/src/facts/`). The first five shipped with M1.3, phys through batman with M1.3b (D-035); the rest come later:
- system: uptime, load, memory, CPU %, temperature, flash
- board and release
- capabilities: package manager, optional software, hostapd and wpa_supplicant build features, switch kind
- interfaces (netifd) and devices (sysfs): addresses, gateways, counters, carrier, link speed
- phys (nl80211, collected rarely): per band, the standards, widths, spatial streams, and channels with their DFS state and power limits
- radios (netifd and nl80211): configuration, channel, width, center frequency, tx power, noise, and the busy share from the survey
- stations per interface (nl80211 and hostapd): MAC, signal, rx/tx rate (MCS/NSS/width/GI), counters, retries, airtime, inactivity, expected throughput, capabilities (11k, 11v, 11w, WMM), and a mesh peer's link state; SNR is the signal over the radio's noise
- DHCP leases (dnsmasq, odhcpd), ARP/NDP neighbors and the bridge FDB (rtnetlink, or /proc and `brforward`)
- batman (batctl's JSON queries): the mesh, hard interfaces, originators, neighbors, gateways and both translation tables; the `batctl s` counters later
- WAN: addresses, routes, delegated prefix
- conntrack count
- per-host usage from nlbwmon (gateway, if installed)
- LLDP neighbors (if lldpd is installed)

## Topology [M2.1–M2.2]

The graph builder (`topo` crate, pure; D-051) works on these elements:
- **Nodes:** the internet, the gateway, APs and bridges, inferred unmanaged switches, mesh peers that are not adopted, and clients. Ids are stable across builds: `internet`, `node:<id>`, `switch:<node id>:<port>`, `peer:<mac>`, `client:<mac>`.
- **WAN edges:** the gateway's interfaces with a default route, or named `wan*`, one edge per device beneath them (`wan` and `wan6` on `eth1` are one link).
- **Wired edges:** from the bridge-FDB port where a node's MAC was learned, or from a batman neighbor heard on an Ethernet hard interface (LLDP once its collector exists). Of what a port sees, whatever another node there has on a port other than its port toward this node is behind that node; the rest share the port's segment. Segments that name each other are one: two ends make a cable, more an unmanaged switch. Wi-Fi and mesh clients don't count, so an AP's clients never make a switch.
- **Mesh edges:** 802.11s peer links (established ones, and the others to adopted nodes, shown down) and batman neighbors on Wi-Fi hard interfaces, merged per pair and band, with the weaker end's signal and TQ.
- **Wi-Fi edges:** each client at the AP it is associated with (the most recently active one while it roams); a bridge at the AP its station interface is associated with.
- **Clients left:** the batman translation tables (behind a peer that is not adopted, or a node without facts), then the neighbor caches off each node's uplink. Names and IPs come from DHCP leases (the gateway's first) and neighbor caches.
- **Per edge:** the ports at both ends, up or down, band and SSID, signal, TQ, capacity (Ethernet speed, expected throughput, batman throughput), quality 0–100 (signal: −90 dBm is 0, −50 dBm is 100; TQ as a share), and traffic from one end's counters with rates against the previous build.
- **Tree:** the cheapest paths from the internet (a gateway that doesn't reach it is a root of its own): batman's best path toward the gateway and the WAN cost 1, Ethernet 2, Wi-Fi 3, other mesh links 6, a link that is down 1,000 more. Edges point downstream (`a` is upstream).

**Delivery:** the engine builds the topology 1 s after a node's facts or status change, `GET /api/v1/topology` returns it, and the WS topic `topology` sends it as a snapshot, then as deltas (the vertices and edges new or changed, and the ids gone) against what the client has.

**Views** (M2.2; `/topology?view=tree|rf|clients`):
- **Tree:** rooted at the internet, following batman best paths (each vertex's `parent`, `uplink` and `hops`), laid out as a tidy tree: the uplinks only, each host's clients grouped into one vertex until expanded.
- **RF/mesh:** the nodes, switches and mesh peers (not the internet, nor clients) and every link between them, mesh links colored by quality and labeled with their signal (or batman-adv's TQ).
- **Clients:** everything, each AP's clients around it; a host's clients collapse into a group and expand again (double-click, or the details panel).

**Live behavior:**
- Edge width shows throughput (log scale) and color shows quality (the link ramp; wired links neutral, links that are down dashed red), with flow particles both ways (more and faster with more traffic; none under reduced motion).
- Rings around each adopted node show its status (green, amber, red) and its CPU (an arc colored by load); clients are dots in their band's color.
- Click a vertex or link for the details panel (status, addresses, uplink, signal, TQ, capacity, traffic; "Open device"); a selection dims all but its neighbours. In the force views (RF, clients), dragging a vertex pins it where it is dropped: the controller keeps the pins (`topology_layout`), one layout for every user and browser, shared by both views, and "Unpin" / "Unpin all" release them. Browsers load the pins when they open the page (not live).
- A replay slider covers the last 24 h, its changes marked, answering "what did the network look like when it broke?" Moving it shows the topology at that time (without traffic or load); "Back to live" returns.
- Keyboard: the canvas takes `+`, `-`, `0` (fit), the arrows (pan) and Escape; a visually hidden list of every vertex selects them for keyboards and screen readers. The view fits itself until the user pans or zooms.

**History:** the engine records each build as `Topology::for_history` keeps it (no traffic or load, signals to 2 dB, quality to 5 points, so noise is no change): a keyframe every hour and deltas between, at most every 30 s (the changes of that time share one), pruned at each keyframe to the last 24 h (`TopologyHistory`, D-052).

**Rendering:** Canvas 2D (WebGL above about 2,000 entities), with the d3-force layout running in a Web Worker (on the main thread where there are no workers). Lucide icons are drawn as paths, colors are read from the design tokens and follow the skin and theme, and the canvas draws only while something moves.

## Clients [M2.3]

The inventory is keyed by MAC and kept by the `clients` crate's book (D-053): every client the adopted nodes saw in the last 30 days (longer with an alias), online or not. A client is a `client:<mac>` vertex of the topology; its edge gives its place (an access point's interface, a node's port, or the port an inferred switch hangs from, or a mesh peer that isn't adopted) and the node's facts its station and radio. It is online while its link is up and that node isn't offline.

**Fields:**
- name: the alias a user gave it (admins rename it; audited), else its DHCP hostname, else its MAC
- vendor, from the embedded IEEE registry (MA-L, MA-M and MA-S; `just oui` rebuilds it), none for randomized addresses
- device kind for the icon (phone, tablet, laptop, desktop, server, watch, TV, speaker, console, printer, camera, storage, smart home, network), from rules over the hostname, then the vendor (`crates/clients/src/rules.rs`); DHCP vendor classes and mDNS will sharpen it once collected
- randomized ("private") addresses, flagged and grouped with the device's others by a hostname that isn't generic, else by a DHCP client ID that isn't the MAC; a group shares an alias
- IPv4/IPv6 addresses
- connection: node, interface or port, SSID, band, channel, signal and noise, rates both ways, Wi-Fi standard, 802.11k/r/v/w and WMM, link capacity, traffic and its rates
- first and last seen
- sessions: one per place, from the association's start (its connected time) to when it was last seen there; moving while connected, or showing up elsewhere within 15 s of leaving, ends one as roamed. An association that began before its session ended carries the session on (a restart, a node that went quiet). Kept 30 days.
- samples for the Signal tab: signal, SNR, rates, throughput and score, at most every 5 s, for an hour, in memory (stored metrics come with M2.4)
- experience score

**Actions:** rename now. Kick (deauth) and block/unblock change the access point, so they wait for management mode and M3's transactions; they come with M5.1 (D-054); the page shows them disabled. Fix IP (DHCP reservation), Wake-on-LAN, ping from its AP and Connection Doctor come with M4 and M8.

**Experience score v1 (0–100):** a weighted mean, its breakdown always shown (`crates/clients/src/score.rs`):

| Factor | Weight | 0 at | 100 at |
|---|---|---|---|
| SNR (else signal) | 35 | 10 dB (−85 dBm) | 40 dB (−55 dBm) |
| PHY rate against the best the client and radio can reach (standard, width, streams), while it moves traffic | 25 | 5 % | 50 % |
| Retries per frame sent between collects (failures count fivefold) | 20 | 40 % | 5 % |
| Channel busy | 10 | 90 % | 40 % |
| Roams in the last hour (flapping) | 10 | 12 | 4 |

A part without data drops out; without signal or rate there is no score. Sticky clients need the inter-node RSSI matrix (M2.6), and latency from the AP needs probes (M8). The Clients page averages the score of online Wi-Fi clients for the site and per access point.

**API:** `GET v1/clients`, `GET v1/clients/{mac}` (the client, its samples and a day of sessions), `GET v1/clients/{mac}/sessions?since=<ms>` and `PATCH v1/clients/{mac}` (`{"alias": …}`); WS topics `clients` (snapshot, then changed clients by MAC and the ones forgotten) and `client:<mac>` (snapshot, then its view, new samples and changed sessions; holding it speeds up the telemetry of the node the client is at, following it as it roams).

## Metrics [M2.4]

- **Storage** (`store`, SQLite): series keyed by (entity, metric). Raw samples for 48 h, 1-minute rollups for 14 d, 1-hour rollups for 400 d (min/avg/max/last). Writes are batched and retention runs as a job.
- **Queries:** the server downsamples with LTTB to the requested pixel width.
- **Charts:** uPlot. Standard panels cover node CPU/memory/load/temperature, interface throughput, radio utilization/noise/clients, client signal/rate/throughput, WAN latency/jitter/loss/throughput and mesh link quality.
- A Prometheus `/metrics` exporter is in the backlog.

## Logs [M2.5]

**Ingest:**
- OpenWrt logd remote syslog (`log_ip`, `log_port`, `log_proto tcp`), enabled when a node becomes managed, or with consent in observe mode.
- Zero-change alternative for observe mode: the agent's `log_follow` (D-036). By default it polls logd by sequence number every second; it can also stream `logread -f`. Either backfills the last N entries on request.

**Receiver:** RFC 3164/5424 over UDP and TCP (octet-counted or newline-framed) on port 5514. It parses host, program, PID, severity and facility, then maps the host to a node.

**Storage:** an SQLite FTS5 table plus metadata. Retention is by age and size (default 14 days / 1 GB).

**UI:**
- live tail (pause, resume, follow)
- filters (node, program, severity, time) and full-text search
- a severity histogram and saved searches
- jump to context: ± N lines across all nodes
- correlation: click an event to see the logs around it

**Annotations:** `log-patterns.toml`, embedded in the crate that annotates logs (D-025), maps a regex to a meaning, severity, hint and link. Examples: hostapd deauth reasons, DFS radar, netifd link flaps, dnsmasq DHCP pool exhaustion, kernel OOM, mt76 firmware errors.

## Events, alerts, notifications [M2.5]

**Events:**
- nodes: online/offline/reboot, mesh link degraded, low flash
- config: applied or rolled back; upgrade steps
- clients: join/leave/roam/blocked; new device on the network
- WAN: down/up, IP change
- Wi-Fi: DFS radar, high utilization
- DHCP pool exhaustion

**Alerts:** built-in default rules plus user rules (threshold, duration, condition), with dedup, cool-down and maintenance windows (alerts are silenced while jobs run).

**Notifiers:** in-app, browser push (PWA), email (SMTP), ntfy, Gotify, generic webhooks (Discord/Slack/Teams/Home Assistant templates), Pushover, Telegram.

## Dashboard [M2.4, extended in M2.6]

- a health header: internet, mesh, Wi-Fi, nodes, clients
- a WAN card: status, ISP, IPs, latency sparkline, throughput
- a topology mini-map
- top clients, alerts, experience score
- an RF summary and recent events

## Diagnostics toolkit [M8.3]

Everything runs from any node and streams live:
- ping, traceroute, DNS lookup (per resolver), HTTP check
- iperf3 node ↔ node (backhaul capacity) and node ↔ controller
- WAN speed test (from the gateway)
- Wi-Fi scan and channel survey
- **packet capture:** tcpdump on the node with a BPF filter, streamed to the browser as a `.pcap` download, with size and time caps
- cable diagnostics (ethtool, where supported)
- ARP/neighbor/route/conntrack tables
- a browser **SSH terminal** (xterm.js over the controller's SSH session; audited)
- a **UCI explorer**: read-only by default; edits go through the change pipeline

## Troubleshooting assistants [M8.3]

- **Connection Doctor** (per client): one timeline merging association, roam, DHCP, DNS and log events, plus signal history and the experience breakdown. It flags detected issues (weak signal, sticky client, DHCP or DNS failures, band problems, 802.11r incompatibility) and suggests fixes.
- **WAN quality monitor:** continuous ICMP/DNS/HTTP probes from the gateway to several targets, smokeping-style latency bands, and outage detection with evidence of where the fault lies (gateway, ONT or ISP hop).
- **Support bundle:** one click collects configs (secrets redacted), facts, and recent logs and events from every node into a tar.gz.
- An opt-in AI assistant that explains incidents is open option O-7.
