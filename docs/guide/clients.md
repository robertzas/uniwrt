# Clients

A client is anything that connects through your adopted devices: phones, laptops, TVs, printers, smart home gadgets, and wired machines. The **Clients** page lists every client the controller has seen in the last 30 days, online or not, and each client has its own page with its connection, roaming history, signal, traffic and a troubleshooting doctor. Everything updates live.

Clients you rename or block are kept even after 30 days away.

## Clients page

![The Clients page, listing every client with its connection, signal and traffic](https://robertzas.github.io/uniwrt/guide/images/clients.png)

Open **Clients** in the sidebar. Until a client connects, the page says **No clients yet** and suggests the next step (adopting a device, or setting up Wi-Fi).

At the top, four tiles sum up who is online and how well:

- **Online**: the clients online now, split into Wi-Fi and wired.
- **Wi-Fi experience**: the average [experience score](#experience-score) of the online Wi-Fi clients, and how many it averages.
- **Weak signal**: how many online clients have a signal below −75 dBm.
- **Experience by access point**: each access point with its number of Wi-Fi clients and their average score, the weakest first.

Below them, the **Clients** list shows how many clients are online and how many are known. Its columns are:

- **Client**: an icon for the kind of device, a dot that is green while it is online, its name, and below it the vendor, the hostname (when you gave it another name) and the kind. A shuffle icon marks a private (randomized) address, a ban icon marks a blocked client, and "+2 addresses" means the row groups several addresses of one device.
- **IP address**: its first address.
- **Connected to**: the access point with the Wi-Fi network and band, or the device and port for a wired client. For an offline client, where it was last, dimmed.
- **Signal**: for an online Wi-Fi client, with a dot that is green from −65 dBm, amber from −75 dBm, and red below.
- **Experience**: its experience score.
- **Traffic (down / up)**: what it moves now.
- **Connected**: how long it has been connected, or how long ago it was last seen.

Select a row to open the [client page](#client-page).

A client's name is the name you gave it, else the hostname it sent to the DHCP server, else its MAC address. The kind (phone, laptop, TV and so on) and the vendor are guessed from the hostname and the MAC address.

### Find a client

1. Type in the search box (**Name, IP, MAC or vendor**). Every word must match the client's name, hostname, MAC address, vendor or one of its IP addresses.
2. To narrow the list, select a filter: **All**, **Wi-Fi** (online over Wi-Fi), **Wired** (online on a cable), **Offline** or **Blocked**.

If nothing matches, the list says **No clients match.** You can also find any client by name, MAC or IP address from the [command palette](dashboard-topology.md#command-palette).

### Private addresses

Many phones and laptops use a different, random MAC address per network or over time. uniwrt marks such a client with **Private address** and groups the addresses of one device into one row when it can tell they belong together (by a hostname that isn't generic, or by the device's DHCP client ID). The grouped addresses share one name.

On a phone, each row of the list becomes a card, and the traffic and connected time are left out.

## Client page

![A client's page on its Overview tab, with its connection and experience](https://robertzas.github.io/uniwrt/guide/images/client.png)

The client page shows one client, live. At the top: its name, **Online** or **Offline**, a **Private address** badge for a randomized address, and a **Blocked** badge for a blocked client (hover over it to see since when). The line below shows the vendor, the kind, the MAC address and the IP addresses.

The buttons beside the name:

- **Events**: the Events page, filtered to this client. See [Insights](insights.md).
- **Rename**, **Fix IP**, **Wake**, **Kick** and **Block** (or **Unblock**): see [Client actions](#client-actions).

The page has five tabs: [Overview](#overview), [Timeline](#timeline), [Signal](#signal), [Traffic](#traffic) and [Doctor](#doctor). **All clients** above the name returns to the list.

- Viewers see the client page but not the action buttons; only **Events** is there.
- A MAC address that wasn't seen in the last 30 days shows **Client not found**.

## Overview

When the [Doctor](#doctor) finds an issue in the last 24 hours, a chip at the top names the worst one ("Weak signal and 1 more"). Select it to open the Doctor tab.

The tiles show the client's state now, with the last hour as a small chart where there are samples:

- **Experience**: its experience score.
- **Signal**: its signal strength, or its signal-to-noise ratio.
- **Rate (down)**: the Wi-Fi rate toward the client, with the rate up below.
- **Traffic (down)**: what it downloads now, with the upload below.
- **Connected**: how long it has been connected, or **Last seen** while it is offline.

The cards below:

- **Connection**: for a Wi-Fi client, **Access point** (a link to the device), **Network**, **Band**, **Channel**, **Wi-Fi standard**, **Link capacity**, **Interface** and **Connected since**. For a wired client, the device, the **Port** and **Connected since**. An offline client shows where it was last connected.
- **Experience**: the score and the part of it each factor gives, with the measured value and a bar for that factor's own score. Hover over a factor to see its weight. See [Experience score](#experience-score).
- **Details**: **MAC address**, **Vendor**, **Type**, **Hostname**, **IP addresses**, **First seen**, **Last seen**, and **Grouped by** for a grouped private address. A Wi-Fi client also shows its **Wi-Fi generation**, the **Bands** it has used, and its **Wi-Fi features**: 802.11k, 802.11v, 802.11w and WMM as seen on its connections, and 802.11r once it has fast-roamed. Hover over a feature for what it means. These are learned over time and kept while the client is away.

## Timeline

The Timeline tab (**Where it connected**) shows where the client connected and when, over **24 hours** or **7 days**.

- Each place has a lane: an access point with its network and band, or a device's port. Each bar is a session there; the session going on now is the brighter one. Hover over a bar for when it started and how long it lasted.
- Marks on a lane show where the access points [steered](wifi.md) the client: green when it moved, gray when it declined, amber when an access point turned it away, red when it was disconnected. Hover over a mark for what happened.
- **Steering**, under the lanes, counts each outcome and lists the newest ten in words, with a link to the rest on the Events page.
- The table lists each session, newest first: **Connected to**, **From**, **For** and **Ended** (**Roamed** when it moved to another place, **Left** when it disconnected, **Ongoing**).

A session ends as roamed when the client moves while connected, or shows up somewhere else within 15 seconds of leaving.

## Signal

The Signal tab charts the client's **Signal** and its **Rates**: **Down (PHY)** and **Up (PHY)**, the Wi-Fi link rates toward and from the client.

- **1h** shows the live samples the controller keeps for the last hour, every few seconds while the client is online over Wi-Fi.
- **24h**, **7d** and **30d** show the stored history. The line above the charts says how fine it is (for example "1-minute averages").
- **Open in Metrics** opens the same series in the Metrics explorer.

## Traffic

The Traffic tab charts the client's download and upload (**Throughput**) over **1h**, **24h**, **7d** or **30d**, like the Signal tab. **Downloaded** and **Uploaded** estimate how much it moved in that range, from the chart.

## Doctor

The Doctor tab looks for what is wrong with a client's Wi-Fi, its address and its DNS, and says what to try. Choose the range at the top: **1 hour**, **24 hours** or **7 days**. It reads again every minute.

The **Issues** card lists what it finds, the worst first, or **No issues found**. Each issue has a severity (**Critical**, **Warning** or **Info**), when it was last seen, the evidence in words, and under **Try** the fixes to try, with a link to the page where you make each one. It looks for these problems:

- **Weak signal**, and **Holds on to a far access point** (a client that stays on a weak access point while others carry its network).
- **Fails to join the Wi-Fi** (failed handshakes, often a wrong passphrase).
- **Can't get an address** and **No address** (DHCP trouble, such as a full pool).
- **DNS lookups fail** and **Many lookups blocked** (by DNS filtering).
- **Stays on 2.4 GHz** though it has used 5 GHz.
- **Trouble with 802.11r** (fast roaming that fails).
- **Keeps disconnecting**.

When a fix is to reconnect the client, admins get **Kick it to reconnect**, which opens the [kick](#kick-a-client) dialog.

Below the issues, a **Signal** chart covers the same range, and the **Timeline** merges the client's events, DHCP steps, Wi-Fi trouble from the logs and failed DNS lookups, newest first. Long timelines show the first 100 entries with **Show all**.

- DNS is checked only when the gateway logs DNS queries. Otherwise the card says so; see [Networks](networks.md).

## Client actions

Admins act on a client from the buttons on its page. Viewers don't see them.

### Rename a client

The name shows everywhere in uniwrt. The device itself isn't changed.

1. Select **Rename**.
2. Type a **Name**, up to 64 characters. Leave it empty to go back to the hostname.
3. Select **Save**.

### Fix IP

**Fix IP** reserves the client's address on the gateway's DHCP server, so the client always gets the same IPv4 address.

1. Select **Fix IP**. The button shows only for a client with an IPv4 address.
2. The dialog proposes an address on the client's network: its current address when no other reservation holds it, else a free one, and says which.
3. Change the **Address** if you want, and optionally a **Host name** to hand out with it.
4. Select **Reserve**. The reservation is saved as the site's next revision.
5. Select **Review and apply** to see what changes on the gateway and apply it. See [Review and apply](changes.md#review-changes).

If the client already has a reservation, the dialog says **Reserved already** with its address and network. Reservations are edited on the [Networks](networks.md) page.

> [!NOTE]
> Fix IP changes nothing on the gateway until you apply the revision.

### Wake a device

**Wake** has the gateway send a Wake-on-LAN packet to the client. Only a device set up for Wake-on-LAN wakes, usually one on a cable.

1. Select **Wake**.
2. Select **Wake** in the dialog. A message says where the packet was sent.

The first time, the gateway installs `etherwake` to send the packet; that can take a minute or more.

- The gateway must be managed for that install; a gateway in observe mode can wake clients only if `etherwake` is already there.
- Waking is refused while the gateway is offline or a job runs on it.

### Kick a client

**Kick** disconnects the client from the Wi-Fi of every device that manages Wi-Fi, and can keep it off for a while. Use it to make a device reconnect, for example to pick a better access point.

1. Select **Kick**. It works only for a client online over Wi-Fi.
2. Under **Keep it off for**, choose **No time**, **1 min**, **10 min** or **1 hour**.
3. Select **Kick**. A message says where it was kicked from, that it wasn't on a managed access point, or that no device manages Wi-Fi.

A kick changes no configuration, so there is nothing to apply.

### Block and unblock

**Block** disconnects the client and keeps it off the Wi-Fi of every device that manages Wi-Fi until you unblock it.

1. Select **Block**.
2. Select **Block** in the dialog.

The client shows a **Blocked** badge, and the list's **Blocked** filter finds it. To let it back, select **Unblock** on its page; it can connect again at once.

- A block belongs to one MAC address: a phone with private addresses may come back under a new one.
- Blocks apply to Wi-Fi only. Devices in observe mode don't enforce blocks or kicks.

> [!WARNING]
> If the controller is down, blocks lapse on the access points within about 10 minutes, and return when the controller is back.

## Experience score

The experience score rates a Wi-Fi client's connection from 0 to 100. It is a weighted mean of these factors, and the client's **Experience** card always shows the parts:

| Factor | Weight | Scores 0 at | Scores 100 at |
|---|---|---|---|
| **Signal to noise** (or **Signal** without a noise reading) | 35 | 10 dB (−85 dBm) | 40 dB (−55 dBm) |
| **Rate against capability**: the link rate against the best the client and radio can reach, while it moves traffic | 25 | 5 % | 50 % |
| **Retries**: frames sent again (failed frames count five times) | 20 | 40 % | 5 % |
| **Channel busy** | 10 | 90 % | 40 % |
| **Roams in the last hour**: many roams mean the client flaps between access points | 10 | 12 | 4 |

A factor without data drops out, and the others share its weight. Without a signal or a rate there is no score. Only clients online over Wi-Fi are scored.

A score of 80 or more reads as good (green), 50 to 79 as fair (amber), and below 50 as poor (red). The Clients page and the Dashboard average the scores of the online Wi-Fi clients, for the site and per access point.
