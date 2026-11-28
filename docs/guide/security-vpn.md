# Security and VPN

Two pages cover how traffic gets in and out of your networks. The **Security** page holds the gateway's firewall: which networks reach which, the ports open to the internet, and UPnP. The **VPN** page holds the gateway's WireGuard server with its peers, and Tailscale.

Everyone can read both pages. Only admins can change them. Each change is saved as the site's next revision, which you then review and apply (see [Review and apply](changes.md#review-changes)). Nothing reaches the gateway before you apply it, and a failed apply rolls back by itself.

Both pages need a gateway. Until you adopt one, they show **No gateway yet** (see [Devices and adoption](devices.md)). uniwrt doesn't change a gateway in observe mode: you can save changes, but they reach it only after you turn its management on and apply.

## Security

![The Security page: traffic between networks, port forwards and UPnP](https://robertzas.github.io/uniwrt/guide/images/security.png)

The page starts with a warning when uniwrt doesn't manage the gateway's **firewall** package: applying then leaves its firewall as it is. Select **Open its configuration** and choose the firewall package there first.

### Traffic between networks

This card is a matrix. Each row is a network whose clients open connections; each column is a network they may reach, and the last column is **Internet**. Each cell reads:

- **Allow**: the row's clients may open connections to the column.
- **Block**: they may not.
- **Replies only**: blocked, but the other side may open connections this way, so its replies pass. Replies to connections the other side opened always pass.

A small marker flags a cell that differs from the default for its network's purpose ("Changed from the purpose's default"). The diagonal reads **Same network**.

The defaults depend on each network's purpose. The default and management networks reach everything. Guest, IoT and custom networks reach the internet only, and reach the gateway for addresses and names only. When a WireGuard server or Tailscale is set up, the matrix gains a **WireGuard VPN** or **Tailscale** row and column, which start out like the default network.

There is no row for the internet: the way in from the internet is a port forward.

#### To change which networks reach which

1. On **Traffic between networks**, select **Edit**.
2. Select a cell to switch it between **Allow** and **Block**.
3. To go back to the defaults for every cell, select **Reset to defaults**.
4. Select **Save**. All your cell changes go into one revision.
5. Select **Review and apply**.

### Port forwards

Port forwards open a port on the internet address and lead it to a host on one of your networks. Each forward in the list shows its protocol, ports and host, with badges where it is **Off**, limited (**Only from …**), or not reachable from inside your networks (**Not from inside**).

> [!TIP]
> Give the host a fixed address first: select **Fix IP** on the client's page (see [Clients](clients.md)). A forward to an address that changes stops working.

#### To add a port forward

1. On **Port forwards**, select **New forward**.
2. Fill in the **New port forward** dialog:
   - **Name**: such as `Game server`. Each forward has its own name.
   - **Protocol**: **TCP**, **UDP** or **TCP and UDP**.
   - **Internet ports**: a port, such as `443`, or a range, such as `27015-27030`.
   - **Only from**: an address or subnet allowed to connect, such as `203.0.113.0/24`. Empty allows anyone.
   - **Host**: the address of a host on one of your networks. The field suggests your clients' addresses.
   - **Host port**: empty uses the same port. A range keeps its length.
   - **On**: an off forward stays saved but opens nothing.
   - **Reachable from inside too**: hosts on your networks reach the forward through the internet address too (NAT reflection). It is on by default.
3. Select **Save**, then **Review and apply**.

The dialog checks what the gateway would refuse: a host outside your networks or the gateway's own address, and two enabled forwards on the same ports and protocol.

#### To change or delete a port forward

1. Select **Edit** on the forward.
2. Change its fields and select **Save**, or select **Delete forward** and confirm. Its ports close once you apply.
3. Select **Review and apply**.

#### To set a DMZ host

The **DMZ host** gets every connection from the internet that no forward claims. All its services are exposed, so prefer port forwards.

1. Under **DMZ host**, select **Edit**.
2. Type the **Host address** and select **Save**.
3. Select **Review and apply**.

To remove it, select the **Remove the DMZ host** button (bin icon) next to **Edit**, confirm with **Remove** in **Remove the DMZ host?**, then review and apply.

### UPnP and NAT-PMP

UPnP and NAT-PMP let games, consoles and calls open ports for themselves, and only for themselves, from port 1024 up.

#### To turn UPnP on or off

1. On **UPnP and NAT-PMP**, select **Edit**.
2. Check or clear **Let clients open ports**.
3. Under **On these networks**, choose the networks whose clients may open ports. Networks without IPv4 can't be chosen (**No IPv4**).
4. Select **Save**, then **Review and apply**.

If the gateway doesn't manage the **upnpd** package yet, the card says so. Choose it in the gateway's configuration; the apply then installs miniupnpd.

**Open ports** lists the ports clients opened: **Protocol**, **Port**, **Client** (address and name), **Client port**, **For** and **Expires in**. It is read from the gateway every minute.

## VPN

![The VPN page: the WireGuard server, its peers and Tailscale](https://robertzas.github.io/uniwrt/guide/images/vpn.png)

The **VPN** page lets you reach your networks from anywhere: a WireGuard server on the gateway for phones, laptops and other sites' routers, and Tailscale.

### WireGuard server

The card shows the server's state on the gateway:

- **Off**: the server is saved but its tunnel is down.
- **No report from the gateway yet**, or **Gateway offline**.
- **Not running on the gateway: apply the latest revision**.
- **Running with an older key: apply**.
- **Running**, with when the gateway last reported.

Below the state, you see the **Endpoint** peers dial, the **Tunnel** address, **DNS** (**The gateway** or **The peer's own**) and the server's **Public key**, with a button to copy it.

#### To set up the WireGuard server

1. If you want peers to dial a name rather than an address, add a DDNS name on the **Networks** page first (see [Networks](networks.md)).
2. On **WireGuard server**, select **Set up WireGuard**.
3. Set the **UDP port** (51820 by default). It is opened on the WAN.
4. Set the **Gateway's tunnel address**, with the tunnel's prefix (`10.0.250.1/24` by default). Peers get the next free addresses. The tunnel must use private addresses and must not overlap your networks, CGNAT space or `192.168.1.0/24`.
5. Under **Peers dial**, choose **A DDNS name** and pick it, or **A name or address** and type the gateway's **Public name or address**.
6. Check **Peers use the gateway for DNS** if you want your devices' names to resolve and DNS filtering to apply to peers.
7. Leave **On** checked and select **Save**. uniwrt makes the server's keys on the first save and keeps the private key sealed in its vault.
8. Select **Review and apply**.

To change the server later, select **Edit**. To remove it, select **Remove server** in the same dialog and confirm. This deletes every peer and its keys, so each peer needs a new configuration if you set the server up again.

The UDP port can't be one a UDP port forward already takes.

### Peers

Each peer is a phone, laptop or remote router that dials the server. Each row shows its name (with an **Own key** badge if it brought its own keypair), its address in the tunnel, its role (**Client · full tunnel**, **Client · split tunnel**, or **Site** with its networks) and its state:

- **Connected · handshake … ago**, or **Last handshake … ago**.
- **Never connected**.
- **Not on the gateway yet: apply**, or **Tunnel down**.

When the gateway reports it, the row also shows where the peer dials from and its traffic. The states refresh every 30 seconds.

#### To add a phone or laptop

1. On **Peers**, select **Add peer**.
2. Type a **Name**, such as `Robin's phone`. The **Address in the tunnel** is filled in with the next free address.
3. Under **Peer**, choose **Phone or laptop**.
4. Under **Route**, choose **Everything (full tunnel)** to send all its traffic through home, or **Only your networks (split tunnel)**.
5. Select **Save and show configuration**. The peer's configuration opens.
6. In the WireGuard app, choose to add a tunnel, then create it from a QR code, and scan the code. Or select **Copy** or **Download** to get the `.conf` file.
7. Select **Review and apply**. The peer can connect once the revision is applied.

A server has up to 250 peers.

#### To add another site

1. Select **Add peer**, type a **Name**, and choose **Another site**.
2. Under **Its networks**, list the subnets behind that router, one per line, such as `192.168.50.0/24`. They must not overlap your networks, the tunnel, or another site's networks.
3. Optionally fill in **Its public name or address** and **Its UDP port**. The gateway then dials it there; empty waits for it to dial in.
4. Select **Save and show configuration**, and load the configuration on the remote router.
5. Review and apply.

#### To use a peer's own keys

By default uniwrt makes each peer's keys. If the peer makes its own keypair, check **It brings its own keypair** and paste **Its public key**. Its configuration then lacks the private key: put the peer's private key where the file says.

#### To show a peer's configuration again

1. Select the QR code button on the peer's row (**Configuration of …**).
2. Scan the QR code, or select **Copy** or **Download**.

Revealing a configuration is recorded in the audit log. If the peer or the server is new or changed, apply the latest revision first.

#### To change, turn off or delete a peer

1. Select the edit button on the peer's row.
2. Change its fields. Clear **On** to keep the peer and its keys but refuse its connections.
3. Select **Save**, or select **Delete** and confirm. A deleted peer's keys are gone, and it can no longer connect once applied.
4. Review and apply.

### Tailscale

Tailscale joins the gateway to your tailnet. Devices in your tailnet can then reach your networks through the gateway, and use the gateway as an exit node. Tailscale runs on the gateway only, which must be managed.

The card shows Tailscale's state, such as **Connected**, **Logged out: log in to join the tailnet**, **Waiting for approval in the tailnet's admin console**, **Not on the gateway yet: apply the latest revision** or **Running, but not reaching Tailscale's servers**. It also lists the **Tailnet**, **Machine name**, **Addresses**, **Subnet routes**, **Exit node** and **Version**, and any warnings Tailscale reports.

#### To set up Tailscale

1. On **Tailscale**, select **Set up Tailscale**.
2. Under **Advertise these networks**, choose the networks tailnet devices may reach. The default networks are chosen for you; only networks with an IPv4 subnet are listed.
3. Optionally check **Offer the gateway as an exit node**.
4. Optionally set a **Machine name**. Empty uses the gateway's hostname.
5. Under **Login**, choose:
   - **From this page**: once applied, you log in from the Tailscale card.
   - **With an auth key**: paste an **Auth key** (it starts with `tskey-`). The gateway logs in by itself. You make a key in Tailscale's admin console, under **Settings › Keys**.
6. Leave **On** checked and select **Save**.
7. Select **Review and apply**. The apply installs the tailscale package, about 25 to 30 MB of flash.

> [!NOTE]
> Routes and the exit node only work once your tailnet's admin approves them. Until then the card says what waits for approval; select **Open the admin console** to approve it.

On OpenWrt 24.10, the tailscale package sends the daemon's logs to Tailscale, and uniwrt can't turn that off.

#### To log the gateway in

1. When the card says **Logged out**, select **Log in**.
2. The **Log in to Tailscale** dialog shows a link and a QR code. Select **Open the login page** on a device where you're signed in to Tailscale, or scan the code.
3. Wait until the dialog says "Logged in to …".

Whoever opens the link adds the gateway to their tailnet, so don't share it. Starting a login is recorded in the audit log.

#### To log the gateway out

Select **Log out** and confirm. Your networks are no longer reachable through Tailscale. If an auth key is saved, uniwrt logs the gateway in again with it: turn Tailscale off to keep it out.

#### To turn off or remove Tailscale

- To take the gateway out of the tailnet but keep its login, select **Edit**, clear **On**, save and apply.
- To stop managing Tailscale, select **Edit**, then **Remove Tailscale** and confirm. Its firewall rules go, but the daemon on the gateway stays as it is. Turn Tailscale off first to take the gateway out of the tailnet.

Tailscale takes UDP port 41641, so no UDP port forward or WireGuard server may use it.

### Tailnet devices

While Tailscale runs, this card lists the other machines in your tailnet as the gateway sees them: each one's address and system, whether it is online (**Direct** or **Relayed**) or when it was last seen, and its traffic.
