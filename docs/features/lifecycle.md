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

## Render → diff → apply [M3.3–M3.4]

1. **Render** (pure, `crates/render`, D-067): `(SiteConfig, the vault's secret values, the node's role, NodeFacts, the node's current packages)` → its managed packages (`system`, `network`, `wireless`, `dhcp`, `firewall`; managing another is an error until it gets a renderer), the services to enable or disable, and warnings, or the issues that keep the node from rendering. Rendering depends on capabilities (switch kind, hostapd features, batman-adv) and the board's layout (board.json), not version strings.
   - Every node bridges all networks into `br-lan`: a plain bridge while every network is untagged (OpenWrt's stock shape), otherwise a VLAN-filtering bridge with each network's interface on `br-lan.<vid>`, Ethernet ports and `bat0` as trunks (the untagged network untagged, the others tagged) and Wi-Fi joining through its interface. On swconfig each VLAN is a `switch_vlan` tagged on the CPU port, whose `<cpu>.<vid>` devices join the bridge as access ports.
   - The gateway routes (a static interface per network, the WAN, dnsmasq, odhcpd, a firewall zone per network); APs bridge every port, the WAN port too, take a DHCP address on the management network (else the default one), and have dnsmasq, odhcpd and the firewall disabled, with firewall defaults that accept input.
   - Values the node generated for itself are carried over from its current packages: the DHCPv6 DUID (`network.globals.dhcp_default_duid`, which keeps the delegated prefix), `compat_version` when board.json has none, the board's `led` sections, and odhcpd's options other than `maindhcp`, `leasetrigger` and `loglevel` (its paths differ between releases).
   - The node's raw sections join their package last, replacing a rendered section of the same name.
   - Rendered packages hold secrets in option values (`key`, `password`): the review screen must mask them.
2. **Diff:** a semantic UCI diff (`uniwrt_uci::diff`: sections added, removed, changed or moved, option by option; order counts only among the sections of one type; D-066) against the node config, fetched fresh right before apply, shown on the review screen.
3. **Plan:** order the nodes. Disruptive changes go leaves → gateway; additive ones gateway first.
4. **Apply** each node as a transaction (D-012):
   - Upload the bundle (files plus metadata). Validate it: `uci -c <stage> show` for each package, and `fw4 check` for the firewall.
   - Snapshot the current packages to `/etc/uniwrt/txn/<id>/`, install the new files, then reload only what changed (`reload_config`; `wifi reload` when wireless changed; a network restart only when required).
   - Arm the dead-man timer (supervised by procd, or a detached timer). Default 120 s; longer for WAN and mesh changes.
   - The controller verifies over a **new** SSH connection to the expected address and runs role-specific health checks: gateway WAN up and DNS working; mesh neighbor count at least what it was; radios up; clients re-associating. Then it sends `confirm`.
   - Without a confirm, the node restores the snapshot and reloads. After an unexpected reboot, `/etc/init.d/uniwrt-guard` rolls back any unconfirmed transaction.
5. **Record** the applied revision per node, plus events and logs.

**Partial failure:** the job stops and alerts by default. The user can instead keep the successfully applied nodes, which stay consistent with their revision, or roll back all of them.

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
