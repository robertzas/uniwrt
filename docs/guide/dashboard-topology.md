# Dashboard and topology

The Dashboard shows the whole network at a glance, and the Topology page draws it as a live map. Both update on their own as devices report, so there is nothing to refresh. This chapter also covers the parts of the app that every page shares: the top bar, the sidebar's appearance and account menus, the command palette and the keyboard shortcuts.

## Dashboard

The Dashboard is the first page after you sign in (**Dashboard** in the sidebar). Every card is live; only the internet throughput chart reads stored history.

Before any device is adopted, the page shows **Welcome to uniwrt** instead. Admins get two ways in: **Set up your internet router** starts the setup wizard for a new router, and **Adopt an existing network** opens adoption on the Devices page. See [Getting started](getting-started.md) and [Devices and adoption](devices.md).

### Get started

Admins see a **Get started** checklist at the top of the Dashboard. Each step ticks itself when the network reaches that state, and each open step links to where you take it:

| Step | Done when | Link |
|---|---|---|
| **Adopt your access points** | A device other than the gateway is adopted | **Adopt a device** |
| **Set up Wi-Fi** | The site has a Wi-Fi network | **Add a Wi-Fi network** |
| **Connect the access points** | The mesh is on, mesh links are seen, or every access point has a cable that is up | **Set up the mesh** |
| **Manage your devices** | Every device is managed (the step counts them, for example "2 of 4 managed") | **Open devices** |
| **Back up the controller** | The backup passphrase is set | **Set up backups** |
| **Get notified** | A notifier exists | **Add a notifier** |

The progress line reads "3 of 6 done", and the title changes to **You're all set** when every step is done.

To hide the checklist, select the **×** button (**Hide the checklist**). It stays hidden in that browser.

- Viewers don't see the checklist.
- Once hidden, the checklist can't be shown again from the app.

### Network health

The row of tiles under the checklist sums up the network. Each tile is colored by its state (green, amber, red, or gray when there is nothing to judge) and opens the page with the details.

