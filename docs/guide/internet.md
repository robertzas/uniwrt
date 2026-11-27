# Internet

The **Internet** page covers the gateway's connection to your ISP. You set up the WAN from an ISP preset, test it on the gateway before you save, and apply it like any other change. The page also measures speed and bufferbloat, shapes the line with smart queueing, adds a backup line, and watches the connection's quality.

Everyone can read this page. Only admins can change it ("Only admins can change the connection.").

> [!NOTE]
> Dynamic DNS names and encrypted DNS resolvers are not on this page. You find them on the **Networks** page, in the **Dynamic DNS** and **Upstream DNS** cards (see [Networks](networks.md)).

Before you start, check the notes at the top of the page:

- **No gateway yet**: adopt your main router first. Its WAN port is the site's internet connection. See [Devices and adoption](devices.md).
- *"… is in observe mode"*: the gateway is read-only. You can save a connection, but you can't test it until you turn management on. Select **Open its configuration** to do that.
- *"uniwrt doesn't manage the network package on …"*: applying leaves the gateway's WAN as it is. Select **Open its configuration** and choose **network** there first.

## Connection

The **Connection** card shows the WAN as it is saved:

- **ISP**: the preset it was last filled from, or **Custom settings**.
- **IPv4**: DHCP, PPPoE (with the username), a static address with its gateway, DS-Lite, or **None (IPv6 only)**.
- **VLAN**: the VLAN ID, or **Untagged**.
- **IPv6**: **DHCPv6 with prefix delegation** (with the prefix length requested), 6rd (with its prefix and border relay), or **Off**.
- **MAC address**: a cloned MAC, or **The gateway's own**.
- **MTU**: a fixed MTU, or **Automatic**.

The button in the card's header opens the **Internet connection** wizard. It reads **Set up** while the WAN has no preset, and **Change** after that. The wizard has four steps: **ISP**, **Settings**, **Test** and **Saved**.

### To set up the connection from an ISP preset

