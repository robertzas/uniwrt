# Devices and adoption

The **Devices** page lists every router and access point uniwrt manages, with live status. This chapter covers every way to adopt a device, how to switch a device from observe mode to managed, and each tab of a device's page: restarting it, its LEDs, replacing or removing it, and hardening its SSH server.

## Devices

![The Devices page: a gateway and two access points online in observe mode, and an access point discovered but not adopted](https://robertzas.github.io/uniwrt/guide/images/devices.png)

The list shows each adopted device with its **Name**, **Status**, **Model**, **IP address**, **Firmware**, **Clients**, **CPU**, **Memory** and **Uptime**. Status is **Online**, **Degraded** or **Offline**; a device in observe mode also has an **Observe** badge. Click a row to open the device's page.

Below the list, the **Discovered** card shows OpenWrt devices your adopted devices see, and admins get the **Waiting for adoption** card.

While devices are adopted but the site is still the defaults, admins see **The site is still the defaults** with **Import from the devices**. Import before you manage them (see [Adopt an existing network](getting-started.md#adopt-an-existing-network)).

- Only admins adopt devices. Viewers see the list and the **Discovered** devices, without the adoption controls.

### To adopt a device by address

Use this for a device that already runs OpenWrt and that the controller can reach over SSH.

1. Press **Adopt device**. You can also run **Adopt a device** from the command palette (Ctrl+K, or Cmd+K on a Mac).
2. Enter the device's **Address**: its IP address or host name, such as `192.168.1.2`. Add `:port` for another SSH port.
3. Enter its **Root password**. Leave it empty if root has no password, or if the device trusts uniwrt's key already. Check **Keep the password in the vault** to let admins look it up later.
4. Press **Continue**. uniwrt reads the device's SSH host key without logging in.
5. Under **Confirm the device's identity**, compare the fingerprint with the device's own. To print it, run the command shown on the device.
6. Optionally give a **Name** (the hostname is used otherwise) and a **Role**: **Detect**, **Gateway**, **Access point** or **Bridge** (a wireless bridge that joins the mesh and bridges its Ethernet ports). **Detect** makes it a gateway if it has a WAN uplink with the default route, otherwise an access point.
7. Press **It matches, adopt**, and follow the steps.

uniwrt pins the host key you confirmed: it will only ever talk to a device that presents it. When adoption succeeds, press **Open the device** or **Adopt another**. You can close the dialog while it runs; the device appears in the list once it is recorded.

### What adoption changes

Adoption installs uniwrt's SSH key, reads the device's facts and packages, takes a configuration backup and starts watching the device. If root had no password, uniwrt sets a random one that the vault keeps. Nothing else changes: the device is in observe mode.

A device needs OpenWrt 24.10 or later, 8 MiB of free memory and 1 MiB free in `/tmp`.

### If adoption fails

The dialog explains the failure and offers the action that fits:

- **The device cannot be reached**: check the address, that the device is on, and that SSH listens. **Check the address** goes back.
- **The device presented another key**: if it was reset or reflashed, **Read the key again** and compare.
- **The device rejected the login**: **Change the password** goes back with the password cleared.
- **This device cannot be adopted**: it lacks memory, space in `/tmp` or a MAC address to know it by.
- **This device is adopted already**: it is in the list.

### Unsupported releases

uniwrt is built and tested for OpenWrt 24.10 or later. A device on an older release, another distribution, or one without a release name fails with **This release isn't supported**.

A device whose files name no release is taken for the release its kernel belongs to (Linux 6.6 is 24.10's, 6.12 is 25.12's). The check and the device's **Firmware** row then say, for example, *OpenWrt 24.10 (inferred from Linux 6.6.93)*. Updates still need the release the device names.

To adopt it anyway, press **Adopt anyway** in the dialog (several devices: **Adopt these N anyway**; the first-run wizard: **Adopt it anyway**). The other checks still apply. Its page then shows an **Unsupported** badge next to **Firmware** in **Details**, and some features may not work on it.

## Discovered

Once your gateway is adopted, this card lists the OpenWrt devices your adopted devices see that are not adopted yet: from DHCP leases, LAN neighbors and the mesh. It shows each device's address, where it was seen, whether SSH answers, and its host key. uniwrt only reads the SSH banner and host key; it never logs in. **Scanning** shows while it probes.

A device seen only on the mesh shows **No address known**. Adopt it by address once you know it.

To adopt one device, press its **Adopt**. The adoption dialog opens with its address filled in.

### To adopt several devices at once

1. Check the box of each device. Only devices with an address and a host key can be checked.
2. Press **Adopt N selected**.
3. Compare each fingerprint with the device's own key.
4. Enter the **Root password** they share (empty if they trust uniwrt's key already), and press **Adopt N devices**.

Each device gets its own progress bar and, on failure, the reason. Devices that couldn't start are listed under **Not started**.

## Waiting for adoption

Admins see this card on **Devices**. New devices set up for uniwrt call home here, and factory-default ones plugged into the gateway's adoption port show up here. Each row shows the device's hostname, model, MAC address, when it last called, the role it was built for, its address and its host key fingerprint.

### To adopt a device that called home

1. Press **Adopt** on its row.
2. Compare the **Host key fingerprint** with the device's own, and check the model and release.
3. Optionally set a **Name** and a **Role**: **As built**, **Access point**, **Bridge** or **Gateway**.
4. Press **It matches, adopt**.

uniwrt logs in with its own key, which the device trusts, and replaces the root password with one the vault keeps. The row shows the progress. On failure, it shows the reason with **Try again**. **Dismiss** removes a device from the card; it shows up again the next time it calls home.

### Pre-approved

A pre-approved device is adopted as soon as it calls home, without a click.

To pre-approve a device:

1. Press **Pre-approve a device**.
2. Enter its **MAC address**, as printed on its label.
3. Optionally set a **Name** and a **Role**.
4. Press **Pre-approve for 7 days**.

Each approval is used once and expires after 7 days. The **Pre-approved** list shows when each expires and who made it; **Withdraw** cancels one.

### The adoption port

If your site has an adoption port, a factory-default OpenWrt router plugged into it shows up here too, marked **On the adoption port**. The port is an isolated gateway port, so the new router's own DHCP server can't disturb your network. The card's first line names the port, or links to where you set one up.

To set up the adoption port, open the gateway's page, then its **Ports** tab. On the **Adoption port** card, press **Edit**, pick the **Port** and **The gateway's address there** (192.168.1.250 unless you pick another host of 192.168.1.0/24 but 192.168.1.1), then **Save**, review and apply. The port stops carrying your networks, and its row on **Ethernet ports** says **Adoption port**. Pick **None** to give it back.

- The list offers the gateway's LAN ports. The WAN port and the ports of a switch chip (swconfig) can't be the adoption port, and neither can the backup line's port.
- While the site has an adoption port, no network may use 192.168.1.0/24: renumber such a network first.

To adopt a router on the adoption port:

1. Plug the new router's LAN port into the gateway's adoption port, and power it on. It appears within a few minutes.
2. Press **Adopt**. Pick **Access point** or **Bridge**. A device marked **Root password set** needs its **Root password**.
3. Press **It matches, adopt**, and keep the device plugged in. The card shows its steps under **On the adoption port**.
4. When it says the device is adopted and set up, unplug it and place it. It joins through the mesh or any Ethernet port and keeps its address.

- One factory-default device at a time: they all answer at 192.168.1.1.
- A device adopted this way is managed at once.

## Set up a device

Press **Set up a device** on the **Waiting for adoption** card. Every method gives the device uniwrt's key, a random root password, a DHCP client on all its ports, no DHCP server and no Wi-Fi. The device then calls home and appears under **Waiting for adoption**. Nothing it receives contains secrets.

### One-liner

For a device that already runs OpenWrt.

1. Plug the device's WAN port into your network.
2. On the **One-liner** tab, choose a **Role** (**Access point** or **Bridge**) and optionally a **Name**.
3. Press **Make a command**, then **Copy**.
4. Open a shell on the device (SSH or the console) and paste the command.

The command runs the script only if its SHA-256 matches, so nobody on the network can swap it. It works once, within 15 minutes. The device prints its host key, then reloads its network, which ends your session. The dialog follows the command, and once the device calls home it is adopted. **Withdraw** cancels an unused command.

- The one-liner doesn't set up a gateway.

### Build an image

For a new device, especially one still on its vendor's firmware.

1. On the **Build an image** tab, choose the **OpenWrt release**.
2. Find the **Model**: search by vendor or model.
3. Choose a **Role**: **Access point**, **Bridge** or **Gateway**.
4. Press **Build the image**. OpenWrt's public build service (ASU) builds it; this takes a minute or more.
5. Download the **Factory image** (for the first flash, from the vendor's firmware) or the **Sysupgrade image** (for a device already running OpenWrt). Each has its SHA-256.
6. Flash it, following **How to flash this model**, and plug any of its ports into your network.

ASU keeps built images, so you can close the dialog and build again later. To use another ASU server, or none, start the controller with `--asu-url` (or `UNIWRT_ASU_URL`); `off` turns it off.

### First-boot script

For an image you build yourself. Choose a **Role**, press **Download the script**, and add it as "Script to run on first boot (uci-defaults)" in the OpenWrt Firmware Selector, or as `files/etc/uci-defaults/99-uniwrt` in the Image Builder.

## Device page

![The gateway's page on its Overview tab: its stats, radios, details and performance charts](https://robertzas.github.io/uniwrt/guide/images/device.png)

A device's page shows its name, status, role, model and management address. **Events** and **Logs** open those pages filtered to this device. When something is wrong, a card under the header says why, such as **Its SSH host key changed. uniwrt stopped connecting until you confirm it.** Admins can trust the new key from there: see [A device's SSH host key changed](troubleshooting.md#a-devices-ssh-host-key-changed).

The page has seven tabs: **Overview**, **Ports**, **Mesh**, **Config**, **Tools**, **Terminal** and **UCI**.

### To restart devices

- One device: press **Restart** on its page, then confirm.
- Several: on **Devices**, press **Restart…**, check the **Devices to restart**, and press **Restart N devices**.

Devices restart one at a time, farthest from the gateway first and the gateway last. Each is back before the next goes down, and the first device that doesn't come back stops the restarts. You can close the dialog; the top bar shows the restarts. While the gateway restarts, the whole network has no internet.

- Only managed devices that answer can be restarted. Only admins restart devices.

## Overview

The **Overview** tab shows live numbers (**CPU**, **Memory**, **Temperature**, **Uptime**, **Wi-Fi clients**, **Load**), the **Radios** with their channel, width and clients (admins press **Edit** beside a radio to fix its channel, width and power or turn it off; see [Radios](wifi.md#radios)), the device's **Details**, **Performance** charts, its **Wi-Fi clients**, and the clients it steered in the last 24 hours (**Steering**).

## Ports

The **Ports** tab, **Ethernet ports**, shows each port's link and which networks it carries: a trunk carries the default network untagged and every VLAN tagged; an access port carries one network. It also says whether the mesh backhaul may use the port. Press **Edit** to change a port, then **Save**. See [Networks](networks.md).

- Ports apply only on a device whose network package is managed. Only admins change ports.
- The gateway's **Ports** tab also has the **Adoption port** card (see [The adoption port](#the-adoption-port)).

## Mesh

The **Mesh** tab shows the **Path to the gateway** (what this device depends on, and what loses the gateway if it goes down), its **Mesh links** with their health, drops and 24-hour quality, and **Failed cables** that failed the backhaul probe. See [Wi-Fi and mesh](wifi.md).

## Config

The **Config** tab holds the device's settings and actions, one card each. Most actions need an admin.

### Role

Shows what the device's role means. To change it, press **Change role**, pick the new role and press **Change role** again. Only the stored role changes; the device converts at the next apply, which the review shows first.

- A site keeps one gateway: make the current one an access point first.
- A managed gateway keeps its role: turn its management off, or make another device the gateway, first.
- When a router becomes an access point, it loses its DHCP server, firewall and WAN. Connect one of its ports to the site's network before you apply.

### Management

This card switches the device between observe mode and managed.

To turn management on:

1. Press **Turn management on**, then **Turn on**. uniwrt installs its transaction tool and boot guard, a few kilobytes that survive firmware upgrades.
2. In **Managed packages**, check the packages uniwrt should own, and press **Save**. Nothing is chosen at first, so applies leave the device alone until you choose.
3. Press **Review changes** in the top bar, check the changes, and apply them (see [Review and apply](changes.md#review-changes)).

The card says which revision the device holds. To stop, press **Turn management off**, then **Turn off**: uniwrt removes its tools and stops applying changes, and the device's configuration stays as it is.

> [!WARNING]
> For a device you configured yourself, import its configuration before you choose packages (Settings › Import). A managed package is owned whole: an apply replaces what the site doesn't hold.

### Wi-Fi daemon, Firmware and Installed packages

- **Wi-Fi daemon**: **Install the full wpad** adds roaming suggestions (802.11v). See [Wi-Fi and mesh](wifi.md).
- **Firmware**: the release the device runs and the upgrades on offer. See [Updates](updates.md).
- **Installed packages**: the software on the device. **Add or upgrade…** and **Review changes…** change it at once; these changes aren't part of the site's configuration.

### SSH hardening

Controls how the device's SSH server lets people in. uniwrt logs in with its own key either way.

To harden SSH:

1. Check **Keys only**: passwords no longer log in over SSH, only keys. The root password still opens LuCI and the console.
2. Optionally check **SSH on** *network* **only**: the server listens on the management network alone. This needs the network package managed. On the gateway, uniwrt must log in from that network, or the apply refuses.
3. Press **Save**, then review and apply.

The apply checks that uniwrt can log in anew before it keeps the change; otherwise it rolls back. The card shows the server's current state under **Now**.

- It takes effect once the device is managed. Saving makes uniwrt manage the device's SSH server configuration.

### Root password

Admins can press **Show root password** to see the root password the vault keeps for this device, for emergencies. Each view is recorded in the audit log.

### Schedules

Sets how the device follows the site's restart and LED schedules (set in Settings › Site). Saved changes need no apply.

- **Restarts**: **The site's**, **Never** or **Its own**, with the next restart shown.
- **LEDs**: check **Turn its LEDs off with the site's schedule**. The card says how many of its LEDs are lit now.

- Only managed devices follow schedules.

### Backups

Configuration backups kept on the controller, sealed: at adoption, weekly, before every apply or upgrade, and on request. Press **Back up now**, **Download** a backup, or **Restore** one. A restore is a transaction that rolls back unless the device is as healthy afterwards.

- Only admins back up, download and restore. Restoring needs management on.

### Replace or remove

Admins use this card when a device fails, should start over from its defaults, or leaves the network. While other devices are adopted, the gateway offers only **Replace…**: remove the others first, or replace its device.

To replace a failed device with new hardware (any model):

1. Set up the new device with OpenWrt and connect it.
2. Press **Replace…**, enter its **Address** and **Root password**, and press **Continue**.
3. Compare the fingerprint and press **It matches, replace**.
4. When it succeeds, press **Review changes** to check the moved settings.

The new device takes over the record: name, role, settings, floor placement and history carry on. Radio settings follow their band, port settings the port's name. The new device is in observe mode: turn management on and apply.

To reset a device to its defaults, press **Reset…**, then **Reset**. `firstboot` erases its settings and restarts it at 192.168.1.1 with no root password. Its record waits for it, and the device page shows **Waiting for its device** with how it comes back: a uniwrt image calls home by itself; a stock image is adopted again at 192.168.1.1 or through the adoption port. **Stop waiting** ends the wait; the record stays, offline.

To decommission a device:

1. Press **Decommission…**.
2. Check **Also reset it to its defaults** to erase it with `firstboot`. Without it, the device keeps the root password uniwrt set, so note it down first: **Show root password** in the dialog reveals it.
3. Press **Decommission**.

uniwrt removes its tools, scripts and SSH key from the device, the device leaves the mesh, and its record, charts and backups are deleted. The timeline keeps the event.

To forget a device that can't be reached, press **Forget…**, then **Forget**. Nothing changes on the device; its record and the root password the vault keeps are deleted.

### Drift

On a managed device, **Drift** lists packages someone changed on the device after uniwrt confirmed them (in LuCI, with `uci`, or in an editor). For each, choose **Import** (take the device's version into the site), **Overwrite** (review an apply that rewrites it), **Acknowledge** (accept it until it changes again) or **Mute** (stop watching that package). See [Changes](changes.md).

## Tools

The **Tools** tab runs diagnostics on the device itself, with live output:

- **Run a test**: **Ping**, **Traceroute**, **DNS lookup**, **HTTP check**, **Throughput test (iperf3)** and **Cable test**. **Recent runs** keeps them for 15 minutes.
- **Packet capture**: captures with tcpdump into a `.pcap` file for Wireshark.
- **Optional tools**: installs tcpdump, iperf3 or ethtool when a test needs them (managed devices only).
- **Tables**: the device's **Neighbors**, **Routes** and **Connections**.

See [Troubleshooting](troubleshooting.md).

- Only admins run tests.

## Terminal

The **Terminal** tab opens root's shell on the device over the controller's SSH connection. Press **Open terminal**. Opening and closing it are audited; what you type isn't recorded.

- Changes made here bypass the site's configuration and show as drift. Only admins open a terminal.

## UCI

The **UCI** tab, the **UCI explorer**, shows every package in the device's `/etc/config`, read live. Secrets are masked. **Search the configuration** finds a name or value, and **Read again** reloads. Each package is marked **Rendered** (uniwrt writes it whole), **Patched** (the device's own file, patched by the site's overrides) or **Device's own** (untouched).

On a managed device, admins can **Edit** a section, **Add section** or **Delete section**. Each edit is saved as an override in the site and reaches the device at the next apply. **Drop** removes an override.

- The explorer is read-only for devices in observe mode and for viewers.
