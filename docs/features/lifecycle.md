# Configuration lifecycle, upgrades & backups

Read this when working on desired state, revisions, render/diff/apply, drift, brownfield import, firmware upgrades, backups or node replacement.
Milestones: M3.1–M3.5, M7.1–M7.2.

## Desired state & revisions [M3.1] (D-065)

- The controller owns one typed document, `SiteConfig` (`crates/model/src/site/`): site settings (name, country, time zone, domain, NTP), the address plan (IPv4 space and ULA), networks (purpose, VLAN, IPv4 with DHCP and reservations, IPv6 mode, isolation, mDNS, IGMP snooping), WLANs (security, passphrase, bands, node scope), the mesh, the radio policy per band, the WAN (DHCP, static or PPPoE; VLAN, MAC clone, IPv6), firewall policy overrides between networks, local DNS records, and per node its hostname, managed packages (D-011), radio overrides and raw UCI sections. A node's role stays on its inventory record. Schema v1 has every section; the fields are what M3.3 renders, and later WPs extend them.
- Networks and WLANs have ids (`[a-z][a-z0-9_]{0,11}`), which name their UCI sections (interfaces, zones, DHCP pools, `uw_<wlan>_<band>`), so they never change once created. A few are reserved for render's own (`loopback`, `wan`, `wan6`, `wan_6`, `bat0`, and the `uw_` prefix). Nodes are keyed by their id.
- **One linear history:** every save is an immutable revision (`revisions` table) with its author, time, message, schema and its structural diff from its parent (list items matched by key, so editing one network is one change at `networks/<id>/…`). A save names the revision it was edited from and is refused with 409 when another save came first. A save that changes nothing makes no revision. Restoring copies an older revision's document into a new revision. Discard (M3.4) is a restore of the last applied revision.
- **Validation** (`site::validate`): errors refuse a save (422 with issues by path); warnings come with the document. It enforces the IPAM guarantees (features/network.md), unique ids, names and VLANs, WLAN security rules (6 GHz needs WPA3 or OWE; PSK modes need a passphrase), channel and width per band, the WAN's fields, firewall cells, DNS names, and UCI identifiers in raw sections.
- **Secrets** are vault references, never values: each new value is sealed under a new name `config/<32 hex digits>` and committed with its revision, so an older revision (and a restore) still finds the value it had. A document may refer only to the secrets its parent or the revision it restores refers to, plus the ones committed with it. Values are checked (Wi-Fi and mesh keys 8–63 printable ASCII or a 64-hex PSK; PPPoE 1–128 characters).
- A new site starts from the defaults (revision 1): the 10.0.0.0/16 plan with a random ULA, the default LAN at 10.0.0.1/24 (DHCP .100–.249) with IPv6 from both the ULA and the delegated prefix (D-067), no WLANs, the mesh off, DHCP on the WAN, no country (a warning once radios broadcast). Nodes stay unmanaged, so nothing reaches them until management is enabled (M3.4, M3.5).
- Inheritance: site defaults → node group (later) → node overrides → per-node raw UCI sections (the validated escape hatch).
- Schema migrations (`site::migrate`) are forward-only steps on the JSON; revisions keep the schema they were written with and are upgraded when read. The tests upgrade a fixture of every schema (`crates/model/fixtures/site/v<n>.json`).
- API: `GET`/`PUT /api/v1/site`, `POST /api/v1/site/secrets/reveal` (admins; audited), `GET /api/v1/revisions` (pages), `GET /api/v1/revisions/{id}`, `GET /api/v1/revisions/{id}/diff?against=<id>`, `POST /api/v1/revisions/{id}/restore` (admins; audited).

## Ownership (D-011)

- For each node and each UCI package, the package is either **unmanaged** (observe only) or **managed** (rendered by the controller, which owns the whole file). The managed set grows feature by feature: network, wireless, dhcp, firewall, system, dropbear, usteer, sqm, wireguard, and so on.
- Unmanaged packages are never touched. Unknown sections inside a managed package survive only if brownfield import captured them as raw overrides.
- **Management mode [M3.4a1] (D-069):** a node starts in observe mode (directive 8) and is managed only once an admin turns management on for it (`PUT /api/v1/nodes/{id}/management`, `{"enabled":true}`; audited). The manage job logs in with the controller's key against the pinned host key, installs the footprint (the transaction tool `/etc/uniwrt/txn`, the guard `/etc/init.d/uniwrt-guard`, `/lib/upgrade/keep.d/uniwrt`) unless that version is there already, and records the node as managed. Turning management off removes the footprint and leaves the configuration as it is. An open transaction blocks both. Applies touch managed nodes only and list the others as skipped.

## Render → diff → apply [M3.3–M3.4] (D-012, D-069, D-070)