1. On the **Connection** card, select **Set up** (or **Change**).
2. In the **ISP** step, pick your **Country**. The list starts with the site's country. Choose **Generic only** to see only the generic presets.
3. Type in **Search ISPs** to filter the list. Your country's ISPs come first, then the **Generic** presets, which fit most lines (DHCP, DHCP on a VLAN, PPPoE, PPPoE on a VLAN, static, 6rd and DS-Lite).
4. Select a preset. An ISP's preset carries a badge: **Verified** (confirmed by the ISP's documentation or on a uniwrt installation), **Community** (confirmed by users' reports) or **Unverified**.
5. In the **Settings** step, read the preset's notes and its **Sources**. Fill in only the fields it asks for, such as the PPPoE **Username** and **Password**, or a static **Address and prefix**.
6. Select **Test the connection** (see below), or **Save without testing**.

If no preset fits, select **Custom settings** in the **ISP** step (it appears while the WAN has no preset), then set everything under **Advanced**.

> [!TIP]
> Some presets set IPv6 to try native IPv6 first and then the ISP's 6rd. The **Settings** step says so: "The test tries native IPv6 first, then the ISP's 6rd, and keeps whichever works." Test such a connection rather than saving it untested.

### Advanced settings

Open **Advanced** in the **Settings** step to see every setting. The preset only fills them in.

- **IPv4**: **DHCP**, **PPPoE**, **Static address**, **DS-Lite** or **None (IPv6 only)**.
- **IPv6**: **DHCPv6 with prefix delegation**, **6rd** or **Off**.
- **VLAN ID**: leave it empty if the ISP doesn't tag the line. **802.1p priority** (0–7) is for ISPs that require one on their VLAN, and needs a VLAN ID.
- **MTU**: empty uses the default, 1500, or 1492 with PPPoE.
- **Clone MAC address**: the ISP router's MAC, for ISPs that register it.
- DHCP options: **Vendor class (option 60)**, **Client ID (option 61)** in hexadecimal, **More DHCP options** (each as `code:value`, separated by spaces) and **Hostname sent** (empty sends the gateway's name).
- Static: **Address and prefix** (such as `203.0.113.10/29`), **ISP gateway** and **DNS servers** (separated by commas).
- PPPoE: **Username**, **Password** and **Service name** (only if the ISP requires one). After the first save, leave **Password** empty to keep the stored password.
- DS-Lite: **AFTR**. Leave it empty if DHCPv6 announces it.
- DHCPv6: **Prefix length to request** (48 to 64). Empty takes what the ISP delegates.
- 6rd: **6rd prefix** (such as `2602::/24`), **Border relay** and **IPv4 mask length** (usually 0).

Some combinations are refused: DS-Lite runs over DHCPv6, and a connection without IPv4 gets IPv6 by DHCPv6. Set **IPv6** to **DHCPv6 with prefix delegation** in both cases.

### To test the connection before you save it

The test tries your settings on the gateway for up to two minutes and then puts the gateway's own WAN back, whatever the result. Nothing is saved. Devices lose the internet briefly while it runs.

1. In the **Settings** step, select **Test the connection**. The button is disabled while the gateway is in observe mode.
2. Watch the steps as they run: **Link on the WAN port**, **Get an address**, **Reach the ISP's gateway**, **Resolve names**, **Reach an HTTPS site** and **IPv6** (plus **IPv6 over 6rd** for a preset that falls back to 6rd).
3. Read the result:
   - "The internet works over these settings." Select **Save**.
   - "The internet works. Native IPv6 didn't come up, but 6rd did, so it's saved as 6rd." Select **Save**.
   - "The internet works over IPv4, but IPv6 didn't come up." Check **Save with IPv6 off** if you don't want IPv6, then select **Save**.
   - **No internet over this WAN**: the failed step says what went wrong, often with a hint from the preset (for example, that the ONT must be in bridge mode). Select **Back** to change the settings, then **Test again**.

### To apply the new connection

1. After you save, the **Saved** step says "Saved as revision N".
2. Select **Review and apply** to open the **Changes** page.
3. Review the change and apply it. See [Review and apply](changes.md#review-changes).

> [!WARNING]
> Applying a WAN change briefly interrupts the internet. If the gateway's WAN or DNS doesn't come back, the apply rolls back to the previous configuration by itself.

## Speed and bufferbloat

This card measures the line from the gateway: download and upload speed, and latency both idle and while the line is full. Other devices' traffic counts too, so test when the line is quiet.

The newest test shows its **Bufferbloat** grade (A+ to F), what the grade means, **Download**, **Upload**, **Idle**, **Loaded ↓** and **Loaded ↑** latency. Each test is marked **Line, SQM off** or **As shaped**. With more than one test, **History** lists the latest ten.

The grade rates how much latency grows under load, in the worse direction. A+ means it barely moves; from C on, calls and games lag while the line is busy.

### To run a speed test

1. Select **Run a speed test**. This measures the line as it is shaped now.
2. Follow the steps: **Latency, idle**, **Download**, **Upload**, then **Grade the line**.

### To measure the line and get SQM rates

1. Select **Measure the line**. It stops SQM during the test to measure the line itself. This button appears only while the gateway is managed.
2. When the test ends, the card suggests rates: 90 % of the measured download and 95 % of the upload.
3. Select **Use these rates**. The **Smart queueing** form opens with them filled in. Save it there.

The suggestion shows while SQM is off, or when the line's speed changed since the rates were set ("The line's speed changed since the rates were set.").

Only admins can run tests.

## Smart queueing

Smart queueing (SQM) keeps latency low while the line is busy. It shapes traffic with cake a little below the line's speed, so the queue stays on the gateway instead of in the ISP's buffers. The card shows "Shaping at … down, … up", the link layer, and **Re-measured weekly** when autotune is on.

### To turn on smart queueing

1. Run **Measure the line** on the **Speed and bufferbloat** card and select **Use these rates**, or select **Edit** on this card.
2. Check **Shape the line with cake**.
3. Set **Download (Mbit/s)** and **Upload (Mbit/s)**. Each rate is 0.064 to 10000 Mbit/s.
4. Choose the **Link layer**: **None added**, **Ethernet, fiber (38 bytes)**, **Ethernet with PPPoE (46 bytes)**, **Cable, DOCSIS (18 bytes)** or **VDSL2 (34 bytes)**.
5. Optionally check **Re-measure the line every week at night and suggest new rates**.
6. Select **Save**, then **Review and apply**.

The first time, the apply also installs the SQM packages on the gateway ("The next apply installs the packages this needs on the gateway.").

With autotune on, uniwrt measures the line once a week at night. When the result is more than 10 % away from your rates, the **Speed and bufferbloat** card offers the new rates. Nothing changes until you save and apply them.

To turn SQM off, select **Edit**, clear **Shape the line with cake**, then save and apply.

## Failover

Failover adds a backup line, such as an LTE router or a tethered phone. While the WAN fails its health checks, IPv4 traffic moves to the backup line. Health checks ping 9.9.9.9 and 1.1.1.1 every 5 seconds. IPv6 stays on the WAN.

The card shows "No backup line." or the backup line, such as "Port lan4, by DHCP".

### To add a backup line

1. Select **Edit** and check **Fail over to a backup line**.
2. Under **Backup line**, choose:
   - **A port of the gateway**: type the **Port**, such as `lan4` or `eth2`. The port leaves the LAN. Add a **VLAN (optional)** if the backup line is tagged.
   - **USB tethering**: type the **USB device**, usually `usb0`.
3. Under **Address**, choose **DHCP**, or **Static** with **Address / prefix**, **Gateway** and **DNS servers**.
4. Select **Save**, then **Review and apply**. The first time, the apply also installs the packages failover needs.

An IPv6-only connection has no failover.

## Connection quality

Every 30 seconds the gateway pings, looks up and fetches a few targets. The card shows whether the internet works and, during an outage, where it lies.

- The badge reads **Internet up**, **Internet down** or **Not known yet**, with how long it has been so.
- **Checked at** gives the time of the latest round, with the ISP's first hop (or **No default route**).
- During an outage, a line says where the fault is: the gateway's WAN, the line (the ONT, the modem or the cable), the ISP's network, somewhere past the gateway, or DNS alone.
- **Latest round** lists each target with its **Latency**, **Fastest – slowest** and **Loss**. **No answer** means it lost every probe.
- Select a row to chart that target's latency and loss. Pick a range above the chart (**Last 24 hours** by default).
- **Outages** lists the outages of the last 30 days, newest first, with where each lay and how long it lasted.

The default targets are the ISP's first hop (**ISP gateway**), **Cloudflare** (1.1.1.1), **Google** (8.8.8.8), **Quad9** (9.9.9.9), lookups through the **Gateway resolver** and the **ISP DNS**, and a **Web** check.

### To change the probe targets

1. Select **Targets**. The **Probe targets** dialog opens.
2. Check or clear **Probe the connection** to turn the monitor on or off.
3. For each target, set its **Name** and its **Probe**:
   - **Ping**: a **Host** (an address or host name). Empty pings the ISP's first hop.
   - **DNS**: a **Name to look up**, and whom to **Ask**: **The gateway's resolver**, **Each DNS server the WAN learned**, or **A server** (with its **Server address**).
   - **HTTP**: a **URL**, `http://` or `https://`. Any answer counts.
4. Select **Add a target** for another one (up to 12), or the remove (bin) button next to a target to drop it.
5. Select **Save**. The targets take effect at once (no apply needed), and a round runs right away.

The internet counts as down when every ping past the first hop and every web check fails. Alert rules can watch a target by its series, `probe:<id>`, shown under its name. See [Insights](insights.md).

## Connection history

This card shows the last 7 days as a strip: the WAN down in red, and traffic on the backup line in amber. Below it you see how long the connection has been up without a break, and the latest events: the WAN going down and coming back, address changes and failovers. For the full list, open **Events** under **Insights** and choose the **Internet** category. See [Insights](insights.md).
