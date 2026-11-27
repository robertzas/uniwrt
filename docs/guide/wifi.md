# Wi-Fi and mesh

The **Wi-Fi** page holds the site's wireless networks (WLANs), the site's client steering and the mesh that links your access points to the gateway. Everything here is one setting for the whole site: each managed device broadcasts the WLANs that apply to it. A save becomes the site's next revision and changes nothing on the air until you review and apply it (see [Review and apply](changes.md#review-changes)).

> [!NOTE]
> Only devices whose **Radios, Wi-Fi networks and the mesh** package is managed take these settings. A device in observe mode, or one whose Wi-Fi package isn't chosen, keeps its Wi-Fi as it is. You choose the packages on the device's **Config** tab, under **Managed packages** (see [Devices and adoption](devices.md)).

## Wi-Fi

The top of the page lists every WLAN. Each row shows:

- The name (SSID), an **Off** badge when it is disabled, its security mode and its bands in the band colors.
- **Network** and the network its clients land in, the devices that broadcast it (**All devices**, **Only …** or **All but …**), and **Hidden**, **Clients isolated** or **Up to N clients per radio** when those are set.
- Its schedule, with **on now** or **off now**.
- Warnings about this WLAN, if the site has any.
- How many clients are online on it now.

Notes above the list tell you when something needs attention:

- The site has no country yet. The radios then stay on their defaults. Select **Set the country** to open Settings.
- A device doesn't manage its Wi-Fi package, so applying leaves its Wi-Fi alone. Select **Open Devices** to fix that.
- A device runs the basic wpad, so its access points can't suggest where clients should roam (802.11v). The note links each such device to its **Config** tab (see [Install the full wpad](#install-the-full-wpad)).

The line under the list names the regulatory domain the radios use (**Regulatory domain: …**).

Viewers see the list but can't add or edit WLANs.

### To add a WLAN

1. Select **New WLAN**.
2. Under **Start from**, pick a preset:
   - **Main**: your own devices, WPA2/WPA3 on 2.4 and 5 GHz.
   - **Guest**: visitors. No passphrase, encrypted where possible, clients isolated from each other, on your guest network if you have one.
   - **IoT**: smart-home devices. WPA2 on 2.4 GHz only, on your IoT network if you have one.
