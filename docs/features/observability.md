# Observability & troubleshooting

Read this when working on telemetry, topology, clients, metrics, logs, events/alerts, the dashboard or diagnostics.
Milestones: M1.4 (telemetry), M2.1–M2.7, M8.4–M8.5.

## Telemetry pipeline

The node agent writes JSON lines over SSH → the `node` crate parses them into `model::facts` → the engine updates live state in memory → the event bus feeds:
- WebSocket topics (deltas to the UI)
- the batched metrics writer
- the event and alert evaluators
- periodic snapshots to SQLite

**Intervals:**
- 15 s base.
- 2 s for entities currently open in the UI (driven by subscriptions).
- Push events go out immediately: association, disassociation and roams; link up/down; DHCP leases; DFS; netifd ifup/ifdown.

**Collectors** (one small module each, in `node-agent/collect/`):
- system: uptime, load, memory, CPU %, temperature, flash
- board and release
- interfaces and devices: counters, carrier, link speed
- radios: channel, width, tx power, noise, survey
- stations per BSS: MAC, signal, SNR, rx/tx rate, MCS/NSS/width, airtime, inactivity, capabilities
- DHCP leases (gateway), ARP/NDP neighbors, bridge FDB
- batman: o, n, tg, gwl, s
- WAN: addresses, routes, delegated prefix
- conntrack count
- per-host usage from nlbwmon (gateway, if installed)
- LLDP neighbors (if lldpd is installed)

## Topology [M2.1–M2.2]

The graph builder (`topo` crate, pure) works on these elements:
- **Nodes:** the internet, the gateway, APs, inferred unmanaged switches, and clients.
- **Wired edges:** from LLDP, from the bridge-FDB port where a node's MAC was learned, or from a batman neighbor heard on an Ethernet hard interface. When several node or client MACs share one port, an unmanaged switch is inferred there.
- **Mesh edges:** batman neighbors on mesh hard interfaces, plus 802.11s station stats.
- **Client edges:** the association list; the FDB port plus the batman translation table; names and IPs from DHCP.

**Views:**
- **Tree:** rooted at the gateway, following batman best paths.
- **RF/mesh:** every mesh link, colored by quality.
- **Clients:** APs with client clusters that expand and collapse.

**Live behavior:**
- Edge width shows throughput and color shows quality, with animated flow particles.
- Rings around each node show health and CPU.
- Click anything for a details panel; drag to pin (positions are saved).
- A replay slider covers the last 24 h from snapshots, answering "what did the network look like when it broke?"

**Rendering:** Canvas 2D (WebGL above about 2,000 entities), with the d3-force layout running in a Web Worker.

## Clients [M2.3]

The inventory is keyed by MAC. Randomized MACs are flagged and grouped by hostname or DHCP client ID where possible.

**Fields:**
- name/alias
- vendor (from an embedded compact OUI table)
- device-type icon (rules over DHCP vendor class, hostname, OUI and mDNS)
- IPv4/IPv6 addresses and network
- connection: node, radio/band, SSID, signal, rates, Wi-Fi generation, k/v/r support; or the node port for wired clients
- first and last seen
- traffic
- roaming history (sessions → timeline swimlanes)
- experience score

**Actions:** rename, fix IP (DHCP reservation), block/unblock, kick (deauth), Wake-on-LAN, ping from its AP, open Connection Doctor.

**Experience score (0–100):** a weighted mix of signal/SNR, PHY rate against capability, retry/failure rate, radio utilization, roaming health (sticky or flapping clients) and optional latency measured from the AP. It is aggregated per AP and for the whole site, and the breakdown is always shown.

## Metrics [M2.4]

- **Storage** (`store`, SQLite): series keyed by (entity, metric). Raw samples for 48 h, 1-minute rollups for 14 d, 1-hour rollups for 400 d (min/avg/max/last). Writes are batched and retention runs as a job.
- **Queries:** the server downsamples with LTTB to the requested pixel width.
- **Charts:** uPlot. Standard panels cover node CPU/memory/load/temperature, interface throughput, radio utilization/noise/clients, client signal/rate/throughput, WAN latency/jitter/loss/throughput and mesh link quality.
- A Prometheus `/metrics` exporter is in the backlog.

## Logs [M2.5]

**Ingest:**
- OpenWrt logd remote syslog (`log_ip`, `log_port`, `log_proto tcp`), enabled when a node becomes managed, or with consent in observe mode.
- Zero-change alternative for observe mode: `logread -f` over the agent channel. `logread` also backfills on connect.

**Receiver:** RFC 3164/5424 over UDP and TCP (octet-counted or newline-framed) on port 5514. It parses host, program, PID, severity and facility, then maps the host to a node.

**Storage:** an SQLite FTS5 table plus metadata. Retention is by age and size (default 14 days / 1 GB).

**UI:**
- live tail (pause, resume, follow)
- filters (node, program, severity, time) and full-text search
- a severity histogram and saved searches
- jump to context: ± N lines across all nodes
- correlation: click an event to see the logs around it

**Annotations:** `log-patterns.toml`, embedded in the crate that annotates logs (D-025), maps a regex to a meaning, severity, hint and link. Examples: hostapd deauth reasons, DFS radar, netifd link flaps, dnsmasq DHCP pool exhaustion, kernel OOM, mt76 firmware errors.

## Events, alerts, notifications [M2.6]

**Events:**
- nodes: online/offline/reboot, mesh link degraded, low flash
- config: applied or rolled back; upgrade steps
- clients: join/leave/roam/blocked; new device on the network
- WAN: down/up, IP change
- Wi-Fi: DFS radar, high utilization
- DHCP pool exhaustion

**Alerts:** built-in default rules plus user rules (threshold, duration, condition), with dedup, cool-down and maintenance windows (alerts are silenced while jobs run).

**Notifiers:** in-app, browser push (PWA), email (SMTP), ntfy, Gotify, generic webhooks (Discord/Slack/Teams/Home Assistant templates), Pushover, Telegram.

## Dashboard [M2.4, extended in M2.7]

- a health header: internet, mesh, Wi-Fi, nodes, clients
- a WAN card: status, ISP, IPs, latency sparkline, throughput
- a topology mini-map
- top clients, alerts, experience score
- an RF summary and recent events

## Diagnostics toolkit [M8.4]

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

## Troubleshooting assistants [M8.5]

- **Connection Doctor** (per client): one timeline merging association, roam, DHCP, DNS and log events, plus signal history and the experience breakdown. It flags detected issues (weak signal, sticky client, DHCP or DNS failures, band problems, 802.11r incompatibility) and suggests fixes.
- **WAN quality monitor:** continuous ICMP/DNS/HTTP probes from the gateway to several targets, smokeping-style latency bands, and outage detection with evidence of where the fault lies (gateway, ONT or ISP hop).
- **Support bundle:** one click collects configs (secrets redacted), facts, and recent logs and events from every node into a tar.gz.
- An opt-in AI assistant that explains incidents is open option O-7.
