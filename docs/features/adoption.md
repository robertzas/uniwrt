# Adoption & onboarding

Read this when working on the first-run wizard, discovery, adoption, node roles or ZTP.
Milestones: M1.7 (adopt existing nodes in observe mode), M4.3 (gateway bootstrap), M5.4 (ZTP, adoption port, one-liner).

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

1. In Devices → **Adopt device** (or ⌘K "Adopt a device", or a Discovered device's **Adopt**), enter the IP or hostname (`host:port` for another SSH port) and the root password, or leave the password empty if the controller's key is installed already or root has none (factory defaults: the job tries the key, then a login without a password). The wizard is a dialog over the Devices list (`/devices?adopt=1&host=…`, M1.8b, D-048) over `POST /api/v1/adopt/probe`, then `POST /api/v1/adopt` (D-042).
2. The controller reads the node's host key without logging in and shows its fingerprint, with the command that prints the node's own (`dropbearkey -y -f /etc/dropbear/dropbear_ed25519_host_key`); the user may also give a name and a role here. Adoption sends back the key the user confirmed; the node must present it, and it is pinned (trust on first use, D-032), so every later connection must present it too. The controller then logs in and checks, before changing anything: OpenWrt 24.10 or later (or a snapshot), 8 MiB of available memory and 1 MiB free in `/tmp` for the agent, an identity (below), and that the node is not adopted yet.
3. It appends the controller's public key to `/etc/dropbear/authorized_keys` unless it is there, then logs in with it. In observe mode, this is the only change it makes, except on a node whose root has no password (factory-default OpenWrt lets anyone on its LAN in as root over SSH's `none` method): over the key's login it then sets a random root password (six groups of four letters and digits) with `passwd`, sealed in the vault before it runs and kept as `node/<id>/root_password`, which admins can see (D-080). The password is stored (in the vault) only if the user opts in.
4. It pulls an inventory: the facts (board, system, capabilities, interfaces, radios…), the installed packages (from opkg's status file, or apk's database and world file, read without running the package manager), and a `sysupgrade -b -` backup streamed to the controller and sealed in its database. The node, its pinned key, facts, inventory and backup are recorded in one transaction, and the adoption is audited.
5. It starts watching the node (the ephemeral agent in `/tmp`), and live telemetry begins. The node shows an **Observed** badge. Its role is the one the user gave, or a gateway if a WAN-like interface (`wan*`, PPP, cellular) is up with a default route, otherwise an access point.
6. Later, "Manage this node" triggers a brownfield import (M3.5) and a review of the rendered diff, which should be empty or nearly so. The node then becomes **Managed**, one package at a time (network, wireless, …).