- **Internet**: **Online** while a WAN link of the gateway has a default route, **Offline** when none has, **Unknown** without a gateway or while it is offline. The line below shows what the WAN carries now (↓ download, ↑ upload), or why it is not online (**No gateway adopted**, **The gateway is not answering**, **No WAN link has a route**). Opens the Topology page.
- **Mesh**: the mesh links between adopted devices, such as "3 links" or "2/3 up". The line below names the weakest link ("Weakest link at 55/100") or how many links are down. Any link down turns it red; a weakest link below 60 turns it amber, below 40 red. Opens the **RF / mesh** view of the topology.
- **Wi-Fi experience**: the average [experience score](clients.md#experience-score) of the Wi-Fi clients online, how many there are, and how many have a weak signal (below −75 dBm). Opens Clients.
- **Nodes**: devices online out of devices adopted, then **All online** or how many are offline or degraded. One offline device turns it red, one degraded device amber. Opens Devices.
- **Clients online**: how many clients are online, split into Wi-Fi and wired. Opens Clients.

### Internet

The **Internet** card shows the gateway's connection.

- A badge with the state (**Online**, **Offline** or **Unknown**).
- **Download · Now** and **Upload · Now**: what the WAN carries at this moment.
- **Throughput**: the site's download and upload over **1h**, **24h** or **7d**. **Open in Metrics** opens the same series in the Metrics explorer (see [Insights](insights.md)).
- The WAN details: **Interface** (name and protocol), **IPv4**, **IPv6**, **Delegated prefix**, **Gateway**, **DNS**, **Connected for**, and **Speed test** (the grade and rates of the newest speed test, once one has run; see [Internet](internet.md)). These are read from the gateway every 30 seconds while the page is open.

Without a gateway, the card says so; admins get an **Adopt your gateway** link.

### Topology mini-map

The **Topology** card draws the [Tree view](#views) of the map, with each device's clients grouped and traffic flowing. The line under the title counts devices and clients.

- Select a device to open its page, or a client to open the client's page. Anything else opens the full map.
- **Open the map** opens the Topology page.
- The mini-map doesn't pan or zoom: the mouse wheel and touch scroll the page.

### Top clients

**Top clients** lists the five online clients moving the most traffic right now. Each row shows where the client is connected, its download and upload rates, and a bar against the busiest client. Select a row to open the client. **All clients** opens the Clients page.

### Wi-Fi experience

The **Wi-Fi experience** card shows the average experience score of the online Wi-Fi clients as a ring, and how they spread across **Good** (80 and up), **Fair** (50 to 79) and **Poor** (below 50). **Weakest access point** names the access point whose clients score lowest, with its average. See [Experience score](clients.md#experience-score).

### Recent events

**Recent events** lists the newest notable events (notices and worse), newest first, each with its time. Clients joining and leaving don't show here; they stay on the Events page. Select an event to open the Events page filtered to its device. **All events** opens the Events page.

When alerts are firing, a **Firing** section above the events shows up to three of them, the ones needing attention first. Each opens the alert on the Alerts page, and the count (for example "2 alerts") opens the Alerts page. See [Insights](insights.md).

### RF

The **RF** card has a tile per band (2.4, 5 and 6 GHz):

- the channels your radios use,
- the busiest channel, how busy it is, and on which device,
- how many radios and neighboring networks the band has,
- badges for crowded radios ("2 crowded") and for networks that look like yours ("1 lookalike of your network").

Select a band's name to open RF insight on that band, or **Open RF insight** for the whole page.

## Top bar

The top bar runs across every page. From left to right:

- **Open navigation** (phones only): opens the sidebar as a drawer.
- The current page's name.
- **Search**: opens the [command palette](#command-palette). It shows the shortcut, ⌘K on a Mac, iPhone or iPad and Ctrl K elsewhere. On a phone it is a magnifier icon.
- **Drift** (admins, only when there is drift): how many managed packages were changed on the devices after uniwrt confirmed them. Select it for the list: each entry shows the device and the package, whether it was removed, and how long ago, and opens that device's **Config** tab. **Review & overwrite** opens the review page, where applying the latest revision writes your settings back. See [Changes](changes.md).
- **Review changes** (admins, only when there is something to apply): a count of the managed devices that the latest revision hasn't reached yet. Select it to [review and apply](changes.md#review-changes). While an apply runs it reads **Applying…** with a spinner. If the apply ends while you are on another page, a message says whether it worked.
- The alerts bell, described below.
- The controller status: **Online** with the controller's version, **Connecting**, or **Unreachable** when the controller doesn't answer. Hover over it for the build's commit and date. This is the status of the controller itself, not of your network; the network's health is on the [Dashboard](#network-health).

On a phone, **Drift** and **Review changes** shrink to icons with a count.

### Alerts bell

The bell's badge counts the alerts that need attention: firing, and neither acknowledged nor silenced. Its color follows the worst of them. When alerts are firing but all are acknowledged or silenced, the bell shows a small gray dot instead.

To see and acknowledge alerts:

1. Select the bell. The **Alerts** list shows how many are firing and up to eight alerts, the ones needing attention first. Each shows what fired, on which device, why, and for how long.
2. Select an alert to open it on the Alerts page.
3. As an admin, select the check mark beside an alert to acknowledge it. It dims until it resolves.
4. Select **Open alerts** for all of them.

When nothing is firing, the list says **All quiet**.

- Viewers can see alerts but can't acknowledge them.

## Appearance

The **Appearance** menu sits at the bottom of the sidebar. It holds two choices, both kept in your browser:

- **Skin**: **Graphite pro** (neutral, crisp and dense), **Midnight glass** (translucent layers and neon accents) or **UniFi-familiar** (airy cards, with the sidebar as a rail of icons).
- **Theme**: **System**, **Light** or **Dark**.

You can also switch both from the [command palette](#command-palette).

## Account

The **Account** menu sits below **Appearance** at the bottom of the sidebar, shown as your initial and user name. It shows your role (**Administrator** or **Viewer**) and has **Sign out**.

## Command palette

The command palette finds a page, a device or a client, or runs a command, from anywhere in the app.

To use the command palette:

1. Press ⌘K (Mac, iPhone, iPad) or Ctrl+K (everything else), or select **Search** in the top bar.
2. Type a few words. An entry matches when every word appears in its name or in what it is about, so "ap2 ip" or a part of a MAC address both work.
3. Use ↑ and ↓ to move, then ↵ to run the entry. Esc closes the palette, and so does the shortcut pressed again.

The palette offers, in groups:

- **Go to**: every page in the sidebar.
- **Devices**: **Adopt a device**, **Set up your internet router** (only while no gateway is adopted), **Review and apply changes**, then every adopted device, found by name, role, MAC address, IP address or model.
- **Clients**: every client the controller knows, found by name, hostname, MAC address, IP address or vendor.
- **Appearance**: **Use the Graphite pro skin** and the other skins, **Use the light theme** and the other themes.
- **Account**: **Sign out**.

- Viewers don't get the commands that change the network (adopting, setup, reviewing changes).

## Keyboard shortcuts

| Where | Key | Does |
|---|---|---|
| Anywhere | ⌘K or Ctrl+K | Opens or closes the command palette |
| Command palette | ↑ ↓, Home, End | Moves through the results |
| Command palette | ↵ | Runs the selected entry |
| Command palette | Esc | Closes the palette |
| Topology map | + (or =) and - | Zooms in and out |
| Topology map | 0 | Fits the map to the screen |
| Topology map | Arrow keys | Pans the map |
| Topology map | Esc | Clears the selection |

The topology keys work while the map has focus: select the map or move to it with Tab. Screen reader and keyboard users can also reach every device, client and link through a hidden list of the map's contents: moving to an entry selects it and opens its details.

## Topology

The **Topology** page is a live map of the network: the internet, the gateway, the access points and bridges, mesh peers that aren't adopted, unmanaged switches that uniwrt infers from what the devices see, and the clients. It redraws itself within a second or two of a change.

If no device is adopted yet, the page says **Nothing to map yet** and, for admins, offers to adopt one.

### Views

Choose a view with the switch at the top right. The view is part of the page's address (`/topology?view=tree`, `rf` or `clients`), so you can bookmark it.

- **Tree**: the network as a tree from the internet down, following each device's best path to the gateway. It draws only the uplinks. Each device's clients are grouped into one "12 clients" vertex.
- **RF / mesh**: the devices, switches and mesh peers with every link between them, including backup paths the tree leaves out. Mesh links are colored by health. No clients and no internet.
- **Clients**: everything, each access point with its clients around it.

To show or group a device's clients, double-click its client group or the device, or use **Show clients** and **Group clients** in the details panel. Your choice is kept per view while the page stays open.

### Reading the map

Select **Legend** in the toolbar for a key to the colors. In short:

- **Line width** shows how much traffic a link carries.
- **Line color** shows the link's health, from **Poor** to **Excellent**. A healthy cable stays gray (**Wired**), so a slow or flapping cable stands out.
- **Dashed red** lines are links that are down.
- **Dots flow** along links with the traffic, more and faster with more traffic. They don't move when your system asks for reduced motion.
- **Rings** around each adopted device show its status (green, amber, red) and its CPU load.
- **Clients** are dots in their band's color: 2.4 GHz, 5 GHz or 6 GHz.

The map fits itself to the screen until you pan or zoom. **Fit to screen** in the toolbar, or a double-click on an empty spot, fits it again.

### Toolbar

The toolbar's buttons are icons; hover over one for its name.

- **Zoom out**, **Zoom in** and **Fit to screen**.
- **Unpin all**: shown in the RF / mesh and Clients views while some device is pinned.
- **Weak spots**: see [Mesh health](#mesh-health).
- **Legend**: shows or hides the key.

To move around with a mouse, drag an empty spot to pan and use the wheel (or a trackpad pinch) to zoom.

### Pin devices in place

In the **RF / mesh** and **Clients** views, the layout arranges itself. You can pin a device where you want it.

1. Drag a device to where you want it. It stays there when you drop it.
2. To release it, select it and choose **Unpin** in the details panel. To release every pin, select **Unpin all** in the toolbar.

Pins are saved on the controller: every user and every browser sees the same layout, and both views share it. Viewers can pin too. Another open browser picks up new pins the next time it opens the page. The **Tree** view lays itself out and can't be rearranged.

### Details panel

Select a device, client, client group or link to open its details on the right. Select **Close details** (×) to close it, or press Esc while the map has focus.

For a device or client, the panel shows its kind (for example **Access point** or **Unmanaged switch**), its name and badges (its status, **Single point of failure**, **Pinned**), then what is known:

- **Connected to** (clients), **Addresses**, **MAC address**, **CPU** and **Memory**.
- **Uplink**: the device it hangs from in the tree and over what, such as "Gateway over Mesh 5 GHz", whichever view you are in. Then the link's **SSID** and **Signal**.
- For a client, the **Link** and its **Traffic now** (down and up) and how much it has **Carried**.
- The mesh health rows described in [Mesh health](#mesh-health).

The buttons at the bottom open the device (**Open device**) or the client (**Open client**), show what goes down, group or show clients, or unpin.

A client group lists its clients (the first 30, then "and 12 more"). Select one to show the group's clients and select that client.

For a link, the panel shows the medium and band, both ends, an **Up** or **Down** badge, and then **Ports**, **SSID**, **Signal**, **Transmit quality** (batman-adv's measure of the path, as a percentage), **Health** or **Quality**, **Drops (30 min)**, **Capacity** and the traffic.

### Mesh health

The map tells you where the network is fragile: what each device relies on and what you lose if it fails.

For a device, the details panel shows:

- **To the gateway**: how many hops it is from the gateway, and how many of them are over the air (for example "2 hops, 1 over the air", or "all wired").
- **Depends on**: the nearest device or switch it can't lose to reach the gateway. Select the name to jump to it.
- **If it goes down**: how many devices and clients would lose the gateway, or "Nothing else loses the gateway". Shown for access points, bridges, switches and mesh peers.
- A **Single point of failure** badge when some other device depends on it.

For a link between devices, the panel shows:

- **Health**: a score out of 100. When it is below 90, it names what holds it back: being down, drops, signal, transmit quality or throughput. It turns amber below 70 and red below 40.
- **Drops (30 min)**: how often the link went down while both ends stayed online, and when it last did.
- A **Critical link** badge when something reaches the gateway only over this link.

To see what a device's loss cuts off:

1. Select an access point, bridge, switch or mesh peer.
2. Select **Show what goes down**. The map crosses the device out, rings everything that would lose the gateway in red, and dims the rest.
3. Select **Back to the map** to return.

To find every weak spot at once, select **Weak spots** in the toolbar. The map rings each single point of failure and haloes each critical link in amber. A badge beside the title counts them, or says **No single point of failure**. Select **Weak spots** again to turn it off.

Each device's **Mesh** tab shows the same health for that device and its links. See [Devices and adoption](devices.md).

### Replay

The bar along the bottom of the map replays the last 24 hours, so you can see what the network looked like when something broke.

1. Drag the slider back. Tick marks above it show when the network changed. The time and how long ago it was show beside the slider, and a **Replaying** badge appears next to the title.
2. Select devices and links as usual to see their state at that time.
3. Select **Back to live**, or drag the slider all the way to the right, to return to the live map (**Live**).

- The replay shows the map without traffic or CPU load, and no dots flow.
- It moves in steps of 15 seconds and covers at most the last 24 hours. Until the network has changed, the bar says **The history fills in as the network changes.**

### On a phone

The map works with touch: drag to pan, pinch to zoom, tap to select. The details open as a sheet along the bottom of the screen, and the map moves the selected device above it. Drag the sheet's grip down to close it.
