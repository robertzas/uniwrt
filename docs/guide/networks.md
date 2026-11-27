# Networks

The **Networks** page holds the site's networks and VLANs, and the services the gateway runs for them: DHCP, IPv6, local DNS names, upstream DNS, DNS filtering, dynamic DNS and the time server. Every change you save here becomes the site's next revision. Nothing reaches your devices until you review and apply it (see [Review and apply](changes.md#review-changes)), and a failed apply rolls back on its own.

Only admins can change this page. Viewers see the same cards without the edit buttons.

> [!NOTE]
> The page needs a gateway. Until you adopt your main router, it shows **No gateway yet**. If the gateway doesn't manage its `network` and `dhcp` packages, a warning says that applying leaves its networks as they are. Click **Open its configuration** to choose them on the device's **Config** tab (see [Devices and adoption](devices.md)).

## Networks

The **Networks** card lists each network with its purpose, its VLAN (or **Untagged**), its subnet and gateway address, its IPv6 mode and its reservations. On the right, a bar shows how full the DHCP pool is (for example, "37 of 150 leased") and how many clients are online in it. The bar turns amber at 75% and red at 90%. Warnings from the site's checks show under the network they concern.

Each network has a purpose that sets its usual VLAN and, on the [Security](security-vpn.md) page, its firewall policy:

- **Default**: your main LAN, usually untagged. You can't delete it or change its purpose.
- **IoT**: smart-home devices. Internet is allowed, but they are kept away from your LAN. Usual VLAN 30.
- **Guest**: visitors. Internet only, and they can't reach each other. Usual VLAN 40.
- **Management**: moves the nodes and the controller off the user LAN. Usual VLAN 99.
- **Custom**: a network of your own, such as a lab or cameras. Usual VLANs 50, 60, and so on.

The address plan suggests a /24 for each network, taken from the site's address space (chosen at first start). Its third octet is the VLAN ID, so VLAN 30 in `10.0.0.0/16` gets `10.0.30.0/24`, with the gateway at `.1` and the pool from `.100` to `.249`. These are only suggestions: you can type any subnet that passes the checks. The checks refuse overlapping subnets, gateways or pools outside their subnet, duplicate reservations, and subnets that collide with the WAN side.

A network reaches Wi-Fi clients through the Wi-Fi networks you put on it (see [Wi-Fi and mesh](wifi.md)), and wired clients through the device's ports (see [Ethernet ports](#ethernet-ports)).

### To add a network

1. Click **New network**.
2. Choose what it is for: **IoT**, **Guest**, **Management** or **Custom**.
3. Optionally, type a **VLAN ID (optional)**. Leave it empty to take the purpose's usual VLAN, or the next free one.
4. Click **Continue**. The network editor opens with the suggested name, subnet, pool and settings.
5. Check the fields (see below), then click **Save**.
6. Click **Review and apply** to send it to your devices.

New networks start with sensible settings for their purpose: **Guest** gets **Client isolation** on and **mDNS reflection** off, **Management** gets **mDNS reflection** off, and the others get **mDNS reflection** on.

### To edit a network

1. Click **Edit** on the network's row.
2. Change the fields in the editor's sections:
   - **General**: **Name**, **Interface name**, **Purpose** and **VLAN ID**.
   - **IPv4 and DHCP**: the gateway's address and the DHCP pool. See [DHCP](#dhcp).
   - **IPv6**: the prefix and how clients get addresses. See [IPv6](#ipv6).
   - **Behaviour**: **Client isolation**, **mDNS reflection** and **IGMP snooping**. See [Behaviour](#behaviour).
   - **Reservations**: fixed addresses. See [Reservations](#reservations).
3. Click **Save**. If the site refuses the change, the reasons show on the fields.
4. Click **Review and apply**.

Notes on the **General** fields:

- **Interface name** names the network's interface, firewall zone and DHCP pool. You choose it when you create the network; it is fixed once saved.
- **VLAN ID** empty means untagged. Only one network can be untagged. A VLAN ID is 1 to 4094, and no two networks can share one. When the mesh runs over cables, its wired backhaul takes one VLAN, which no network can use.

### To delete a network

1. Click **Edit** on the network's row.
2. Click **Delete network**, then **Delete network** again to confirm.
3. Review and apply. Its clients lose their addresses once the change is applied.

You can't delete the default network. You also can't delete a network while UPnP, a port forward or the DMZ host uses it: the editor says which one.

> [!TIP]
> To wake a computer on one of your networks, use **Wake** on its page under [Clients](clients.md). The gateway sends the Wake-on-LAN packet on the client's network.

### DHCP

The **IPv4 and DHCP** section of the network editor sets how the gateway serves the network:

- **The gateway has an address on this network**: on for every routed network. Off makes the network layer 2 only, with no DHCP.
- **Gateway address**: the gateway's address with the prefix length, such as `10.0.30.1/24`.
- **Hand out addresses by DHCP**: turns the pool on or off.
- **Pool start** and **Pool end**: the range DHCP hands out. The pool can't hold the gateway's address.
- **Lease time**: from **30 minutes** to **7 days**. New networks use **12 hours**.

Under **DHCP options**:

- **DNS servers**: up to 4 IPv4 addresses, separated by commas. Leave it empty to hand out the gateway, which also resolves local names and applies [DNS filtering](#dns-filtering). Clients that get other servers here bypass both.
- **NTP servers**: addresses separated by commas. Leave it empty to hand out none, or the gateway when the [Time server](#time-server) is on.
- **Domain**: the search domain handed out. Leave it empty to hand out the site's local domain.
- **Other options**: one per line, a code (or `option:<name>`), a comma, then the value, such as `26,1400`.
- **Network boot (PXE)**: turn it on to fill **Boot file** (such as `pxelinux.0`), **TFTP server name** and **TFTP server address**. Leave the address empty to use the gateway. An address needs the server's name too.

### IPv6

In the **IPv6** section, **Prefix** sets where the network's IPv6 addresses come from:

- **Off**: no IPv6 on this network.
- **Local only (ULA)**: a stable local prefix from the site's ULA range. Not available when the site has no ULA prefix.
- **From the ISP**: a /64 from the prefix your ISP delegates.
- **ISP and ULA**: both. New networks get this when the site has a ULA prefix, except Guest networks, which get **From the ISP**.

**How clients get addresses** sets the client mode:

- **SLAAC only**: clients pick their own addresses; no DHCPv6.
- **SLAAC, DNS by DHCPv6 (recommended)**: clients pick their addresses and learn DNS through stateless DHCPv6.
- **SLAAC and DHCPv6 addresses**: both, as stock OpenWrt does.
- **DHCPv6 only (Android gets no address)**: the gateway hands out every address. Android devices don't support it.

### Behaviour

- **Client isolation**: Wi-Fi clients on this network can't reach each other. It applies to the network's Wi-Fi networks; wired clients are not isolated.
- **mDNS reflection**: lets AirPlay, Chromecast and printers be found from the other networks that have it on. It works for IPv4, between up to 5 networks, and needs at least two.
- **IGMP snooping**: sends multicast, such as IPTV or casting, only to the ports and Wi-Fi clients that joined it. Turning it on for one network turns it on for all of them.

### Reservations

A reservation gives a client the same IPv4 address every time. The **Reservations** section appears when the network hands out addresses by DHCP.

To add a reservation in the editor:

1. Click **Add a reservation**.
2. Type the client's **MAC address**, the **Address** to keep, and optionally a **Host name**.
3. Click **Save**, then review and apply.

To remove one, click the trash button at the end of its row. Each MAC address and each address can be reserved only once.

The quicker way is **Fix IP** on a client's page (see [Clients](clients.md)). It keeps the client's current address when no other reservation has it, or picks a free one, and saves the reservation for you. The address plan keeps `.32` to `.99` of each /24 for reservations.

## Local DNS

The gateway answers `<hostname>.<domain>` for every client by itself, and `uniwrt.<domain>` for this controller, where `<domain>` is the site's local domain (usually `lan`). The **Local DNS** card lists the names you add on top of these.

### To add or change local DNS records

1. On the **Local DNS** card, click **Edit**.
2. Click **Add a record**.
3. Choose the **Kind**: **Address (A/AAAA)** for a name that points at an IP address, or **Alias (CNAME)** for a name that points at another name.
4. Type the **Name**, then the **Address** or the **Target name**.
5. Click **Save**, then review and apply.

To remove a record, click the trash button next to it while editing. You can't add a record named `uniwrt`: that name always points at the controller.

## Upstream DNS

The **Upstream DNS** card shows where the gateway sends the queries it can't answer itself. It asks up to three layers in turn:

1. **Encrypted resolvers**: up to 4 resolvers over DNS over HTTPS (DoH) or DNS over TLS (DoT), asked first, in the order you give.
2. **DNS servers**: up to 4 plain servers, IPv4 or IPv6, asked when the encrypted resolvers fail.
3. **ISP**: the servers your WAN connection hands out, asked last, as stock OpenWrt does.

A later layer hears a query only after the earlier ones refused or failed it. Within a layer, the gateway prefers the fastest server, so one dead server doesn't stall your DNS. The card also says whether the encrypted proxies run on the gateway, such as "2 DoH proxies running on the gateway" or "Stubby (DoT) running on the gateway". If it says one isn't running yet, apply the latest revision.

### To add an encrypted resolver

1. On the **Upstream DNS** card, click **Edit**.
2. Next to **Add a resolver**, choose **DNS over TLS** or **DNS over HTTPS**.
3. Click a provider: **Cloudflare**, **Quad9**, **Google**, **NextDNS**, **AdGuard** or **Custom**. A provider you already added for that protocol is greyed out.
4. Fill in what the row asks for:
   - **NextDNS**: your **NextDNS profile ID**, from your NextDNS setup page.
   - **Custom** DNS over HTTPS: the **Resolver URL** (an `https://` URL) and 2 to 4 **Bootstrap servers**, the plain DNS servers that find the resolver when the gateway starts.
   - **Custom** DNS over TLS: the **Certificate name** (the host name on its TLS certificate), 1 to 4 **Addresses** and the **Port** (853 unless the resolver says otherwise).
   - Optionally, for DNS over TLS, up to 4 **Key pins (optional)**. A pin stops working when the resolver changes its key, so add pins only on purpose.
5. Use the up and down arrows to set the order, then click **Save**.
6. Review and apply.

DNS over TLS resolvers always stay together in the list: the gateway asks them in turn, in the first one's place.

### To set the fallback layers

1. Click **Edit**.
2. Under **DNS servers**, click **Add a server** and type an IP address. Repeat for up to 4.
3. Tick or clear **Then the ISP's servers**.
4. To keep every query encrypted, tick **Encrypted only**. The gateway then asks only the encrypted resolvers. The other layers stay in the site, marked **Not asked: encrypted only**, for when you turn it off.
5. Click **Save**, then review and apply.

At least one layer must be in use. **Encrypted only** needs an encrypted resolver first.

> [!WARNING]
> With **Encrypted only**, the gateway has no plain DNS to fall back on. If every encrypted resolver is down, names stop resolving until one comes back.

A DoT resolver that stops answering costs a query up to 2 seconds before the next one is asked. A DoH resolver that stops answering costs the client's retry, about 1 to 5 seconds.

## DNS filtering

DNS filtering blocks ads, trackers and malware domains for every client on every network. The controller downloads the lists you choose, merges them and sends one compiled list to the gateway, which answers blocked names with NXDOMAIN. It is off until you turn it on.

When it is on, the **DNS filtering** card shows:

- The gateway's state, such as "The gateway blocks 152,301 domains", "Sending the lists to the gateway" or "The gateway is offline: the lists go out once it's back".
- When the lists were last checked.
- A note while the saved settings wait for an apply.
- A warning when the chosen lists hold more domains than the gateway can keep. The malware lists and your own blocks are kept first.
- **Domains blocked**, **Lists**, **Allowed** and **Clients bypassing**.
- Each list with its download state. A list that fails to download keeps its last good copy.

The controller checks the lists every night and sends new content to the gateway without an apply. Your choice of lists, allowed and blocked domains, and bypassing clients reach the gateway with an apply.

### To turn on DNS filtering

1. On the **DNS filtering** card, click **Edit**.
2. Tick **Filter DNS on every network**. With no list chosen, it starts from HaGeZi Normal and URLhaus.
3. Under **Lists**, tick the lists you want, under **Ads & trackers** and **Malware & phishing**. Each shows its size, and the dialog adds up your choice against what the gateway holds.
4. Click **Save**, then review and apply.

### To add your own lists and domains

1. Click **Edit**.
2. Under **Custom lists**, click **Add a list** and type a name and an `http://` or `https://` URL. Hosts files, plain domains, adblock (`||domain^`) and dnsmasq lines all work. You can add up to 16.
3. In **Allowed domains**, type the domains that are never blocked, one per line. Each covers its subdomains.
4. In **Blocked domains**, type the domains that are always blocked, one per line.
5. Click **Save**, then review and apply.

Each domain list holds up to 1,000 domains.

### To let a client bypass the filter

1. Click **Edit**.
2. Under **Clients bypassing the filter**, type a client's name or MAC address, and click **Add a client**.
3. Click **Save**, then review and apply.

A bypassing client's DNS goes to an unfiltered resolver on the gateway with the same upstream DNS. Up to 256 clients can bypass the filter.

### To download the lists now

Click **Refresh lists**. The card shows "Downloading the lists…" while it runs.

Limits you may hit:

- Clients that use their own DNS servers are not filtered.
- After the gateway reboots, names resolve unfiltered until the controller sends the lists again, which it does as soon as it sees the gateway online.

## DNS queries

The **DNS queries** card shows what the clients asked the gateway's DNS and what the filter blocked. It appears while DNS filtering and **Keep query statistics** are both on in the latest revision. To turn statistics on or off, click **Edit** on the **DNS filtering** card and use **Keep query statistics**. The controller keeps 24 hours of queries and 30 days of hourly counts.

The card shows, for the chosen **Range** (**24 h**, **7 days** or **30 days**):

- **Queries**, **Blocked**, **Blocked share** and **Clients**.
- A chart of answered and blocked queries, by hour over 24 hours and by day otherwise.
- **Top clients**, each linking to its client page.
- **Top domains** and **Top blocked** of the last 24 hours.

### To allow or block a domain from the statistics

1. Point at a domain in **Top domains** or **Top blocked** (on a touch screen, the button always shows).
2. Click the button next to it: it blocks an answered domain, or allows a blocked one.
3. Review and apply the new revision.

A domain already on your lists shows **Allowed** or **Blocked by you** instead.

### To search the query log

1. Click **Query log**. To see one client's queries, click the button next to it in **Top clients** instead.
2. Filter by **Client**, **Outcome** or **Domain contains**.
3. Click **Load older** to read further back, or **Refresh** for the newest.

Each query shows its outcome: **Forwarded**, **Cached**, **Local**, **Blocked** or **Failed**. You can allow or block domains from the log in the same way.

## Dynamic DNS

The **Dynamic DNS** card lists the names a DDNS provider keeps pointing at the gateway's public address, so you can reach home by name. Each row shows the provider, the record and where the address comes from, and what the gateway reports, such as "Registered 203.0.113.7", **Waiting for the first update** or **Updater not running**.

### To add a DDNS name

1. On the **Dynamic DNS** card, click **Add a name**.
2. Type the **Hostname**, the full name to keep up to date, such as `home.example.com`.
3. Choose the **Provider**: **Cloudflare**, **DuckDNS**, **deSEC**, **No-IP**, **Dynu**, **FreeDNS (afraid.org)** or **Custom URL**.
4. Fill in what the provider needs: a **Zone** for Cloudflare, an **Update URL** for a custom one, a **Username** where asked, and its token or password. The secret is stored in the controller's vault; when editing, leave it empty to keep it.
5. Choose the **Record**: **IPv4 (A)** or **IPv6 (AAAA)**.
6. Choose **Address from**: **The WAN interface**, or **A web echo** for a gateway behind another router.
7. Keep **Keep this name updated** ticked, then click **Save**.
8. Review and apply.

The gateway checks the address every 10 minutes and sends it anyway every 3 days. With **The WAN interface**, a private address is never published. You can keep up to 16 names.

To change or delete a name, click the pencil button on its row. **Delete** stops the updates; the provider keeps the last address.

## Time server

The **Time server** card turns the gateway's NTP server on or off. When **Serve time to the networks** is on, the gateway answers NTP on its networks, and networks without their own **NTP servers** hand out the gateway over DHCP. Clients and the access points then keep the gateway's clock, even while the internet is down.

### To change the time server

1. Tick or clear **Serve time to the networks**.
2. Click **Save**, then review and apply.

## Ethernet ports

Which networks each Ethernet port carries is set per device, on the device's **Ports** tab, in the **Ethernet ports** card. A port without settings is a trunk: it carries the default network untagged and every VLAN tagged. An access port carries one network only.

The card lists each port with its **Link** state and speed, the **Untagged** network, the **Tagged** networks and **Mesh backhaul**.

### To change what a port carries

1. Open the device from [Devices](devices.md) and choose the **Ports** tab.
2. Click **Edit**.
3. For each port, choose the **Untagged** network, or **None**.
4. Under **Tagged**, choose **All VLANs**, **None**, or **Some** and tick the networks it carries.
5. Under **Mesh backhaul**, clear the box to keep the port a plain port when it is a cable to another managed device. Otherwise the mesh may take the port over for its wired backhaul.
6. Click **Save**, then review and apply.

Limits you may hit:

- Until a network has a VLAN, ports carry the default network only.
- A switch port can't carry the mesh backhaul.
- A port the device doesn't have is shown as **Not on this device: render leaves it out**.
- If the device doesn't manage its `network` package, applying leaves its ports as they are.

> [!WARNING]
> On an access point, a port that stops carrying the device's own network can cut it off. The apply's automatic rollback restores the old settings if the device stops answering.
