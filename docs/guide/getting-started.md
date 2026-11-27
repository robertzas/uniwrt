# Getting started

This chapter explains the few ideas uniwrt is built on, then walks you through your first session: creating the admin account, setting up a new router with the first-run wizard, or adopting a network you already run. It ends with the dashboard's **Get started** checklist, which tracks the rest.

## Core concepts

### The site

uniwrt manages one network, called the site. The site holds everything you configure: networks and their addresses, Wi-Fi networks, the mesh, the internet connection, firewall rules and per-device settings. You edit the site in the app; uniwrt turns it into each device's OpenWrt configuration.

### Roles

Every adopted device has one role:

- **Gateway**: the site's router. It holds the internet connection, routing, the firewall, DHCP and DNS. It can serve Wi-Fi too. A site has one gateway.
- **Access point**: a "dumb" access point. It bridges the site's networks to the gateway and takes its address from the gateway's DHCP. It has no DHCP server, firewall or WAN of its own.
- **Bridge**: a wireless bridge. It joins the mesh and bridges its Ethernet ports, for wired devices far from a cable.

You pick a role when you adopt a device, and you can change it later on the device's **Config** tab (see [Role](devices.md#role)).

### Observe mode and managed mode

A device always starts in **Observe** mode. uniwrt watches it, shows its live status and clients, and changes nothing on it. The device list marks it with an **Observe** badge.

