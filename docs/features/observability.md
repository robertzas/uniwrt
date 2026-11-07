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
- 15 s base for system, devices, interfaces, radios and stations; 30 s for neighbors and batman; 60 s for leases and config (the hashes of `/etc/config`, for drift); 1 h for capabilities and phys; board only when the agent starts.
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
- DHCP leases (dnsmasq, odhcpd) and, in the same set, the gateway's UPnP mappings (miniupnpd's lease file); ARP/NDP neighbors and the bridge FDB (rtnetlink, or /proc and `brforward`)
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

A part without data drops out; without signal or rate there is no score. Sticky clients can now draw on the inter-node RSSI matrix (M2.6, features/wifi.md); scoring them waits for the roaming work (M5.2). Latency from the AP needs probes (M8). The Clients page averages the score of online Wi-Fi clients for the site and per access point.

**API:** `GET v1/clients`, `GET v1/clients/{mac}` (the client, its samples and a day of sessions), `GET v1/clients/{mac}/sessions?since=<ms>` and `PATCH v1/clients/{mac}` (`{"alias": …}`); WS topics `clients` (snapshot, then changed clients by MAC and the ones forgotten) and `client:<mac>` (snapshot, then its view, new samples and changed sessions; holding it speeds up the telemetry of the node the client is at, following it as it roams).

## Metrics [M2.4]

