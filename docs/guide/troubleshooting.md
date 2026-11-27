# Troubleshooting

When something on your network misbehaves, you can usually find out why without leaving uniwrt. Each device has a **Tools** tab for tests, tables and packet captures, and a **Terminal** tab with a root shell. Each client has a **Doctor** tab. **Settings** has a support bundle to attach to a bug report. The end of this chapter covers problems people commonly hit and what to do about them.

## Tools

![A device's Tools tab, ready to run a test](https://robertzas.github.io/uniwrt/guide/images/device-tools.png)

Open a device under **Devices** and pick its **Tools** tab. Tests run on the device itself, and their output streams to the page live. This works in observe mode too: none of these tools change the device's configuration.

- Only admins run tests and capture packets. Viewers can follow runs and read the tables.
- A device that is offline can't run anything until it answers again.

### Run a test

The **Run a test** card offers these tools:

| Tool | What it does | Fields |
|---|---|---|
| **Ping** | Pings a host from the device. | **Host**, **Requests** (1 to 100), **Address family** |
| **Traceroute** | Shows each hop from the device to a host. | **Host**, **Address family** |
| **DNS** | Looks a name up, through the device's own resolver or another server. | **Name**, **Record type**, **Server** |
| **HTTP** | Checks whether a web address answers, and how fast. | **URL** (`http://` or `https://`) |
| **Throughput** | Measures speed with iperf3 between the device and another device or the controller. | **Far end**, **Direction**, **Seconds** (1 to 60), **Parallel streams** (1 to 8) |
| **Cable** | Tests an Ethernet cable on one of the device's ports. | **Port** |

1. Pick the tool.
2. Fill in its fields. A value the device would refuse is marked before you send it.
3. Click **Run**.

Notes on some tools:

- **Throughput**: pick **This controller** or another adopted device as the **Far end**, and **Upload** (the device sends), **Download** (the device receives) or **Both**. Device to device, it measures a mesh or wired link. Device to controller, the device must reach the controller's iperf3 port (5201 by default): open it in the host's firewall, or publish it from a bridged container. A device takes part in one throughput test at a time.
- **Cable**: the port's link drops while the test runs, for up to a minute. Don't test the port the device's own uplink uses unless you can lose it for that long.
- A test that needs a tool the device lacks says which package to install. See [Install optional tools](#install-optional-tools).

### Follow a run

The **Output** card shows the run: its lines as they arrive, its state (**Running**, **Succeeded**, **Failed** or **Stopped**), who started it and how long it took. **Stop** ends a run early. For a throughput test, the card also shows the rate in each direction, a chart of each second's rate, and the sender's retransmits.

**Recent runs** lists the device's runs; pick one to show it again. The controller keeps runs in memory for 15 minutes, and forgets them all when it restarts.

### Install optional tools

OpenWrt's images leave out tcpdump, iperf3 and the full ethtool. The **Optional tools** card shows whether each is installed: **Packet capture** (`tcpdump-mini`), **Throughput tests** (`iperf3`) and **Cable tests** (`ethtool-full`, which replaces `ethtool`).

1. On the **Optional tools** card, click **Install** next to the tool.
2. Wait until it shows **Installed**.

- Installing works only on a managed device. In observe mode the card is read-only.
- Only admins install tools.

### Read the device's tables

The **Tables** card reads tables from the device when you open them. Everyone can read them, in observe mode too.

- **Neighbors**: the neighbor cache (ARP and IPv6 neighbors). Each address shows its MAC, the client it belongs to, the network device and the state.
- **Routes**: the routing tables. Pick a table, or **All tables**.
- **Connections**: the connections the firewall tracks, busiest first, with NAT translations and traffic. The card also shows how many the kernel tracks against its limit.

Type in **Filter** to keep only the rows that match, pick **Both**, **IPv4** or **IPv6**, and click a column header to sort. **Refresh** reads the table again.

### Capture packets

The **Packet capture** card records traffic on the device with tcpdump and saves it as a `.pcap` file you can open in Wireshark. The capture leaves out the controller's own SSH session with the device.

1. Pick the **Interface**, or **Every interface**.
2. Optionally type a **Filter**, such as `port 53` or `host 192.168.1.20`. Leave it empty to capture everything.
3. Set **Seconds** (up to 3,600), **Size limit (MiB)** (up to 256) and **Bytes per packet** (empty for whole packets).
4. Click **Capture**. The card shows the packets and size so far.
5. Click **Stop and save**, or wait for the time or size limit. The browser saves the file.

- The capture keeps running while you look at other tabs, and saves when it ends. Leaving or reloading the device's page ends it.
- One capture runs per device at a time.
- It needs `tcpdump-mini` on the device. Only admins capture packets.

## Terminal

The **Terminal** tab opens root's login shell on the device, over the controller's SSH connection. You don't need an SSH client or the device's password.

1. Open the device and pick its **Terminal** tab.
2. Click **Open terminal**. The header shows **Connecting…**, then **Connected**.
3. When you are done, click **Disconnect**, or type `exit`. **Reconnect** opens a new shell.

> [!WARNING]
> Changes you make in the terminal bypass uniwrt's configuration. On a managed device, they show as drift, and the next apply can overwrite them. Make lasting changes through uniwrt's pages instead. See [Devices and adoption](devices.md).

- Only admins can open a terminal. Opening and closing it are recorded in the audit log; what you type isn't.
- The terminal keeps going while you look at other tabs of the same device. It ends when you leave the device's page.
- A terminal with no activity for 30 minutes ends by itself.
- At most 3 terminals can be open per device.

## Doctor

Each client's page has a **Doctor** tab that looks at its Wi-Fi, address and DNS and lists the problems it finds, worst first, with fixes to try. Pick a **Range** (**1 hour**, **24 hours** or **7 days**). It spots weak signal, a client holding on to a far access point, wrong Wi-Fi passwords, DHCP and DNS failures, a client stuck on 2.4 GHz, trouble with 802.11r and clients that keep disconnecting.

Start here when one device in your home has a bad connection. [Clients](clients.md) covers the tab in full.

## Support bundle

**Settings**, **Support** downloads one file to attach to a bug report. It holds the controller's versions, the site's latest configuration, the devices and their status, the last 24 hours of events and logs, the WAN monitor, the controller's own log, and from each online device its UCI configuration and a snapshot of its status, addresses, routes, neighbors, `dmesg` and `logread`.

1. Go to **Settings** and pick the **Support** tab.
2. Click **Download the bundle**. Reading every online device can take a minute.
3. Open the file and look it over before you share it.

Passwords and keys are masked, but the logs and configurations still name your devices, networks and clients.

- Only admins can download the support bundle.

## Common problems

### The browser warns about the certificate

The controller's certificate comes from its own local CA, which browsers don't know.

1. [Download and trust the CA](install.md#trust-the-controllers-certificate) on each device you use.
2. If the warning stays, check that the name or address you typed is one the certificate covers: `https://<host>:8443/api/v1/tls` lists them. If it isn't, add it to `UNIWRT_TLS_NAMES` and restart the controller. Look for a log line that says the certificate `cannot cover` a name: the CA can't sign names outside the local domains that weren't configured on the first start. Use a name under `lan`, `home.arpa`, `internal` or `local` instead. See [Names the certificate covers](install.md#names-the-certificate-covers).

### Setup is closed

The **Setup is closed** screen means the 10-minute window for creating the first admin ran out.

1. Restart the controller: `docker restart uniwrt`, or restart its service.
2. Click **Check again**, then create the account within 10 minutes.

### The controller doesn't answer

The sign-in page shows **Can’t reach the controller**, or the top bar shows **Unreachable** and live views pause.

1. Check that it runs: `docker ps`. The container's health shows there too.
2. Read its log: `docker logs uniwrt`. A start that fails says why on its last lines.
3. Check that you use `https://` and port 8443, from a computer that can reach the host.

The page reconnects by itself once the controller is back.

### The controller doesn't start

Its log says why:

- `the master key … is missing`: the database holds secrets, but the key file is gone. Put `master.key` back, or point `UNIWRT_MASTER_KEY_FILE` at it. See [The master key](install.md#the-master-key).
- `cannot create the data directory` or a permission error: the data directory isn't writable by the controller's user. For a host directory used by the container, `chown 65532:65532` it.
- `cannot listen on` an address: another program uses the port. Stop it, or pick other ports with `--https-listen` and `--http-listen`.

### Too many sign-in attempts

After 5 wrong passwords in a row for one username, sign-in waits 1 minute, doubling up to 15 minutes. After 10 failures from one computer in 15 minutes, that computer waits too. The message says how many seconds are left.

### The admin password is forgotten

Run `reset-admin` on the controller's host. The controller can keep running:

```sh
docker exec uniwrt /uniwrt reset-admin
```

Without Docker, run `uniwrt reset-admin --data-dir <the controller's data directory>`. With several admins, name one: `reset-admin alice`.

It prints a new random password once, such as `qde9k-fdxwk-gjpc9-3getq`, and signs that admin out everywhere. Sign in with it. If sign-in asks you to wait, earlier wrong attempts are still counted: wait as it says.

### A device is offline or stuck connecting

A device's status says why it isn't online: **Connecting**, **Unreachable** with the error, **The node agent failed** with the error, **No fresh data from it** and so on. The controller keeps retrying by itself and shows when it tries next.

1. Check the device has power and its uplink: open the **Topology** page, or ping the device's address from the gateway's **Tools** tab.
2. If the gateway is offline, every device behind it is too. Fix the gateway first.
3. Look under **Insights**, **Events** for what happened just before (a restart, a firmware upgrade, a mesh link lost).
4. If the device answers but stays **Unreachable**, check that SSH runs on it and that nothing blocks the controller's address.

### A device's SSH host key changed

The status says **Its SSH host key changed. uniwrt stopped connecting until you confirm it.** uniwrt pins each device's host key at adoption and refuses to talk to a device that presents another one, in case something is impersonating it.

If you reset or reflashed the device yourself, take it over again with its new key:

1. Open the device and pick its **Config** tab.
2. On the **Replace or remove** card, click **Replace…**.
3. Give the device's address and root password, compare the fingerprint it shows with the device's own key, and click **It matches, replace**.

The device keeps its name, settings and history, and comes back in observe mode. If you didn't touch the device, find out why its key changed before you do this.

### A change rolled back

When an apply ends with **The changes were rolled back**, a device failed its checks after the change, so every device went back to what it had. Nothing is left half-applied. This is the safety net working: each device applies a change with a timer armed and undoes it unless the controller confirms in time.

1. In the apply's progress, find the device marked **Failed** or **Rolled back**, and read why. [Review and apply](changes.md#review-changes) explains the steps.
2. Fix the setting that caused it, then click **Try again**.

**The changes applied only in part** means some devices confirmed before others rolled back. Keep the result, or apply the previous revision to go back. A controller restart in the middle of an apply rolls back the devices that weren't confirmed yet.

### You lost contact after renumbering the network

When an apply moves the network the controller sits on to a new subnet, the plan says so: the gateway keeps the old address beside the new one until the controller's host has an address on the new network, then drops it.

- The controller's host must get its address from the gateway's DHCP. A host with a static address can't follow, and the apply rolls back, safely.
- Wired devices move within a minute. Wi-Fi devices, and devices behind a switch, move once their lease renews. Reconnect your computer to hurry it.
- If the page stops updating, your computer or the controller got a new address. Reconnect, then open the controller at its new address, or at `https://<host name>.lan:8443` if your gateway's DNS knows the host.
- The certificate picks up the host's new address at its daily check. Restart the controller to add it at once.

### Logs don't arrive from a device

The controller follows each device's log over its SSH connection, so most logs need no setup. For devices that send syslog, check that port 5514 is published (bridge networking) and open in the host's firewall, and that `UNIWRT_SYSLOG_LISTEN` isn't `off`.

### The controller didn't come back after a restore

A restore from the web UI stops the controller so that it can start with the backup. It needs a restart policy to start again by itself: `--restart unless-stopped` for a container, `Restart=always` for a systemd service. Start it by hand otherwise; the restore happens on that start.
