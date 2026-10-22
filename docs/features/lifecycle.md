# Configuration lifecycle, upgrades & backups

Read this when working on desired state, revisions, render/diff/apply, drift, brownfield import, firmware upgrades, backups or node replacement.
Milestones: M3.1–M3.7, M7.1–M7.4.

## Desired state & revisions [M3.1]

- The controller owns one typed document, `SiteConfig` (in the `model` crate): networks, WLANs, mesh, radio policy, WAN, firewall, services, nodes (role and overrides), plus a schema version.
- Every save creates a revision with author, time, message and a structural diff. Secrets are stored as vault references, never inside revisions.
- Inheritance: site defaults → node group (later) → node overrides → per-node raw UCI snippets (a validated escape hatch).
- Schema migrations are forward-only and tested against fixtures of every old schema.

## Ownership (D-011)

- For each node and each UCI package, the package is either **unmanaged** (observe only) or **managed** (rendered by the controller, which owns the whole file). The managed set grows feature by feature: network, wireless, dhcp, firewall, system, dropbear, usteer, sqm, wireguard, and so on.
- Unmanaged packages are never touched. Unknown sections inside a managed package survive only if brownfield import captured them as raw overrides.

## Render → diff → apply [M3.3–M3.4]

1. **Render** (pure): `(SiteConfig, NodeFacts capabilities)` → per-node UCI packages. Rendering depends on capabilities (DSA vs swconfig, wpad variant, apk vs opkg), not version strings.
2. **Diff:** a semantic UCI diff against the node config, fetched fresh right before apply, shown on the review screen.
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

## Firmware & rolling upgrades [M7.1–M7.2] (D-015)

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

## Backups [M7.3]

- **Nodes:** a `sysupgrade -b` archive on a schedule and before every apply or upgrade, keeping the last N per node.
- **Controller:** an online SQLite backup (`VACUUM INTO`) plus the vault and certificates, as an encrypted tarball. Scheduled, and restorable on a new host.
- **Site export/import** as JSON (secrets optional and encrypted), for migration or config-as-code.

## Replace / RMA / decommission [M7.4]

- **Replace:** mark the old node as failed, adopt the new device (any model), and map role, name and overrides onto it. Radios are remapped by band. Then apply.
- **Factory reset & re-adopt:** a guided flow.
- **Decommission:** remove the node from the mesh, wipe the uniwrt footprint, and optionally run `firstboot`.
