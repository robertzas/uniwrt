# Adoption & onboarding

Read this when working on the first-run wizard, discovery, adoption, node roles or ZTP.
Milestones: M1.7 (adopt existing nodes in observe mode), M4.4 (gateway bootstrap), M5.4 (ZTP, adoption port, one-liner).

## Starting states we must handle

| Device state | How we can reach it | Path |
|---|---|---|
| Running OpenWrt and already configured (the owner's current 4 nodes) | SSH at a known IP, with the root password or a key | **A**: adopt existing |
| Stock vendor firmware (a new purchase) | Not at all; it has to be flashed anyway | **B**: ZTP image |
| Factory-default OpenWrt (fresh flash or after `firstboot`) | 192.168.1.1, root with no password, Wi-Fi off, DHCP server on LAN | **C**: adoption port (or B, or D) |
| Anything else, or an edge case | The user has a shell on the device | **D**: bootstrap one-liner |
| The gateway itself, factory-default, with the controller host behind it | The host got a 192.168.1.x lease from it | **E**: first-run gateway bootstrap |

The firewall on factory-default OpenWrt rejects input on WAN, so a fresh device can never be reached through its WAN port. Its LAN runs a DHCP server, so plugging that LAN into the home network would put a rogue DHCP server on it. Every path below avoids both problems.

## Path A: adopt existing (observe first) [M1.7]

1. In Devices → Adopt → "Existing OpenWrt device", enter the IP or hostname and the root password (or skip the password if the controller's key is already installed).
2. The controller reads the node's host key without logging in and shows its fingerprint. Once the user confirms it, the key is pinned (trust on first use, D-032), and every later connection must present it. The controller then logs in and checks the OpenWrt version (≥ 24.10), ucode modules, free RAM and flash, architecture and board.
3. It installs the controller's public key in `/etc/dropbear/authorized_keys`, tagged `uniwrt-controller`. In observe mode, this is the only change it makes. The password is stored (in the vault) only if the user opts in.
4. It pulls an inventory: `board.json`, `/etc/config/*`, the package list, radio capabilities, and a `sysupgrade -b` backup kept on the controller.
5. It starts the ephemeral agent, and live telemetry begins. The node shows an **Observed** badge.
6. Later, "Manage this node" triggers a brownfield import (M3.5) and a review of the rendered diff, which should be empty or nearly so. The node then becomes **Managed**, one package at a time (network, wireless, …).

Batch mode: once the gateway is adopted, the other nodes appear under Discovered (from DHCP leases and batman originators) for one-click adoption.

## Path B: zero-touch image (recommended for new devices) [M5.4]

1. Devices → Add new device: search for the model in OpenWrt's profile catalog, pick a role, and uniwrt asks ASU for a build: target, profile and version, the role's package set, and a first-boot `uci-defaults` script.
2. The user flashes the image (vendor web UI, TFTP or OpenWrt sysupgrade), following model-specific instructions linked from OpenWrt's Table of Hardware.
3. The first-boot script **contains no secrets** (D-022). It:
   - turns the LAN into a DHCP client on all bridged ports, with no DHCP server, so the device is safe to plug in anywhere;
   - keeps Wi-Fi off;
   - installs the controller's **public** key;
   - sets the hostname to `uniwrt-<last 4 MAC hex>`;
   - calls home with `POST /api/v1/ztp/hello` (board, MAC, host-key fingerprint) to `uniwrt.lan`, falling back to the controller's literal IP, retrying with backoff.
4. The device appears in the **Pending adoption** inbox. The owner clicks Adopt, or pre-approves the device by MAC. The controller connects over SSH and pushes secrets (root password, mesh key, Wi-Fi) and the full config as a transaction.
5. For wireless-only placement, adopt the device once on the bench over a cable. After it has the mesh credentials, place it anywhere within mesh range.

Why no secrets: ASU is a public build service, and build requests and results may be cached on third-party servers.

## Path C: adoption port (bench adoption of factory-default OpenWrt) [M5.4]

A gateway port picked by the user (for example, `lan4`) becomes an isolated `adopt` segment: bridge `br-adopt` with static address 192.168.1.250/24 (configurable, and never part of the site address space), its own firewall zone, no forwarding and no DHCP.

1. Plug the new router's LAN port into the adoption port and power it on.
2. The gateway's agent sees the link come up and 192.168.1.1 answer on SSH. The controller reaches the device through an SSH session tunneled via the gateway (`direct-tcpip`) and shows a device card (model, version, MAC).
3. The user picks a role. The controller sets the root password and key, uploads the agent, and applies a staging config as a transaction: DHCP client on bridged ports, no DHCP server, mesh credentials.
4. The gateway temporarily moves the adoption port into the site LAN, so the node gets its reserved address. The controller reconnects there and confirms the transaction. When the link goes down, the port returns to `adopt` mode.
5. The UI says "Unplug and place it." The node joins through the mesh or any Ethernet port.

If packages are missing (for example, full wpad or `kmod-batman-adv`), they are installed during adoption through temporary NAT for the adopt segment. Alternatively, reflash to a ZTP image for a consistent package set (recommended before the first rolling upgrade).
Limitation: one factory-default device at a time, because they all answer at 192.168.1.1.

## Path D: bootstrap one-liner [M5.4]

The UI shows a short-lived command to paste into any OpenWrt shell, for example `uclient-fetch -qO- 'https://<controller>:8443/b/<token>' | sh`. The token is single-use and expires after 15 minutes. The device must be able to reach the controller, for example with its WAN port plugged into the site LAN (outbound traffic is allowed by the default firewall). The script is the ZTP first-boot script plus the token, so the device shows up pre-approved in the inbox.

## Path E: first run with a factory-default gateway [M4.4]

The controller host sits behind a fresh OpenWrt gateway and has a 192.168.1.x address.

1. First-run wizard: create the admin account, then "Set up your internet router". The controller sees that its default gateway runs OpenWrt with passwordless root and shows the model.
2. ISP wizard (wan.md), then the address plan (10.0.0.0/16, network.md), then basic Wi-Fi, then a review screen.
3. **Make-before-break renumbering.** Add 10.0.0.1 alongside 192.168.1.1 → switch DHCP to the new subnet with short leases → bounce the LAN ports to force renewals → wait for the controller host to reappear on 10.0.x.x → reconnect via 10.0.0.1 → remove the old subnet → confirm. A container with bridge networking follows its host transparently.
4. WAN changes go last, in their own transaction. If internet health checks fail before the dead-man timer expires, only the WAN change rolls back; the LAN changes stay.

## Roles

| Role | Purpose | Key config |
|---|---|---|
| `gateway` | Primary router: WAN, routing/NAT, firewall, DHCP/DNS, batman gateway; can also act as an AP | WAN, layer 3 for all networks, services |
| `ap` | Access point; backhaul `auto` (batman on every uplink, wired or mesh) | Dumb AP (no DHCP, DNS or firewall), bridges networks/VLANs, management IP from a DHCP reservation |
| `bridge` | Wireless client bridge for wired devices (for example, a TV) | Mesh or STA uplink; LAN ports bridged |

Future roles: `switch` (an OpenWrt managed switch) and secondary gateways (multi-WAN at another node).

## Identity, credentials, trust

- **Identity** is the label/primary MAC (from `board.json`) plus the board name, so it survives IP changes. A host-key mismatch quarantines the node and raises an alert. The admin can re-trust it, for example after a reflash.
- **Controller key:** ed25519, generated on first run and kept in the vault. A rotation job adds the new key → verifies it → removes the old one.
- **Root password:** randomized per node when management starts, stored in the vault and viewable by an admin for emergencies. Optional hardening: key-only SSH and dropbear bound to the management network.
- **Management addressing:** every node gets a DHCP reservation in the infrastructure range (10.0.0.2–10.0.0.31 by default). The controller keeps reservations in sync, so nodes have stable addresses without static configuration.

## Discovery (once the gateway is adopted)

Sources: the gateway's DHCP leases (hostnames `OpenWrt` or `uniwrt-*`), batman originators, LLDP neighbors, rate-limited SSH banner probes of new leases, and ZTP hellos. Unadopted OpenWrt devices show up as **Discovered**, with one-click adoption (asking for credentials when needed).