- **Series** (`model::metric`, D-056): an entity (`site`, `node:<id>`, `node:<id>/dev:<device>`, `node:<id>/radio:<radio>`, `link:<topology edge id>`, `client:<mac>`) and a metric with its unit. Nodes: `cpu`, `memory` (in use), `load` (1 min), `temperature`, `stations`. Devices (every one but loopback): `rx_bps`, `tx_bps` from their counters. Radios (up): `busy`, `noise`, `stations`. Mesh links: `signal`, `tq`, `quality`, `capacity`. Clients: `signal`, `snr`, `tx_rate`, `rx_rate`, `down_bps`, `up_bps`, `score`. Site: `down_bps`, `up_bps` (WAN), `clients` and `nodes_online`.
- **Sampling** (the pure `metrics` crate's `Sampler`, fed from the bus by the engine's `MetricsRecorder`): one value per series per aligned 15 s window, the average of what arrived in it. Gauges are read from every facts announcement, topology build and client sample; device rates come from two readings of fresh counters (weighted by time; none across a counter reset or a gap over 2 min). Windows close 5 s after they end and are written every 15 s; stopping writes the open ones. The 2 s telemetry of an open page reaches it over the WebSocket, not the store.
- **Storage** (`store`, migration 0007): `metric_series` names each series; `metrics_raw` keeps the windows for 48 h, `metrics_1m` for 14 d and `metrics_1h` for 400 d, each rollup with min/mean/max/last and the windows it covers (which weigh its mean in the next). Rollups run every minute for the minutes that ended 40 s before, in bounded chunks (an hour of minutes, a day of hours) from where they stopped (`metric_rollups`); retention runs every 15 min and drops series left empty.
- **Queries:** `GET v1/metrics?entity=&metrics=a,b&from=&to=&width=` reads at the finest resolution that holds `from` and keeps the span manageable (raw up to 6 h, minutes up to 8 d, else hours), fills the tail rollups have not reached from finer resolutions, and reduces each series with LTTB per run to about `width` points (16–4,000; default 800), each kept point with the envelope of those it stands for. A row of `null`s breaks the line where data stops for over three steps; values keep four significant digits. `GET v1/metrics/series?under=<entity>` lists what is kept (for the Metrics explorer).
- **Charts** (`web/src/viz/timeseries`, D-057): uPlot, loaded on first use, drawn in the theme's tokens (redrawn when the skin or theme changes), with the `min`/`max` envelope as a band, an HTML legend with each line's value at the pointer (or the latest), cursors shared between a page's charts, and drag-to-zoom in the explorer. Ranges: 1 h, 6 h, 24 h, 7 d, 30 d and 1 y, read again about as often as a point changes (15 s for an hour, 15 min beyond a week) while the page is visible. Lines from different reductions share one time axis, each spanning the times only the others have, and breaking where its own data stops.
  - **Device page** (Performance card, 24 h by default): CPU and memory, load, temperature (when the node reports it), one interface's throughput (the WAN's device by default, else the LAN's; a picker for the others) and the radios' channel utilization, Wi-Fi clients and noise floor, a line per radio in its band's color.
  - **Client page:** the Signal tab (signal; PHY rates down and up) and the Traffic tab (download and upload, with about how much moved in the range). The last hour comes from the samples in memory (live, every few seconds); 24 h, 7 d and 30 d from the stored series.
  - **Metrics explorer** (`/insights/metrics?entity=&metrics=&range=` or `&from=&to=` once zoomed): the entities with series, grouped (site; each node with its radios and interfaces; mesh links; clients) and filterable; the chosen entity's metrics as toggles (up to 8), one chart per unit. The device and client pages link to it.
  - **Dashboard:** the site's WAN throughput (Dashboard, below).
  - Still to come: WAN latency, jitter and loss (the gateway's probes, M4.1) The inter-node RSSI matrix (M2.6) is on the RF page (features/wifi.md), not in the stored series.
- A Prometheus `/metrics` exporter is in the backlog.

## Logs [M2.5]

Backend in M2.5a1 (D-059); the UI in M2.5a2 (D-060).

**Ingest** (the engine's `LogRecorder`):
- Every watched node's log is followed by its agent (`log_follow`, polling logd by sequence number every second; D-036), so observe mode needs no change on the node. The agent's backfill after a (re)start is skipped through the newest entry stored, so a controller restart neither loses nor repeats entries.
- OpenWrt logd's remote syslog (`log_ip`, `log_port 5514`, `log_proto tcp`) takes over once it arrives from a node: the agent stops following, and follows again when the node's TCP connection closes or its UDP syslog is quiet for 10 minutes. Around each switch, an entry one source delivered is skipped when the other delivers it within 30 s. Managed mode configures remote syslog later (M3); until then the owner may set it by hand.
- A syslog sender is a node when its address is one of a node's interface addresses, else when the host name in its messages is a node's. Loopback addresses, and addresses or names that several nodes share (the default `OpenWrt`), name no node. Everything else is stored under the sender's host name, or its address when it gives none.

**Receiver** (`syslog` crate): RFC 5424 and RFC 3164 over UDP and TCP on port 5514 (`--syslog-listen`, off in demo mode unless given). TCP frames are octet-counted or end with a line feed or NUL (RFC 6587; logd's `log_trailer_null`). The parser is lenient: what doesn't parse becomes the text, and messages are cut at 8 KiB. An RFC 3164 time names no zone, so the arrival time is kept instead, unless logd's `[s.ms]` stamp (`log_timestamp`) is there. Each sender may send 200 messages a second (bursts of 2,000), at most 64 TCP connections are open at once, and a full queue drops messages; drops are counted and logged once a minute.

**Storage:** the `logs` table plus a contentless FTS5 index over program and text. Entries are written in batches every 250 ms and announced once stored. Retention: 14 days by arrival, and 1 GiB, estimated from the newest entries' size. Both are pruned every 15 minutes, at most 10,000 entries per call. The hosts and programs that logged (`log_sources`) feed the filters.

**API:** pages, a histogram, context and saved searches (ARCHITECTURE.md, API). A filter takes sources (nodes and hosts together: an entry from any one passes), programs, the least severity, full text (every word must appear, `"a phrase"` as written, `word*` as a prefix, `-word` must not appear), annotated only, and a time range. The WS topic `logs[?node=<id>|host=<name>&min=<severity>]` sends the newest 200 stored entries, then each one stored after them.

**UI** (`/insights/logs`, M2.5a2, D-060):
- **URL:** the filter uses the API's names (`?nodes=3&hosts=sw&programs=hostapd&min=warning&q=…&annotated=1`) plus `range=15m|1h|6h|24h|7d|14d` (default 1h) or a fixed `from`/`to`, so views can be shared; the device page links to `?nodes=<id>`.
- **Live tail, newest on top:** the first page (200) comes from `GET v1/logs`, older pages load as the list scrolls (infinite scroll plus a button), and a preset range then follows the WS `logs` topic, narrowed to one node or host and the least severity where the hub can. The page applies the rest of the filter to arrivals, full text included, as the store does (unicode61 tokens: words, phrases, prefixes, exclusions). Scrolling away from the top, or **Pause**, holds arrivals behind a "Show N new entries" button; a reconnect or a `dropped` delta reads the newest page again. At most 10,000 entries are kept, and more than 5,000 held reload. A fixed window doesn't follow.
- **Filters:** search (300 ms debounce or Enter, `/` focuses it, matches marked), least severity, sources (devices and other hosts) and programs as filterable checkbox pickers fed by `GET v1/logs/sources`, "Explained only", clear.
- **Histogram:** stacked bars in four bands (errors and worse, warnings, notice and info, debug) on the status colors, one per ~8 px (20–120), re-read every 15 s (60 s beyond 6 h) while live; hover shows a bar's counts, and a click or drag zooms the page into that window.
- **Entries:** time to the millisecond (with the day when not today), severity, source, `program[pid]`, message, and the annotation's meaning as a chip. A row expands in place: full message, the annotation's meaning, hint and link, when it was logged and received, facility, how it arrived and logd's sequence, and actions: show context, only this source, only this program, copy the line. Lists over 200 entries are virtualized.
- **Context:** a wide dialog with ± 20 entries from every source around the entry (marked and centred), widened to 50, 100 and 200 with "Show more".
- **Saved searches:** a popover lists them (the one in use is checked and names the button); apply, delete, or save the current filter under a name. They keep the filter, not the range.
- Later: correlation with events (M2.5b).

**Annotations:** `crates/syslog/data/log-patterns.toml` maps a regex over an entry's text, for some programs or all, to a meaning, severity, hint and link, and optionally an event category (its entries are events too; Events below). The first match wins and its id is stored with the entry; `GET v1/logs/patterns` explains each id. Each pattern needs a real example line in the tests. The first set covers hostapd's wrong passwords, inactivity and local deauths, DFS radar and CAC, netifd link loss and interface and port downs, a full DHCP pool, kernel OOM, mt76 firmware timeouts and firmware crashes, bridge loops, a full conntrack table, the batman-adv MTU, flash read errors, segfaults, failed SSH logins and PPP drops.

## Events, alerts, notifications [M2.5]

Events in M2.5b1, alerts and notifiers in M2.5b2 (D-061).

**Events** (the `events` crate derives them; the engine's `EventRecorder` stores them):
- **System:** the controller started; a node adopted; offline (unreachable for 30 s) and back online (with how long it was down); degraded (telemetry missing, partial or late for 60 s) and recovered; rebooted (its boot time moved by over a minute; read only from a freshly collected system set); flash low (under 10 % free) and fine again (from 15 %); a job failed; a managed package changed on its node after the controller confirmed it (`config_drift`, raising `node:<id>/drift:<package>`) and its drift ended (`config_drift_ended`: reverted, confirmed by an apply, acknowledged, muted, or no longer managed; features/lifecycle.md, Drift).
- **WAN:** a gateway's WAN link down (no default route for 10 s) and up again; a WAN interface's IPv4 addresses or delegated prefixes changed.
- **Mesh:** a link between adopted nodes down (reported down, or gone from the topology while both nodes are online, for 30 s) and up; weak (quality under 40 for 2 min) and recovered (from 50). A link gone for 24 h is forgotten (the mesh was rewired).
- **Clients:** joined, left (with how long it was connected), roamed (between access points; 802.11r marked) and new (never seen before). Nothing is derived during a node's first minute online, whose station lists show clients that were already there (nor, for clients no node places, during the recorder's first minute), and clients don't "leave" a node that went offline.
- **From logs:** an entry whose pattern has an `event` category in `log-patterns.toml` is an event of that category, at the pattern's severity (DFS radar, a full DHCP pool, OOM, Wi-Fi firmware crashes and timeouts, bridge loops, a full conntrack table, the batman-adv MTU, flash errors, segfaults, wrong Wi-Fi and SSH passwords, PPP drops). A source's repeats of a pattern within 5 minutes of the last fold into one record with a count.
- **Levels:** critical, warning, notice, info (log severities map: emergency to critical → critical, error and warning → warning). **Categories:** system, wan, mesh, wifi, client, network, security.
- **Conditions:** states that begin and end (offline, degraded, flash, a WAN link, a mesh link down or weak) are raised by one event and cleared by another, which share the condition's key (`node:<id>/offline`, `node:<id>/wan:<device>`, `node:<id>/mesh:<edge>`). Nothing is said about a node that is not online, nor its links. After a restart the recorder resumes the conditions that the store's latest events left raised, and the nodes' facts restored from snapshots, so it neither repeats nor misses a change.
- **Storage:** the `events` table (the event's JSON, its level, category, node, client and condition), written in batches every 250 ms and announced once stored (`Event::Recorded`, the alert engine's input). Kept 60 days after the last repeat, and at most 200,000.
- **API:** `GET v1/events` (filter: categories, the least level, nodes, clients, time range; pages newest first). The WS topic `events[?node=<id>&min=<level>]` sends the newest 100, then each event recorded or folded.
- **UI** (`/insights/events`, `features/events`): the filter in the URL with the API's names (`?categories=wan,mesh&min=warning&nodes=3&clients=<mac>`) plus `range=1h|6h|24h|7d|30d|60d` (default 24h) or a fixed `from`/`to`; category toggles, the least level, a device, a client (from the client page), clear. The first page (100) from `GET v1/events`, older ones by a button; a preset range follows the WS topic (narrowed to one node and the level where it can, the rest of the filter applied on the page as the store does), a folded repeat replacing its record. Events run newest first in days ("Today", "Yesterday", the date), each with its category's icon tinted by its level, its title and detail in words (`lib/events.ts`, shared with the dashboard), the repeats' count, and "Ongoing" on a condition nothing cleared yet. A row expands into when, the last repeat, level, category, device, client, condition, and actions: the logs around it (`/insights/logs?from=<at − 5 min>&to=<last + 5 min>&nodes=<node>`), open the device or client, only this device, only this category. The device and client pages link to their events.
- **From radios (M2.6):** a radio moving to another channel (`channel_changed`, marked `radar` when hostapd reported radar on its node in the 2 minutes before: a DFS channel it had to leave), and its channel congested (`channel_congested` at 80 % busy or more for 15 minutes, raising `node:<id>/radio:<name>/busy`; `channel_clear` at 65 % or less, with how long it lasted; the built-in `channel-busy` rule's thresholds). Radar itself and the start of a CAC come from the log patterns `dfs-radar` and `dfs-cac`.
- Later: config applied or rolled back and upgrade steps (M3, M7), clients blocked (M5.1).

**Alerts** (M2.5b2a, D-062; the pure logic in `uniwrt_events::alerts`, run by the engine's `AlertEngine`):
- **Rules** are of two kinds. A *metric rule* watches every stored series that matches an entity pattern (`node:*`, `node:3`, `node:*/radio:*`, `node:*/dev:wan`, `client:*`, `link:*`, `site`): above or below a threshold for `for_s` (contiguous 15 s windows; one missing window is tolerated, two start over) fires one alert per series; it resolves once a window is back on the safe side of `clear` (hysteresis; the threshold by default), or as *no data* when the series says nothing for 5 min. An *event rule* matches recorded events by kind, category, least level, node and log pattern, and fires when `count` matches fall within `window_s` for one subject (the event's node, else its client, else the site); folded repeats count. With a count of 1, an event that raises a condition is its own subject (the condition's key) and its alert lasts until the condition clears; other event alerts resolve once the window holds fewer matches.
- **Built-in rules** are presets of the same two kinds in `crates/events/data/alert-rules.toml` (device offline, degraded, rebooted, going offline repeatedly, flash nearly full, CPU, memory, temperature, job failed, configuration changed on a device (`config-drift`, firing until the drift ends), internet down, mesh link down or weak, channel congested, radar, Wi-Fi firmware trouble, a critical problem in a log, LAN service trouble, repeated wrong passwords; a poor client experience and new devices are off by default). They are stored at start when missing, edited and disabled like the user's own, reset to what shipped, never deleted. At most 200 rules.
- **Alerts:** one per rule and subject while it fires, with its level and name from the rule, the subject's node or client, when it started, last matched and resolved (cleared, no data, or its rule changed), and its cause (the metric's value when it fired and the worst since; or the latest event, the matches in the window and the condition). Firing again within the rule's cool-down of resolving reopens the alert (its count grows, its acknowledgement goes). A rule deleted, disabled or edited resolves its alerts; they start afresh. Resolved alerts are kept 90 days.
- **Acknowledge** (admins): an alert still firing; it dims in the bell until it resolves (b2b: no reminders).
- **Maintenance:** alerts that fire while a job runs on their node (and 5 min after), or within a maintenance window (a time range, every device or some, a note; at most 30 days, starting within a year), are recorded *silenced* (b2b notifies nobody); one still firing when maintenance ends is no longer silenced. A snooze is a window from now.
- **Restart:** the engine resumes the alerts firing and those within their cool-down; a metric alert resolves as no data if its series stays quiet, a counted event alert when its window passes, a condition's when it clears.
- **API:** `GET v1/alerts` (filter: `active`, the least level, nodes, rules, start range; pages newest first), `POST v1/alerts/{id}/acknowledge`; `GET` and `POST v1/alert-rules`, `PUT` and `DELETE v1/alert-rules/{id}`, `POST v1/alert-rules/{id}/reset`; `GET` and `POST v1/maintenance`, `DELETE v1/maintenance/{id}`. Changes are admins' and audited. The WS topic `alerts` sends the alerts firing, then each alert stored (opened, changed, resolved, acknowledged).
- **UI** (M2.5b2a2; `lib/alerts.ts`, `app/AlertsBell.tsx`, `features/alerts`): the signed-in app keeps the alerts firing from the WS topic (`Live.alerts`); each alert reads as its rule's name, its subject in words (a series' entity such as `ap-garden · radio1`, a condition's device and WAN interface or mesh link, a client, the whole network) and its cause (`CPU 93 %, above 90 % · worst 97 %`, or how many events matched).
  - *Bell* (top bar): the count of alerts wanting attention (firing, neither acknowledged nor silenced), tinted by the worst level; a dot when only attended ones fire. Its popover lists the alerts firing, wanting attention first, then by level (8, then "N more"), each with how long it has fired; admins acknowledge there; an alert opens `/insights/alerts?alert=<id>`.
  - *`/insights/alerts`*: tabs in the URL (`?tab=history|rules|maintenance`). **Active** (the bell's order; a row expands into its times, count, level, subject, cause, condition, acknowledgement and silence, with links to its device or client, the events around it and its rule; admins acknowledge and snooze its device or everything). **History** (`GET v1/alerts` pages from a range of 24 h, 7 d (default), 30 d or 90 d, the least level and a device in the URL, kept current by the topic). **Rules** (the user's, then the built-in ones, each in words, e.g. `CPU above 90 % for 10m · Every device`; admins turn them on and off, edit, reset a modified built-in one, delete their own and add new ones; `?rule=<id>` opens a rule's editor for admins and marks it for viewers). **Maintenance** (the windows on now and to come; admins end or cancel them, snooze and schedule one: start, end, every device or some, a note).
  - *Rule editor* (a dialog): name, level, on, and what it fires on. A metric: the kind of entity (devices, radios, interfaces, mesh links, clients, the network) and which (any or one device; an interface or radio name; a client's MAC), the metrics that kind has, above or below, the threshold (with how it reads, `Reads as 50 Mbit/s`), where it clears, and how long. Events: kinds in groups, log patterns (when log events can match), categories, the least level, devices, how many within which window. A cool-down. Checked as the server checks it; its message shows when it refuses anyway.
  - *Snooze*: a window from now for 1 h, 4 h or 1 d, for a device or every device (note "Snoozed").
  - Viewers see everything and change nothing.

**Notifiers** (M2.5b2b, D-063; `uniwrt_notify`, run by the engine's `Notifications`): in-app (the bell, M2.5b2a), ntfy, webhooks and email first; later browser push (PWA), Gotify, Pushover, Telegram.
- **Kinds:** *ntfy* (a server, ntfy.sh by default, and a topic; an access token, or a username and password); *webhook* (a URL and headers, both secrets; bodies as uniwrt's JSON, which Home Assistant's webhook trigger reads, a Discord embed with mentions disabled, Slack blocks, a Teams Workflows Adaptive Card, or a custom JSON template); *email* over SMTP with STARTTLS (required) or TLS, never in the clear (a server, port, optional username with its password, a sender, 1–10 recipients; plain text and HTML). Up to 20 notifiers; each has a name, is on or off, and records its last success and its last failure with why.
- **Which alerts:** each notifier's filter: the least level (warning by default), optionally only some devices (alerts about no device then pass only when it names none), whether it hears of resolutions (yes by default), and a reminder interval (off, or 15 min to 7 days; 4 h by default).
- **When:** an alert is sent once when it opens or reopens to each notifier its filter admits, unless it is silenced or acknowledged; one silenced when it opened is sent once maintenance ends. While it fires, neither acknowledged nor silenced, each notifier that heard of it is reminded at its interval. When it resolves, those that heard of it are told, if they want resolutions. An alert resolved because its rule was edited tells nobody, and the alert that replaces it within 2 minutes carries on where it left off. A notifier added or changed hears of alerts that open from then on. After a restart the alerts still firing count as heard of.
- **Messages:** the rule's name and the subject in words (`CPU high · ap-garden`; `Again:`, `Still firing:` or `Resolved:` before it when it is not news), the cause (`CPU 93 %, above 90 % · worst 97 %`), the level and how long it has fired since when (UTC), how often it fired; a link to `/insights/alerts?alert=<id>` when the controller's address is set (`link_url` in the settings). Alerts for one notifier within 2 s go as one digest (`3 alerts: 2 firing, 1 resolved`, a line each; at most 25). ntfy's priority follows the level (5 critical, 4 warning, 3 notice, 2 info; 3 for resolutions and tests).
- **Templates** (custom webhooks): JSON with `{{placeholders}}`: text ones (`title`, `message`, `level`, `event` (opened, reopened, reminder, resolved, digest, test), `url`, `at_iso`, `name`, `subject`, `cause`) are JSON-escaped without quotes (write `"{{title}}"`); number ones (`at`, `alerts`, `alert_id`, `node`) are digits or `null` (write `{{node}}`). A template must make JSON for an alert and for a test, at most 8 KB.
- **Delivery:** HTTP(S) over rustls with Mozilla's roots, 15 s per exchange, no redirects; what may pass (timeouts, connection errors, HTTP 429 and 5xx, transient SMTP errors) is tried again after 15 s, 1 min and 5 min.
- **Secrets** are in the vault (`notifier/<id>`, features/security.md); the API never returns them.
- **API:** `GET v1/notifiers` (the notifiers with which secrets are set, the settings, the placeholders), `POST v1/notifiers`, `PUT` and `DELETE v1/notifiers/{id}` (admins; audited; secrets left out are kept, `null` removes one; only those the kind uses are kept), `POST v1/notifiers/test` (admins: a notifier as given, with a stored one's secrets when it names its `id`; sent at once, once; 502 with why), `PUT v1/notifications/settings` (admins; audited: `link_url`).
- **UI** (M2.5b2b2; `features/settings/notifications`): Settings › Notifications (`/settings`, its sections in tabs named in the URL, `?tab=notifications` the first). **The list:** each notifier's kind, name, where it sends (`ntfy.sh/<topic>`, `Discord · https://discord.com`, the first recipient and how many more), its filter in words (`Warning and above · Every device · Resolutions · Reminders every 4 hours`) and its last delivery (`Delivered 5m ago`, or `Failed 2m ago:` and why); read every 30 s for the deliveries. Admins turn each on and off, edit, remove (asked first) and add (up to 20; with none, a button per kind). **The editor** (a dialog): the kind (chosen once), name, on; ntfy's server (ntfy.sh), topic (a random `uniwrt-…` one, hard to guess) and sign-in (none, an access token, a user and password); a webhook's format (Discord, Slack, Microsoft Teams, Home Assistant, uniwrt JSON, a custom template), URL and headers; email's server, port (587 or 465 follows STARTTLS or TLS unless another was chosen), sign-in, sender and recipients; and *Which alerts* (the least level, every device or some, resolutions, reminders off or every 1, 4 (default), 12 or 24 hours). **Secrets** stored show as set (a URL as its host, a header by its name) with Replace (and Remove where the kind can do without; Keep current undoes either); a field left alone is left out of the request. **Template editor:** the JSON with a byte count, the placeholders (numbers tinted) put in at the caret, and a live preview of what an alert or a test makes of it, rendered as the server renders it (`template.ts`); the same check refuses a template that is not JSON for either. **Send a test** sends what is in the form (with the stored secrets of a stored one) and shows the server's why when it fails. Checked as the server checks it (`draft.ts`); the server's message shows when it refuses anyway. **Links in messages:** the controller's address, prefilled with the page's own while unset ("Not set"), saved or cleared. Viewers see it all and change nothing.

## Dashboard [M2.4, extended in M2.5 and M2.6]

`/` (`features/dashboard`, D-058): everything live except the WAN's history. With no device adopted, a welcome that opens the adoption wizard.

- **Health header:** five tiles, each a link to its details, colored by its state.
  - *Internet:* online while a gateway's WAN link has a default route (the topology's `wan` edges), offline when none has, unknown without a gateway or while it is offline; what the WAN carries now.
  - *Mesh:* the mesh links between adopted nodes (peers not adopted aside): critical with one down, else the weakest link's quality (good from 60, fair from 40).
  - *Wi-Fi experience:* the mean score of the online Wi-Fi clients, how many, and how many have a weak signal (below −75 dBm).
  - *Nodes:* online of adopted; critical with one offline, fair with one degraded.
  - *Clients online:* Wi-Fi and wired.
- **Internet card:** state, download and upload now, the site's throughput (`site` `down_bps`/`up_bps`) over 1 h, 24 h or 7 d with a link to the Metrics explorer, and the gateway's WAN interfaces: protocol, IPv4 and IPv6 addresses, delegated prefix, gateways, DNS servers and how long it has been up. The addresses come from the gateway's record, read every 30 s while the page is visible (following its topic would collect it more often). ISP, latency, jitter and loss arrive with M4.1.
- **Topology mini-map:** the tree view with each host's clients grouped and flows animated, on the topology renderer with a fixed camera (the page keeps the wheel and touch). A device opens its page, a client its own, anything else the full map.
- **Top clients:** the five online clients moving the most traffic now, with bars of download and upload against the busiest.
- **Wi-Fi experience:** the mean score as a ring, how the clients spread across good, fair and poor, and the weakest access point.
- **Recent events:** the six newest notices and worse from the WS topic `events?min=notice` (clients' comings and goings stay on the events page), each with its category's icon tinted by its level, its title and detail, linking to the events page for its device (M2.5b1). Above them, the alerts firing (M2.5b2a2: three, wanting attention first, each opening the alerts page on it; the events shown shrink to keep the card level).
- **RF** (M2.6, `RfCard` over the WS topic `rf`): a tile per band: the channels its radios use, the busiest with how busy, how many radios and neighbors, and badges for crowded radios and lookalikes of the site's network; each opens `/insights/rf` on its band.

## Diagnostics toolkit [M8.3]

Everything runs from any node and streams live:
- ping, traceroute, DNS lookup (per resolver), HTTP check
- iperf3 node ↔ node (backhaul capacity) and node ↔ controller
- WAN speed test (from the gateway; done in M4.1b as the `speed_test` job, wan.md SQM)
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