The apply job (`crates/jobs/src/apply/`) applies the site's latest revision to every managed node: connect → plan → stage → apply → verify → confirm → record. API (admins): `GET /api/v1/apply/plan` (`ApplyPlan`: per node its action — change, unchanged, skipped in observe mode, or refused with its issues — the package diffs with secrets masked, the services switched, an address change, its hops and timer, in apply order), `POST /api/v1/apply` with `{"revision":<id>}` (the latest revision, the one reviewed; 409 otherwise; 202 with the job; audited), `GET /api/v1/apply/applied` (the revision each node holds). A job on several nodes keeps each node's state (`Job.nodes`: pending, unchanged, skipped, staged, applied, verified, confirmed, rolled back, failed) and its revision. A node renders only the packages its `managed` set lists (empty until chosen, D-011), so an apply leaves a node alone until then.

1. **Render** (pure, `crates/render`, D-067): `(SiteConfig, the vault's secret values, the node's role, NodeFacts, the node's current packages)` → its managed packages (`system`, `network`, `wireless`, `dhcp`, `firewall`; managing another is an error until it gets a renderer), the services to enable or disable, and warnings, or the issues that keep the node from rendering. Rendering depends on capabilities (switch kind, hostapd features, batman-adv) and the board's layout (board.json), not version strings.
   - Every node bridges all networks into `br-lan`: a plain bridge while every network is untagged (OpenWrt's stock shape), otherwise a VLAN-filtering bridge with each network's interface on `br-lan.<vid>`, Ethernet ports and `bat0` as trunks (the untagged network untagged, the others tagged) and Wi-Fi joining through its interface. On swconfig each VLAN is a `switch_vlan` tagged on the CPU port, whose `<cpu>.<vid>` devices join the bridge as access ports.
   - The gateway routes (a static interface per network, the WAN, dnsmasq, odhcpd, a firewall zone per network); its DHCP pools have `force '1'`, since dnsmasq's init otherwise refuses a segment where another server answers, such as an AP that still runs its stock one (D-068). APs bridge every port, the WAN port too, take a DHCP address on the management network (else the default one), and have dnsmasq, odhcpd and the firewall disabled, with firewall defaults that accept input.
   - Values the node generated for itself are carried over from its current packages: the DHCPv6 DUID (`network.globals.dhcp_default_duid`, which keeps the delegated prefix), `compat_version` when board.json has none, the board's `led` sections, and odhcpd's options other than `maindhcp`, `leasetrigger` and `loglevel` (its paths differ between releases).
   - The node's raw sections join their package last, replacing a rendered section of the same name.
   - Rendered packages hold secrets in option values (`key`, `password`): the review screen must mask them.
2. **Diff:** a semantic UCI diff (`uniwrt_uci::diff`: sections added, removed, changed or moved, option by option; order counts only among the sections of one type; D-066) against the node config, fetched fresh right before apply, shown on the review screen.
3. **Plan (fleet two-phase, D-069):** every managed node is logged in to and read at once (the tool's status, its packages and services with one command each, the agent's facts, and on the gateway its DNS), and every one must be reachable and plannable, or nothing changes. The nodes whose packages (a non-empty semantic diff) or services change are ordered leaves → gateway (by mesh and wired hops from the gateway in the topology the fresh facts describe). Every node is staged and checked before any applies; then all apply with their timers armed, then all are verified, then all are confirmed. A change that only works on every node at once (the management VLAN or subnet, the trunk ports, the mesh) therefore applies, which neither leaves-first nor gateway-first order allows node by node.
4. **Apply** each node as a transaction (D-012) through the transaction tool (`node-agent/txn/txn`, run over exec; `uniwrt_node::txn`):
   - `stage <id> <service>=on|off…` and `put <id> <package>` (the file on stdin) fill a stage in RAM (`/tmp/uniwrt/txn/<id>`); `check <id>` parses every staged package with `uci -c <stage> show` and checks that the services to start are installed. Nothing has changed yet.
   - `apply <id> <seconds>` snapshots the packages it replaces (or notes them absent) and the services' enablement to `/etc/uniwrt/journal/<id>/`, arms the dead-man timer (30 s – 1 h; default 120 s, longer for WAN and mesh changes, and `extend` keeps it alive while the other nodes apply), stops and disables the services that go off, installs each package atomically (keeping the file's mode), runs `fw4 check` when the firewall is staged (it reads only `/etc/config`, so it runs after install but before anything reloads, and a failure rolls back at once), enables the services that go on, and answers. procd then reloads (`reload_config`, then the services turned on start) in the guard's instance, which the SSH session dropping cannot stop; the state goes `applying` → `reloading` → `applied`.
   - Stop and disable the services that go off **before** `reload_config`: procd runs a service's reload triggers about a second later (`PROCD_RELOAD_DELAY`), and dnsmasq's reload starts it whether it is enabled or not; a stopped service has no triggers left. Health checks wait for the new config's own values (an address, a device), not just "up": until the triggers run, netifd still reports the old state (the system harness, M3.3b).
   - The dead-man timer is the guard's procd instance `txn wait <id>`, respawned if it dies; the deadline counts in uptime, so clock steps don't move it.
   - The controller verifies every node at once over a **new** SSH connection, at the address it moves to first, then its old one (D-070: the gateway follows the network that holds its address; an AP stays while the management network's subnet holds its address, else takes its DHCP reservation, else the plan refuses it). It waits for `applied`, extends the timer, and checks health **against the baseline taken while planning**: what passed before must pass again (the gateway's WAN up with a default route and its dnsmasq resolving `localhost` and `openwrt.org`; radios up that were up and stay enabled; at least as many batman-adv neighbors); reaching the node is itself the check that the controller reaches it. It polls for up to 60 s (150 s after a WAN or mesh change), keeping the timers of the nodes already verified alive. Then it sends `confirm <id>` to each, leaves → gateway (accepted only once `applied`; `expired` once it rolled back).
   - Without a confirm, the node restores the snapshot, its services' enablement, reloads and restarts what it had stopped (`rolling_back`, then `rolled_back` with the reason `timeout`, `requested`, `invalid` or `boot`). After an unexpected reboot, `/etc/init.d/uniwrt-guard` (START=13: after logd, before the network) rolls back any unconfirmed transaction before the services start; a rollback cut short by a reboot runs again from the same snapshot.