Each adoption is a job (`GET /api/v1/jobs/{id}`, the WS topic `jobs`): its steps (connect, check, install key, log in with the key, set a root password (skipped where root has one), inventory, backup, record, watch) with their state and detail, and on failure a code the UI acts on (`unreachable`, `host_key_mismatch`, `auth_rejected`, `unsupported`, `already_adopted`, `identity_mismatch` (a ZTP device that isn't the one that called home, Path B), `node_failed`, `interrupted`, `internal`). Jobs are history: a controller restart fails the ones that were running (D-042).

The wizard shows the steps live, then "Open the device" (or "Adopt another"). A failure is explained by its code, with the action that fits: `auth_rejected` returns to the password (cleared), `unreachable` and `unsupported` to the address, `host_key_mismatch` reads the key again for a new confirmation, `already_adopted` only closes, and the rest retry. Closing the dialog leaves the job running; the device appears in the list when it is recorded.

Batch mode (M1.7b, D-044): once the gateway is adopted, the other nodes appear under Discovered (below), each with the host key its probe read. The user checks the fingerprints, picks the nodes, and gives one password: `POST /api/v1/adopt/batch` checks every node first, then starts one adopt job per node (refusing only the nodes a job runs for already). At most 4 adoptions run at once; the others wait. In the UI, the Discovered card under the Devices list (polled every 15 s, every 2 s while probes run) has a checkbox per device with an address and a host key; "Adopt n selected" opens a dialog with every fingerprint and one password, then a progress bar per device, its failure explained, and the devices refused (D-048).

## Path B: zero-touch image (recommended for new devices) [M5.4]

1. Devices → Add new device: search for the model in OpenWrt's profile catalog, pick a role, and uniwrt asks ASU for a build: target, profile and version, the role's package set, and a first-boot `uci-defaults` script (ASU: M5.4 b3). Until then, `GET /api/v1/ztp/script?role=ap|bridge|gateway` (admins, M5.4 b1) downloads the script (`99-uniwrt`) for the OpenWrt Firmware Selector's "Script to run on first boot (uci-defaults)" or an image builder's `files/etc/uci-defaults/`.
2. The user flashes the image (vendor web UI, TFTP or OpenWrt sysupgrade), following model-specific instructions linked from OpenWrt's Table of Hardware.
3. The first-boot script (`node-agent/ztp/first-boot.sh`, with the controller's values written in by `uniwrt_node::ztp`) **contains no secrets** (D-022). It:
   - turns the LAN into a DHCP client on all bridged ports, with no DHCP server (DHCPv4, DHCPv6 and RA off), so the device is safe to plug in anywhere; an AP or a bridge also joins its WAN port to the bridge, as its first apply would, so any port reaches the controller; the device resolves names through its lease's DNS server (`localuse 0`: its own dnsmasq keeps `.lan` local and drops private answers);
   - keeps Wi-Fi off;
   - installs the controller's **public** key, and gives root a random password that never leaves the device, so nobody logs in without the key (SSH or LuCI);
   - sets the hostname to `uniwrt-<last 4 MAC hex>`;
   - installs and starts `uniwrt-hello` (`node-agent/ztp/uniwrt-hello`, a procd service), which calls home with `POST /api/v1/ztp/hello` (MAC, board, model, hostname, release, its ed25519 host key, the role its image was built for) to `uniwrt.<domain>`, then the controller's address: the gateway's `uniwrt` record, else the IP the admin downloaded the script from; on the HTTPS port the admin used. It sends one a minute, backing off to 5 minutes while no controller answers, and removes itself once answered `adopted`. Names go over IPv4 (musl fails a lookup whose AAAA query a forwarder refuses). There is no certificate check: OpenWrt's mbedtls refuses the controller's CA (its critical name constraints), and the hello is public data (D-102).
4. **The pending adoption inbox** (`GET /api/v1/ztp/pending`, admins): each device by MAC, with its latest hello, the address it came from, its host key's fingerprint and its latest adoption job. The hello is unauthenticated, so it is checked (a unicast MAC, an OpenSSH host key, short printable strings, a 4 KiB body), limited to 20 per 10 minutes per client (IPv6: per /64), and the inbox keeps the 64 most recently seen devices. A hello from an adopted node's MAC is answered `adopted`; one while its job runs, `adopting`.
   - The owner clicks Adopt (`POST /api/v1/ztp/pending/{mac}/adopt`, with an optional name and role; a job), or dismisses it (`DELETE /api/v1/ztp/pending/{mac}`; it comes back at its next hello).
   - Or **pre-approves the device by MAC** before it calls home (`PUT /api/v1/ztp/approvals/{mac}`, with an optional name and role; `DELETE` withdraws it): its first hello uses the approval up and starts the adoption. Approvals are single-use and expire after 7 days (D-102). A failed adoption leaves the device in the inbox.
   - The adoption is Path A's job with the controller's key (the image trusts it) and the host key from the hello pinned. Before anything changes, the node must be the hello's board, and the hello's MAC must be its identity or one of its devices' (`identity_mismatch` otherwise). Root's password, the image's unknown one, is replaced with one the vault keeps. Approvals, withdrawals, dismissals (`ztp`) and adoptions (`adopt`, by the approver for a pre-approval) are audited.
   - Later (M5.4 b2): the inbox in the UI, and the controller pushing secrets (root password, mesh key, Wi-Fi) and the full config as a transaction.
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

## Path E: first run with a factory-default gateway [M4.3]

The controller host sits behind a fresh OpenWrt gateway and has a 192.168.1.x address.

1. First-run wizard: create the admin account, then "Set up your internet router". The controller sees that its default gateway runs OpenWrt with passwordless root and shows the model: `GET /api/v1/bootstrap/gateway` (admins, M4.3a1) probes the controller's IPv4 default gateway (from `/proc/net/route`) and 192.168.1.1 (where a container on a bridge network still reaches it through its host), each where a dropbear banner answers: its host key, then a login without a password, which only a root without one accepts, reading the release (`/etc/openwrt_release`, with why an old one is unsupported), the model and the hostname (`ubus call system board`), and whether a node is adopted at that address already. Nothing changes on it. Adopting it is `POST /api/v1/adopt` with the key confirmed and an empty password (Path A, step 3: the key, then a random root password).
2. The wizard (M4.3b, D-082) is a full page at `/setup`, outside the shell: it opens after the admin account is created, and from the dashboard's welcome and ⌘K while no gateway is adopted. Leaving it is safe at any step: nothing changes on the router before the apply.
   - **Router:** the router found (model, release, address, host key to confirm), or one at an address the user names (`POST /api/v1/adopt/probe`). A factory-default one is adopted with one click; a configured one asks for its root password and what to do: **start fresh** (go on; the review shows what is replaced) or **keep its configuration** (adopt it in observe mode, then the brownfield import, lifecycle.md). The wizard adopts it as the gateway (`role`), since nothing else says so before it has internet, and turns management on.
   - **Internet:** the ISP wizard's form (wan.md); the test on the router is optional.
   - **Addresses:** the site's space and the router's address; the main network is the /24 around it, with the pool at .100–.249 (network.md). The step tells when the router moves.
   - **Wi-Fi:** one WPA2/WPA3 network on the router's 2.4 and 5 GHz radios, in the main network, and the country; skipped on a router without radios, and optional (M5.1 brings the editor).
   - **Review:** the choices in words; "Check with the router" saves them as one revision (the gateway managing `system`, `network`, `wireless`, `dhcp` and `firewall`) and reads the apply's plan, so the impact, the renumbering and anything the router refuses show before Apply; then the apply, live. A WAN that fails leaves the rest applied and leads back to the Internet step.
3. **Make-before-break renumbering.** Add 10.0.0.1 alongside 192.168.1.1 → switch DHCP to the new subnet with short leases → bounce the LAN ports to force renewals → wait for the controller host to reappear on 10.0.x.x → reconnect via 10.0.0.1 → remove the old subnet → confirm. A container with bridge networking follows its host transparently. Every apply that moves the controller's network does this (M4.3a2, D-081; lifecycle.md, Make-before-break).
4. WAN changes go last, in their own transaction. If internet health checks fail before the dead-man timer expires, only the WAN change rolls back; the LAN changes stay (the same in every apply whose WAN changes among other things).

## Roles

| Role | Purpose | Key config |
|---|---|---|
| `gateway` | Primary router: WAN, routing/NAT, firewall, DHCP/DNS, batman gateway; can also act as an AP | WAN, layer 3 for all networks, services |
| `ap` | Access point; backhaul `auto` (batman on every uplink, wired or mesh) | Dumb AP (no DHCP, DNS or firewall), bridges networks/VLANs, management IP from a DHCP reservation |
| `bridge` | Wireless client bridge for wired devices (for example, a TV) | Mesh or STA uplink; LAN ports bridged |

Future roles: `switch` (an OpenWrt managed switch) and secondary gateways (multi-WAN at another node).

**Changing a role (D-101, M5.4 a2):** Config tab › Role, or `PUT /api/v1/nodes/{id}/role` (admins, audited). Only the stored role changes; the engine rebuilds the topology with it and the `fleet` and `node:<id>` topics show it at once. The next apply renders what it means, and the review shows it first. A site keeps one gateway (making a second is refused: demote the first), a managed gateway keeps its role (its routing would go with it), and nothing changes while an apply or a job for the node runs. **Dumb-AP conversion:** a router adopted or made an access point loses at the next apply its DHCP server, firewall and WAN; its WAN port joins the bridge, and it takes its address from the gateway's DHCP (its reservation). The import drops those parts by design instead of keeping them raw (lifecycle.md, Brownfield import). Connect one of its ports to the site's network before applying: the apply reconnects to it at its new address.

## Identity, credentials, trust

- **Identity** is the label MAC (from `board.json`; else the MAC of the device behind `lan`, else the lowest globally administered Ethernet MAC) plus the board name, so it survives IP changes. A host-key mismatch quarantines the node and raises an alert. The admin can re-trust it, for example after a reflash.
- **Controller key:** ed25519, generated on first run and kept in the vault. A rotation job adds the new key → verifies it → removes the old one.
- **Root password:** randomized per node when management starts, stored in the vault and viewable by an admin for emergencies. Optional hardening: key-only SSH and dropbear bound to the management network.
- **Management addressing:** every node gets a DHCP reservation in the infrastructure range (10.0.0.2–10.0.0.31 by default). The controller keeps reservations in sync, so nodes have stable addresses without static configuration.

## Discovery (once the gateway is adopted)

`GET /api/v1/discovered` (M1.7b, D-044) reports the unadopted devices that look like OpenWrt nodes. It waits up to 3 s for new probes, and says `scanning` while some still run.

- **Sources** (from every adopted node's latest facts): DHCP leases, IPv4 neighbors on non-WAN interfaces (this finds APs with static addresses), and batman-adv meshes. A mesh originator is mapped to addresses through the wired clients it announces (batman's global translation table), which include the node's own bridge. The adopted nodes are left out, by every MAC and IP address in their facts. So is an originator that announces one of their MACs, so that a node adopted moments ago, which has no facts yet, does not reappear.
- **Probes:** every candidate address gets a TCP connection to port 22 that reads the SSH banner (no handshake). Dropbear servers, and hosts named `OpenWrt` or `uniwrt-*`, also have their host key read (a handshake that stops at the key). A probe never tries to log in. At most 4 probes run at once, and each result holds for 10 minutes. Demo mode probes the simulated nodes (below).
- **Reported:** dropbear servers, hosts with those names (even when unreachable), and every mesh originator none of whose devices is reported, as a node with no known address.

Later sources: LLDP neighbors. ZTP hellos have their own inbox (Path B).

## Demo mode (D-048)

`serve --demo` adopts the simulated gateway and every access point but the last, which the gateway's leases and mesh show under Discovered. The demo's dial reaches the simulated nodes as SSH would: a dropbear banner, a host key made at start, the root password `demo` (another is rejected; without one, the node must trust the controller's key), and the commands adoption runs, answered by the simulated node (its release, `authorized_keys`, its package database, a small backup). Each answer takes about as long as over a LAN, so a whole adoption takes about 5 s. Other addresses refuse the connection at once. The adopted node joins the engine on its simulated link.

`serve --demo --demo-factory` is the first run instead (Path E): one factory-default gateway at 192.168.1.1 (root without a password, OpenWrt's own `/etc/config`, an untagged DHCP WAN that the simulated ISP, on VLAN 201, doesn't answer), no user and nothing adopted. Create the admin, and the wizard sets the router up; the simulated LAN follows the renumbering.