When you turn management on for a device, uniwrt installs a small transaction tool on it and starts applying the site's configuration to it. Even then, it touches only the configuration packages you choose for that device (network, Wi-Fi, DHCP, firewall and so on). Packages you don't choose stay as they are. See [Management](devices.md#management).

### Revisions and review before apply

Every change you save in the app becomes a new revision of the site. Saving changes nothing on your devices.

When the latest revision differs from what your managed devices hold, the top bar shows **Review changes** with the number of devices to update. The review page reads each device fresh and shows, in words and line by line, what applying will change. Nothing reaches a device until you press **Apply** there. You can also **Discard changes** to go back to what the devices hold. See [Review and apply](changes.md#review-changes).

### Automatic rollback

Each apply is a transaction on every device it touches. Before the device reloads its configuration, it arms a timer. uniwrt then logs in again over a new connection and checks the device against how it was before: its internet connection, its radios, its mesh neighbors, and that uniwrt can still reach it. Only then does uniwrt confirm the change.

If a check fails, or uniwrt can't reach the device in time, the device restores its previous configuration by itself. This also works after an unexpected reboot. A change that cuts the device off therefore undoes itself.

> [!NOTE]
> Internet (WAN) changes apply last, in their own step. If the new internet settings don't work, only they roll back; the rest of the change stays.

## Create the admin account

The first time you open uniwrt, it has no users and shows **Create the admin account**. This account manages the whole network.

To create it:

1. Enter a **Username**.
2. Enter a **Password** of at least 12 characters, and repeat it in **Confirm password**.
3. Press **Create account**.

uniwrt then opens the first-run wizard, **Set up your internet router**.

- For safety, setup stays open for only 10 minutes after the controller starts. If you see **Setup is closed**, restart the controller, then reload the page or press **Check again**.
- Moving from another controller? Use **Moving a controller? Restore a backup instead.** on the same screen (see [Settings](settings.md)).

## Set up your internet router

![The first-run wizard, which has found a factory-default OpenWrt router at 192.168.1.1](https://robertzas.github.io/uniwrt/guide/images/first-run.png)

The first-run wizard turns a factory-default OpenWrt router into your gateway: internet, addresses and Wi-Fi, reviewed before anything changes. It has five steps: **Router**, **Internet**, **Addresses**, **Wi-Fi** and **Review**.

Leaving the wizard is safe at any step: nothing changes on the router before you apply. Press **Skip for now** to leave. To come back, use **Set up your internet router** on the dashboard (shown while no device is adopted), or open the command palette (Ctrl+K, or Cmd+K on a Mac) and run **Set up your internet router** (offered while no gateway is adopted).

- Only admins set up the router.
- Connect the computer that runs uniwrt to one of the router's LAN ports, so it reaches the router.

### Router

The wizard looks for an OpenWrt router at this controller's default gateway and at 192.168.1.1, OpenWrt's default address. It shows the router it finds with its model, release, address and SSH host key. A badge says **Factory defaults** or **Already configured**.

To use a factory-default router:

1. Check the **SSH host key**. Continuing trusts this key from now on.
2. Press **Use this router**.

uniwrt adopts it as the gateway, installs its SSH key, and gives root a long random password that the vault keeps (you can see it on the device's page). It then turns management on. When the step says the router **is your gateway**, press **Continue**.

To use a router that is already configured:

1. Enter its **Root password**. uniwrt uses it once, to install its own key, and doesn't store it.
2. Under **What should happen to its configuration?**, choose:
   - **Start fresh**: continue with the wizard. After a review, its network, Wi-Fi, DHCP, firewall and system settings are replaced by what you choose here.
   - **Keep its configuration**: adopt it read-only and import its settings as the site's. Nothing changes on the router. This leaves the wizard; see [Adopt an existing network](#adopt-an-existing-network).
3. Press **Adopt and continue** or **Adopt and import**.

To find a router at another address:

1. Open **My router is at another address**.
2. Enter the **Router address** (add `:port` if SSH isn't on port 22).
3. Press **Look there**. **Search again** repeats the automatic search.

If the router's release isn't supported (uniwrt supports OpenWrt 24.10 or later), the wizard says **This router's release isn't supported**. You can check **Adopt it anyway**; some features may not work on that release.

### Internet

Pick your internet provider from the list, or press **Enter the settings myself**. Then fill in the fields your provider needs. **Choose another ISP** goes back to the list.

**Test connection** is optional: it tries the settings on the router and rolls them back. Without a test, the apply connects to the internet last and undoes only the internet settings if they don't work. See [Internet](internet.md) for the settings themselves.

### Addresses

The site carves its networks out of one private range.

1. Enter the **Address space**, a private range such as `10.0.0.0/16`.
2. Enter the **Router address**, the gateway's address in the main network (usually the first).

The step shows the **Main network**, its **DHCP pool** and the **Local domain**. It also tells you whether the router keeps its address or moves. When it moves, the router answers at both addresses during the apply, wired devices are asked to renew their address, and the old address goes once the controller has arrived in the new network. Guest, IoT and other networks come later, on the [Networks](networks.md) page.

### Wi-Fi

The wizard sets up one Wi-Fi network on the router's 2.4 GHz and 5 GHz radios, in the main network, with WPA2/WPA3.

1. Keep **Set up a Wi-Fi network** checked, or uncheck it to leave the router's Wi-Fi as it is.
2. Enter the **Network name (SSID)** and a **Passphrase** of 8 to 63 characters.
3. Choose the **Country** the router is in. It sets the channels and power the radios may use.

A 6 GHz radio stays off for now, because it needs WPA3 alone, which older devices can't join. A router without radios skips this step. More networks and bands come later, on the [Wi-Fi and mesh](wifi.md) page.

### Review

The last step lists your choices in words. Nothing has changed on the router yet.

To apply them:

1. Press **Check with the router**. uniwrt saves your choices as one revision and checks them with the router.
2. Read what applying does. **Worth knowing** lists warnings, and **Every change, line by line** shows the full change. If the router can't take the plan, the step says **The router can't take this plan** and why.
3. If **This network is renumbered** appears, expect the computer you use to get a new address. If the page stops updating, reconnect and open the controller again.
4. Press **Apply** and follow the apply live.

When it says **Your router is set up**, press **Adopt access points** or **Open the dashboard**. If the internet settings didn't work, they are undone and everything else stays: press **Change the internet settings**, fix them, and apply again.

## Adopt an existing network

If your routers already run OpenWrt and you like how they are set up, adopt them as they are and import their configuration. Nothing changes on them until you decide.

To adopt and import an existing network:

1. On the dashboard, press **Adopt an existing network** (or go to **Devices** and press **Adopt device**).
2. Adopt your gateway first, then your access points. Once the gateway is adopted, the others usually appear under **Discovered**, where you can adopt several at once. See [Devices and adoption](devices.md).
3. The devices are in observe mode. While the site is still the defaults, **Devices** shows **The site is still the defaults**. Press **Import from the devices**.
4. uniwrt reads every adopted device (nothing changes on them) and shows what the site becomes. Check the changes and each device's result, then press **Accept as revision N**.
5. On each device's **Config** tab, press **Turn management on**, then choose its **Managed packages** and press **Save**.
6. Press **Review changes** in the top bar. The changes should be empty or nearly so. Press **Apply**.

> [!TIP]
> Import before you manage. The first apply then keeps your network as it is, instead of replacing it with the site's defaults.

See [Settings](settings.md) for the import page in detail.

## Get started

Admins see the **Get started** checklist at the top of the dashboard. Each step ticks itself from the network's live state and links to where you take it:

- **Adopt your access points**: done once a device other than the gateway is adopted.
- **Set up Wi-Fi**: done once the site has a Wi-Fi network.
- **Connect the access points**: done once the mesh is on, mesh links are seen, or every access point is on a cable.
- **Manage your devices**: done once every device is managed. Until then it counts how many are.
- **Back up the controller**: done once the controller's backup passphrase is set.
- **Get notified**: done once a notifier is set up.

The header shows how many steps are done, and **You're all set** when all are. Press **Hide the checklist** (the X) to hide it. This is remembered in this browser only.