3. Type the **Name (SSID)** and, for a mode with a passphrase, the **Passphrase**.
4. Change any other field (see [WLAN settings](#wlan-settings)).
5. Select **Save**.
6. Select **Review and apply** to send it to the devices.

Switching presets refills the form but keeps the name and passphrase you typed. Guest and IoT networks themselves are made on the [Networks](networks.md) page. A WLAN only picks the network its clients join.

### To edit or turn off a WLAN

1. Select **Edit** on its row.
2. Change what you need. To stop broadcasting it but keep its settings, clear **Enabled**.
3. Select **Save**, then **Review and apply**.

### To delete a WLAN

1. Select **Edit** on its row.
2. Select **Delete**, then **Delete** again to confirm.
3. Review and apply. Its clients lose their connection at that apply.

### To join a WLAN with a phone

1. Select the QR code button on the WLAN's row.
2. Point the phone's camera at the code.

For a WLAN with a passphrase, the dialog also shows the passphrase. Showing it is written to the audit log. Viewers get the QR code only for WLANs without a passphrase.

## WLAN settings

The WLAN dialog has these sections.

### General

- **Name (SSID)**: 1 to 32 bytes, and no other WLAN may have it. In **Open, encrypted where possible** mode the limit is 29 bytes.
- **Network**: where its clients land: their addresses, VLAN and firewall policy.
- **Enabled**: off keeps the WLAN in the site without broadcasting it.

### Security

**Security mode** offers:

| Mode | What it means |
|---|---|
| **WPA2/WPA3** | WPA3 for the devices that can, WPA2 for the rest. The usual choice. |
| **WPA3** | The strongest, and what 6 GHz requires. Older devices can't join. |
| **WPA2** | For old devices that fail on WPA3 networks. |
| **Open, encrypted (OWE)** | No passphrase, but every client's traffic is encrypted. Devices without OWE can't join. |
| **Open, encrypted where possible** | No passphrase. Devices with OWE move to a hidden, encrypted network by themselves; older ones stay unencrypted under the same name. On 6 GHz it is OWE alone. |
| **Open** | No passphrase and no encryption: anyone nearby can read the traffic. |

The three WPA modes need a **Passphrase** of 8 to 63 printable ASCII characters (or a 64-digit hex key). When you edit a WLAN, leave the field empty to keep the current passphrase. Passphrases are stored encrypted on the controller.

### Bands

Toggle **2.4 GHz**, **5 GHz** and **6 GHz**. Each device broadcasts the WLAN on its radios of those bands. Pick one band at least.

6 GHz requires **WPA3**, **Open, encrypted (OWE)** or **Open, encrypted where possible**.

### Devices

**Broadcast on** chooses where the WLAN is on the air:

- **All devices**.
- **Only these**: tick the devices.
- **All but these**: tick the devices to leave out.

### Options

- **Hide the name**: clients must type the name to join. It hides nothing from an attacker.
- **Isolate clients**: its clients can't reach each other, only the gateway and the internet.
- **Multicast to unicast**: sends group traffic (IPTV, casting) to each client that joined it, at that client's own rate, once IGMP snooping is on. On by default. Turn it off for devices that miss such traffic.
- **Clients per radio**: the most clients one radio takes, from 1 to 2007. Leave it empty for no limit.

### Roaming

These settings help clients move between your access points.

- **Fast roaming (802.11r)**: **Auto**, **On** or **Off**. With it, clients move between access points without a new handshake, so calls and video don't drop. **Auto** turns it on for **WPA2** and off for **WPA3** and **WPA2/WPA3**, where some clients fail with it. The line under the choice says what Auto means for the mode you picked. Modes without a passphrase never use it.
- **Roaming hints (802.11k/v)**: access points tell clients their neighbors and suggest better ones. On by default. Devices without the full wpad offer the neighbors only.
- **Steer clients**: lets the site's [Steering](#steering) move this WLAN's clients. Turn it off for devices that roam badly, such as smart-home gadgets.

> [!TIP]
> If a device can't join a WPA3 network after you turn fast roaming **On**, set it back to **Auto** or **Off**.

### Schedule

Tick **Only on at certain times** to broadcast the WLAN only at the times you set:

1. Pick the days, and the **From** and **To** times. When **To** isn't after **From**, the time runs until the next day; equal times mean the whole day.
2. Select **Add a time** for more spans, or remove one with its remove button.

Times are in the site's time zone (set in [Settings](settings.md)). The controller turns the WLAN off outside these times and on again when they start, without an apply. While the controller is down, the WLAN stays as it is.

## Steering

The **Steering** card moves clients to the better band or access point. The controller asks clients to move, and usteer on the access points tells it where each client is heard. There is one profile for the whole site:

- **Off**: clients choose their access point and band themselves. The default.
- **Conservative**: asks clients with a strong signal (above -60 dBm for 2 minutes) to move up a band. Nothing else.
- **Balanced**: band steering after 1 minute above -63 dBm. A client fading below -75 dBm is asked to move to an access point heard 8 dB stronger, and is disconnected 10 seconds later if it stays, so it reconnects there.
- **Aggressive**: band steering after 30 seconds. Clients below -72 dBm are asked to move to an access point heard 5 dB stronger, and disconnected 10 seconds later if they stay. It also refuses clients below -85 dBm and sheds clients from channels over 75 % busy.

Only clients with 802.11v are asked to move, and only while the controller runs. Other clients choose for themselves (Aggressive still refuses and sheds them). Each WLAN takes part unless you clear **Steer clients** in its Roaming settings.

Viewers see the profile but can't change it.

### To change the steering profile

1. Pick a profile on the **Steering** card.
2. Select **Save**.
3. Review and apply.

If no WLAN takes part, the card warns you. If a device runs the basic wpad, the card names it: steering does nothing there (Aggressive can only refuse or disconnect clients).

To see what steering did, open a client's page or a device's page (see [Clients](clients.md)). Steering events also appear on the Events page (see [Insights](insights.md)).

### Install the full wpad

Most OpenWrt images ship the basic wpad, which lacks 802.11v. Without it, access points can't suggest where clients should roam, and steering can't ask them to move.

1. Open the device's **Config** tab (the Wi-Fi page's note links there).
2. On the **Wi-Fi daemon** card, select **Install the full wpad**, then **Install**.
3. Review and apply to turn 802.11v on.

The device downloads the full wpad and a copy of the current one first; nothing changes unless both arrive. Its Wi-Fi restarts, so its clients reconnect within about 20 seconds, and a mesh access point loses its uplink meanwhile. If the full wpad doesn't install, the device puts the current one back. The device must be managed.

## Mesh backhaul

The **Mesh backhaul** card sets the mesh that links your access points to the gateway. batman-adv carries it over the air (802.11s) and, where two managed devices share a cable, over that cable too.

- **Mesh on**: turns the mesh on or off. Each managed device's mesh radio restarts once to join it, so its Wi-Fi clients reconnect. Devices without batman-adv or 802.11s get them when you apply.
- **Mesh ID**: the mesh's name, which every device shares: 1 to 32 bytes.
- **Key**: the mesh's passphrase, 8 to 63 characters. A key is generated when the mesh turns on without one. Select **Generate** for a new random key. Leave the field empty to keep the current key.
- **Band**:
  - **Auto**: 5 GHz. A device with two 5 GHz radios gives the mesh the one that reaches the highest channel.
  - **2.4 GHz**: the last resort, slow and crowded. Choose it only where 5 GHz doesn't reach.
  - **5 GHz**: the usual backhaul. On a dual-band device it shares the radio and its airtime with Wi-Fi clients.
  - **6 GHz**: the cleanest spectrum, but every device needs a 6 GHz radio. A device without one leaves the mesh.
- **Routing**: **BATMAN V** picks paths by measured throughput and is the best for most meshes. **BATMAN IV** picks paths by packet loss, for meshes that must work with older devices.
- **Mesh over cables**: a cable between two managed devices carries the mesh on a VLAN of its own and leaves the LAN. The LAN between those two devices pauses for about 4 seconds when it switches.
- **Backhaul VLAN**: the VLAN those cables use, 1 to 4094. No network may use it.

Settings you change while the mesh is off wait until it is on. Viewers see the settings but can't change them.

### To change the mesh

1. Change the settings on the **Mesh backhaul** card.
2. Read any warning the card shows (see below).
3. Select **Save**.
4. Review and apply.

### What a mesh change does to the links

> [!WARNING]
> Changing the **Mesh ID**, **Key**, **Band** or **Routing** drops the mesh's links while the devices take the change. They all take it at the same moment. A device that reaches the gateway over the mesh's Wi-Fi alone drops off for a while.

uniwrt protects you here:

- The review names the devices that would drop off.
- If the mesh doesn't come back after the apply, every device rolls back to its previous settings by itself.
- Turning the mesh off would cut off any device that reaches the gateway over the mesh's Wi-Fi alone. The review refuses that apply until the device is connected by cable.

### To keep a port out of the mesh

When **Mesh over cables** is on, batman-adv may take over any cable between two managed devices. To keep a port a plain LAN port:

1. Open the device's **Ports** tab.
2. On the **Ethernet ports** card, select **Edit**.
3. Clear the port's checkbox in the **Mesh backhaul** column, then select **Save**. The column then shows **Opted out**.
4. Review and apply.

Ports on an older switch chip (swconfig) can't carry the backhaul.

## Mesh tab

Each device's page has a **Mesh** tab that shows how the device fits in the mesh.

- **Path to the gateway**: the hops to the gateway, what the device **Depends on** (or **Only the gateway**), and **If it goes down**: how many devices and clients go down with it. **Lose the gateway with it** names those devices.
- **Mesh links**: its links to other devices, weakest first. **To** names the other end, **Link** the medium, band and ports, **Health** whether the link is up with its score and weakest factor, **Drops (30 min)** how often it went down, and **Quality (24 h)** a history line for links over the air. A **Critical link** badge marks a link something reaches the gateway only over.
- **Failed cables**: cables that failed the backhaul's test stay plain trunks. Select **Forget** to have the next apply test the cable again. Only admins can forget a cable.

A warning at the top names a port that carries only the mesh backhaul while no device answers on it. Whatever is plugged in there gets no LAN. Select **Open Ports** to opt the port out if the other end is gone for good.

The [Topology](dashboard-topology.md) page shows the same links on the map.

## Radios

The Wi-Fi page has no per-radio controls. The radios take their settings from these places:

- **Country**: set it in Settings, on the **Site** tab (see [Settings](settings.md)). It sets the radios' regulatory domain: the channels and power they may use. A wrong country is illegal on the air. Until you set one, the radios stay on their defaults.
- **Channel, width and power**: the RF planner chooses them (see [RF planner](#rf-planner)).
- **Bands**: each WLAN uses the radios of the bands you ticked in it.

To see what a radio runs now, open the device's page. Its **Radios** card shows each radio's band, channel, width, Wi-Fi standard, **Clients**, **Transmit power**, **Noise** and **Channel busy**, and the WLANs and mesh on it. **Restarts tonight** means a channel switch moved the radio and it restarts in the nightly window to catch up.

## RF planner

The RF planner lives on the **RF** page under **Insights**, not on the Wi-Fi page. Its **Channel plan** card previews the channel, width and power it would choose for each radio, weighing your other radios, the networks around, how busy each channel is and radar. Select **Use this plan** to save it as a revision, then review and apply it. **Optimize nightly** lets it do this by itself every night when a plan is at least 15 % better.

Radios that move restart, and their clients reconnect within seconds. A new mesh channel drops the mesh's links until every device has it, the same as a mesh change above. Radios on devices in observe mode keep what they run.

See [RF](insights.md#rf) for the full page.