5. **Record** the applied revision per node (`node_applied`, with the nodes found unchanged), and move each renumbered node's address and watch it there; the job is history, and a failed one an event and an alert (`job-failed`).

**Failure:** an unreachable or refused node fails the job before anything changes (`unreachable`, `refused`), as does one that refuses its stage (`node_failed`). A node that fails its apply or health checks stops the job before any confirm (`rolled_back`): the controller rolls back the nodes it applied (`rollback`, the gateway first), and any it cannot reach roll back at their deadline. A first confirm that fails rolls the rest back the same way; only one that fails after others succeeded (the node's timer ran out in between) leaves a partial apply (`partial_apply`): the rest still confirm, the job fails and alerts, and the user can keep the confirmed nodes, which stay consistent with their revision, or roll them back with the previous revision. Renumbering the network the controller itself sits on needs the controller to reach the new subnet within the timer (its own lease renews later): otherwise the apply rolls back, safely. Jobs don't resume (D-042): after a controller restart the unconfirmed nodes roll back by themselves.

## Drift [M3.5]

The agent watches hashes of `/etc/config`. Any change the controller didn't make raises a drift event with a diff. The user can import it into the model, overwrite it (re-apply), or ignore drift for that package.

## Brownfield import [M3.5]

- The node's UCI is parsed and known constructs are mapped into `SiteConfig` (networks, WLANs, mesh, WAN, DHCP, …). Leftover sections become raw overrides for that node.
- Invariant: `render(import(x))` reproduces `x` semantically, enforced by round-trip tests.
- The UI shows an import confidence score and the residual raw sections.

## Firmware & rolling upgrades [M7.1] (D-015)

- **Catalog:** OpenWrt releases (from `downloads.openwrt.org/.versions.json` and per-target `profiles.json`), each node's current version, available upgrades and links to release notes. Snapshot builds are opt-in.
- **Image source:**
  - Default: ASU (sysupgrade.openwrt.org or a self-hosted ASU) building the node's declarative package set (role + features + user extras), so the image has everything baked in.
  - Fallbacks: official images with packages installed after the upgrade, or an image the user uploads.
- **Verification:** the sha256 from ASU or `profiles.json`, `sysupgrade -T` on the node, a free-RAM check (the image is staged in `/tmp`), and board compatibility.
- **Rolling job:** leaves first (deepest in the mesh tree), gateway last. For each node:
  1. pre-checks, backup
  2. stage the image (streamed over SSH), verify it
  3. sysupgrade, keeping the config
  4. wait for the node to go down, then come back up (10 min timeout)
  5. re-push the agent and run post-checks: version, packages, mesh links, clients back, no new error log patterns
  6. soak (default 3 min), then move to the next node
- **On failure:** halt, with recovery guidance (failsafe mode, TFTP).
- **Options:** canary (one node, soak for N hours), maintenance window, pause/resume, skip a node.
- **Version paths:** 24.10 → 25.12 moves from opkg to apk; ASU images carry the package set. 23.05 → 25.12 requires two hops.

## Backups [M7.2] (with replace, D-054)

- **Nodes:** a `sysupgrade -b` archive on a schedule and before every apply or upgrade, keeping the last N per node.
- **Controller:** an online SQLite backup (`VACUUM INTO`) plus the vault and certificates, as an encrypted tarball. Scheduled, and restorable on a new host.
- **Site export/import** as JSON (secrets optional and encrypted), for migration or config-as-code.

## Replace / RMA / decommission [M7.2]

- **Replace:** mark the old node as failed, adopt the new device (any model), and map role, name and overrides onto it. Radios are remapped by band. Then apply.
- **Factory reset & re-adopt:** a guided flow.
- **Decommission:** remove the node from the mesh, wipe the uniwrt footprint, and optionally run `firstboot`.
