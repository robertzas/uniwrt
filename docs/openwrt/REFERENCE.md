# OpenWrt reference

These are the facts uniwrt relies on, collected here so sessions don't research them again.
Marks: ✅ verified (release notes, a test, or a real device) · ⚠️ from documentation or memory. Verify ⚠️ items in the system harness (M3.3) or on a real node before depending on them, then flip the mark.

## Releases

- ✅ **25.12** is the current stable series; 25.12.0 was released 2026-03.
  - **apk** replaces opkg. Package names are mostly unchanged; the CLI differs.
  - The `owut` upgrade CLI ships on devices with larger flash, and the LuCI Attended Sysupgrade app is installed by default.
  - Known issues at release: 802.11r Fast Transition with WPA3 causes client problems, and Pixel 10 phones fail to join WPA3 on Wi-Fi 6 APs.
- ✅ **24.10** uses opkg. Its security support ended in 2026-09, so it is best effort in uniwrt.
- ✅ Direct sysupgrade from 23.05 to 25.12 is not supported; go through 24.10.
- ⚠️ ucode is in every default image since 22.03, because firewall4 depends on `ucode`, `ucode-mod-fs`, `ucode-mod-ubus` and `ucode-mod-uci`. hostapd has embedded ucode since 23.05.

## Factory-default state ⚠️

- LAN `br-lan` at 192.168.1.1/24, with DHCP (dnsmasq) and RA/DHCPv6 (odhcpd); WAN and WAN6 are DHCP clients.
- root has no password; SSH and LuCI accept that until one is set.
- Wi-Fi radios have `option disabled '1'`.
- Firewall: lan → wan forwarding allowed; wan input REJECT.
- Hostname `OpenWrt`; dropbear on :22.

## Files

| Path | Use |
|---|---|
| `/etc/config/*` | UCI packages |
| `/etc/board.json` | Model, LAN/WAN port names, switch layout, LEDs (render reads its layout from the facts) |
| `/etc/openwrt_release`, `/etc/os-release` | Version, target, architecture |
| `/tmp/dhcp.leases` | dnsmasq leases (gateway) |
| `/var/run/miniupnpd.leases` | miniupnpd's mappings (`upnpd.config.upnp_lease_file`): `<PROTO>:<ext port>:<client>:<int port>:<expiry>:<description>`, expiry 0 for none (before 2.0 without the expiry field); the description may hold colons ⚠️ |
| `/etc/dropbear/authorized_keys` | SSH keys |
| `/etc/sysupgrade.conf`, `/lib/upgrade/keep.d/*` | Extra files preserved across sysupgrade |
| `/etc/uci-defaults/*` | First-boot scripts (deleted after they succeed) |
| `/tmp` | tmpfs (RAM) |

## Package manager ⚠️

| Task | opkg (≤ 24.10) | apk (25.12+) |
|---|---|---|
| Refresh the index | `opkg update` | `apk update` |
| Install | `opkg install <pkg>` | `apk add <pkg>` |
| Remove | `opkg remove <pkg>` | `apk del <pkg>` |
| List installed | `opkg list-installed` | `apk list --installed` |
| Install a local file | `opkg install /tmp/x.ipk` | `apk add --allow-untrusted /tmp/x.apk` |

Detect which one is present by trying `command -v apk` first.

## ubus objects ⚠️

| Object | Methods and events we use |
|---|---|
| `system` | board, info, validate_firmware_image, reboot |
| `network.interface`, `network.interface.{name}` | dump, status, up, down; notifications ✅ `interface.update` (the interface's status, `up: true`, and `updated`, such as `["addresses"]`) and `interface.down`, sent by both the main object and the interface's own; broadcast event `network.interface` (`action`: `ifup`/`ifdown`, `interface`) |
| `network.device` | status; notifications ✅ `add`, `remove`, `up`, `down`, `link_up`, `link_down` and `topo_change` (a bridge's ports), each with `name`, `present`, `active`, `link_active`, `auth_status`; only for devices netifd knows |
| `network.wireless` | status ✅: per radio `up`, `disabled`, `config` (UCI options, the Wi-Fi keys included) and `interfaces` (`section`, `ifname`, `config`). 25.12 images without wifi-scripts have no such object |
| `iwinfo` (rpcd-mod-iwinfo) | devices, info, assoclist, scan, freqlist, txpowerlist |
| `hostapd.{ifname}` | get_clients ✅ (`clients.<mac>`: `authorized`, `wmm`, `ht`, `vht`, `he`, `mfp`, `rrm` (5 bytes of RRM capabilities), `extended_capabilities` (bit 19: BSS transition), `aid`, `bytes`, `packets`, `rate`, `signal`; nothing says whether FT was used), get_status ✅ (`freq`, `channel`, `op_class`, `airtime`, `dfs`), del_client ✅ (`addr`, `reason`, `deauth`, `ban_time` in ms: deauthenticates the station if it is there, then bans it from probes, authentications and associations until the time lapses or the BSS restarts; 0 lifts a ban; ✅ on both releases with a subscriber on the object, as the agent always is. hostapd's source arms the timer with `ban_time * 1000` µs in an `int`, so keep bans under ~35 min: D-084), bss_transition_request ✅ (`addr`, `dialog_token`, `validity_period` and `disassociation_timer` in beacon intervals, `disassociation_imminent`, `abridged`, `neighbors` (report elements in hex); with MBO built in (the full wpad) `mbo_reason` 0–9, `cell_pref` 0/1/255 and `reassoc_delay` (only with imminent; a u8 on 24.10/25.12, so ≤ 255) are required: 24.10's and 25.12's handler leaves them uninitialized when omitted and answers "Invalid argument" (D-091; OpenWrt main defaults them to 0, 1 and 0). A timer has hostapd disconnect the station after it unless it left (✅ both releases, D-093); `not_found` for a station not there), rrm_nr_get_own ✅ (`value`: `[bssid, ssid, report hex]`, only with `ieee80211k 1`), rrm_nr_set ✅ (`list` of `[bssid, ssid, report hex]`, replacing the list; in memory, lost when the BSS restarts), rrm_nr_list ✅ (`list`, the same triples; both releases, D-086), rrm_beacon_req, switch_chan ✅ (both releases, D-113: `freq`, `bcn_count`, `center_freq1`, `center_freq2`, `bandwidth` in MHz, `sec_channel_offset`, `ht`, `vht`, `he`, `block_tx`, `force`; **no EHT flag**, so a Wi-Fi 7 BSS's switch is unverified (⚠️) and uniwrt reloads those. It switches every BSS of the radio and answers 0 once the announcement starts (`CTRL-EVENT-STARTED-CHANNEL-SWITCH`); `get_status` reports the new `freq` from then on, while nl80211 (`iw dev … info`, the agent's `radios` set) keeps the old channel until the countdown ends (`AP-CSA-FINISHED`). In the hwsim lab the countdown never ends: no beacons go out (a station there counts `beacon rx: 0`), hostapd logs `Cannot set beacons during CSA period` every 6 s, and only a restart (`wifi reload`) clears it. A switch onto a DFS channel without a finished CAC is refused by the kernel (⚠️ on hardware)), notify_response. Notifications to subscribers ✅ (both releases, all with `address` and `ifname`): `probe` (every probe request), `auth` and `assoc` (`target`, `signal`, `freq`), `sta-authorized` (`auth-alg`: `open`, `sae`, `ft`…), `disassoc` (also when the BSS goes down); fire-and-forget unless `notify_response` is set |
| `hostapd` ✅ (24.10, 25.12; hwsim lab, M5.1) | `config_set` (`phy`, `radio` on a multi-radio phy, `config`: a hostapd config file, as netifd writes `/var/run/hostapd-<phy>[.<radio>].conf`): hostapd adds and removes BSSes to match the file (`Add bss`/`Remove bss … on phy`, `Reloaded settings`); the others keep running, the first BSS (`interface=`) included when it goes (the next `bss=` takes its place); a file without BSSes removes hostapd's interface on the phy, and a mesh point there stays up; an unchanged file changes nothing. `config_remove` (`iface`) removes a BSS too but leaves hostapd's record of the phy's config stale. `ip link set <bss> down` stops a BSS beaconing, but a radio's second BSS never comes back from `up` (NO-CARRIER until a reload) and hostapd logs `Failed to set beacon parameters` every 6 s meanwhile. After a removal, netifd's `network.wireless status` still lists the section with its `ifname`; nl80211 no longer has the interface. hostapd **restarts the radio** (`Restart interface for phy`, every client dropped) whenever the file's radio part changes (`channel`, `htmode`'s lines: `hostapd.uc` compares the radio lines it recorded at the last `config_set`), so a channel change through UCI and a reload always restarts it, also after a `switch_chan` already moved it there (D-113). A `txpower` change alone isn't in the file (netifd's script sets it with `iw phy … set txpower`, 24.10's also per interface with `iw dev`): the reload answers `Reloaded settings` and clients stay ✅ both releases |
| `usteer` ✅ (24.10's 2022.08.18~7d2b17c9, 25.12's 2025.10.04~1d6524c6; D-090) | `get_config` (the running options: numbers, `ssid_list` as an array), `local_info` (the local BSSes it steers, keyed `hostapd.<ifname>`, each with `ssid`, `bssid`, `freq`, `n_assoc`, `load`, `rrm_nr`; BSSes of SSIDs outside `ssid_list` are left out), `remote_info` (every remote BSS in one table, keyed `<host address>#hostapd.<ifname>`, the same fields), `remote_hosts`, `get_clients`. Notifications to subscribers: `probe_req_accept`/`_deny`, `auth_req_*`, `assoc_req_*`, `load_kick_*`, `signal_kick` (with `local`/`remote` node status, `signal`, `reason`, thresholds). It sets every steered BSS's 802.11k list with `rrm_nr_set` each update (1 s; `max_neighbor_reports` caps it, 0 clears it). Its init script passes the options it lists to `usteerd` and not `aggressiveness`, `reassociation_delay`, `probe_steering` or `band_steering_signal_threshold`, so those stay compiled defaults; the package ships `/etc/config/usteer` with one anonymous `usteer` section (`network 'lan'`, `syslog`, `local_mode`, `ipv6`, `debug_level 2`) and enables the service. Peers talk UDP 16720 on `network`'s device |
| `service` (procd) | list, set, delete |
| `uci` (rpcd) | get, set, commit, apply, confirm, rollback |
| `luci-rpc` (only with LuCI) | getHostHints, getDHCPLeases, getBoardJSON |
| `dnsmasq` | its only method is `metrics` ✅; notifications ✅ `dhcp.ack` (`mac`, `ip`, `name`, `interface`) and `dhcp.release` (no `name`) |
| `dhcp` (odhcpd) ✅ | `ipv6leases` (`device.<dev>.leases[]`: `duid`, `iaid`, `hostname`, `ipv6-addr[]` with `valid-lifetime`, `valid`); `ipv4leases` only where odhcpd may serve DHCPv4 (in 24.10, not in 25.12); `add_lease`, `ipv6ra` |
| `log` (logd) ✅ | `read` with `{"lines":N,"stream":false}` (also `oneshot`): the last N entries as `log[]`, with `id` (a sequence, from 0 when logd starts), `priority`, `source` (0 for the kernel's, whose `msg` has no tag), `time` (ms) and `msg` (`program[pid]: text`); `write` |
| ubusd ✅ | broadcast events `ubus.object.add` and `ubus.object.remove` (`id`, `path`) |

## CLI

`uci` · `ubus call|listen|subscribe` · `batctl meshif <dev> mj|hj|oj|nj|gwj|tgj|tlj` (JSON ✅: throughputs and gateway bandwidths in 100 kbit/s, translation-table VLAN -1 for untagged; `batctl s` has no JSON) · `iw dev|phy|station dump|survey dump|scan` · `iwinfo` · `batctl o|n|tg|gwl|s|tp` · `logread [-f] [-e regex] [-l N] [-t]` (✅ `-t` adds `[<s>.<ms>]` after the date: `<ctime> [<s>.<ms>] <facility>.<severity> <msg>`, the kernel's as `kern.<severity> kernel: <text>`; `-f -l N` sends the last N, 0 for none, then follows) · `sysupgrade -T|-b|-n|-k` · `firstboot` · `reload_config` · `wifi reload|up|down|status` · `fw4 check|print|reload` · `jsonfilter` · `uclient-fetch` · `owut` · `apk` / `opkg`

`reload_config` ✅ (both releases) fires `config.change` for each package whose `uci show` output no longer matches the sum it kept in `/var/run/config.md5` (`<md5>  /var/run/config.check/<package>`), then keeps the new sums; without that file it fires nothing. Writing a package's sum there first keeps its reload from happening: netifd then runs the old configuration (`network.wireless status`'s `config` still shows it) until the next reload of `network` or `wireless` (the txn tool's live apply, D-113).

The apply relies on (✅ 24.10 and 25.12, M3.4a2b): BusyBox `nslookup <name> 127.0.0.1` prints `Server:`/`Address:` lines, then a `Name:` line per answer (`localhost` gets two, 127.0.0.1 and ::1) and exits 0; a name it can't find prints `** server can't find <name>: NXDOMAIN` (once per query type) and exits 1. `/etc/init.d/<service> enabled` exits 0 exactly when the service starts at boot (an `S??<service>` link in `/etc/rc.d`, which `disable` removes), else 1.

## ucode ✅

Verified in the 24.10.8 and 25.12.6 rootfs containers (M1.3).

- Modules: `fs`, `ubus`, `uci`, `math` and `html` in both. The 25.12 rootfs adds `uloop`, `log` and `uclient`, and the 24.10 one has no `uloop`. ⚠️ Wi-Fi images have `uloop` through hostapd's dependencies.
- Modules export with `export { a, b };`, because `export function` doesn't parse. `import { x } from "./file.uc"` resolves relative to the importing file, `sourcepath(0, true)` is the script's directory, and `ARGV` holds the script's arguments.
- `sprintf("%J", v)` prints one-line JSON. Bytes that aren't UTF-8 come out re-encoded as Latin-1 on 24.10 and raw on 25.12, so the controller decodes lossily.
- `system(argv, timeout_ms)` runs without a shell. On timeout it kills with SIGKILL and returns -9, and a process ended by a signal returns `-signal`. The child inherits stdout, so redirect it.
- `fs.readfile(path, limit)`, `fs.readlink`, `fs.lsdir`, `fs.glob`, `fs.access`, `b64enc` and `clock()` (`[s, ns]`; `clock(true)` is monotonic) exist in both.
- uloop: a buffered `read()` on a non-blocking descriptor drops data at EAGAIN, so stdin needs blocking reads in a forked `uloop.task`. A task's stdio is `/dev/null`; it reads the pipe through `/proc/self/fd/0`, reopened before the fork. `uloop.process` reports `WEXITSTATUS` (0 after a signal) and has no `kill()`.
- nl80211 and rtnl (ucode-mod-nl80211 and ucode-mod-rtnl, which hostapd depends on, so on Wi-Fi images only) ✅: attributes are named in lower case without their prefix (`wiphy_freq`, `channel_width`, `wiphy_tx_power_level`, `sta_info.signal_avg`, `plink_state`). Bitrates are in 100 kbit/s, with the flags `width_40|80|160|320`, `40_mhz_width`, `short_gi`, and `he_gi`/`eht_gi`; powers are in mBm; `sta_flags` is `[mask, set]`. The survey has no `in_use`, so match the interface's frequency. A split `GET_WIPHY` dump starts with a null entry, and `wiphy_bands` is indexed by `enum nl80211_band`. An `AF_UNSPEC` neighbor dump has no bridge FDB; ask with `family: AF_BRIDGE`. The module exports no `NL80211_SCAN_FLAG_*` and no `NL80211_CMD_ABORT_SCAN` (24.10, 25.12): use the kernel's values (FLUSH 2, AP 4; 114), and scan through a beaconing AP only with the AP flag. `request()` answers null both on error and on an acknowledgement without a reply, and `error()` clears what it reports; a `waitfor()` that times out leaves "No event received" behind, so read `error()` after it.
- `fs.realpath` exists in both releases ✅.
- `ucode -e 'sleep(250)'` sleeps to the millisecond in both releases ✅, where BusyBox's `sleep` takes whole seconds only (`sleep 0.25` fails) and `date` has no `%N` ✅: the txn tool's scheduled reload (M6.1 b2) sleeps through ucode, counting on `/proc/uptime`'s centiseconds.
- ubus subscribers ✅ (both releases, under uloop): `conn.subscriber(notify_cb, remove_cb)`, then `sub.subscribe(path)`; `notify_cb(req)` sees `req.type` and `req.data` but not which object sent it, so use a subscriber per object; `remove_cb` runs when the object goes away. `sub.remove()` frees it (outside its own callbacks). `conn.listener(pattern, cb(ev, data))` receives broadcast events, and `listener.remove()` stops it. `conn.list()` returns names, `conn.list(pattern)` signatures.
- ubus: `conn.call()` returns null both for a failure and for an empty reply. `conn.error()` returns the last error and clears it; messages start with the `ubus_strerror()` text, such as `Not found: …` or `Method not found: …`.

## Containers as test nodes ✅

- An `openwrt/rootfs` image boots with `/sbin/init` as PID 1 in an unprivileged container: procd, ubusd, rpcd, logd, netifd, odhcpd, uhttpd and dropbear run. dnsmasq crash-loops: procd jails it with ujail, which cannot make namespaces in a container ✅. With `--cap-add NET_ADMIN`, netifd builds `br-lan` from eth0.
- The first boot generates `/etc/config/network` (`config_generate`) and then runs `/etc/uci-defaults/*`. Change the network config there, not before boot.
- BusyBox has `tar`, `sha256sum -c -s`, `flock`, `ulimit -f` (512-byte blocks) and `set -o pipefail`. It lacks `timeout`, `stat`, `od` and fractional `sleep`.
- The 24.10 rootfs has opkg (`/bin/opkg`), and the 25.12 one has apk (`/usr/bin/apk`).

- ✅ x86's `99-default_network` makes `eth0` the LAN and `eth1`, when it exists at first boot, the WAN; board.json's model is the host's DMI name. BusyBox's `ip` has no `-br`, and the stock rootfs has no `bridge` (package `ip-bridge`).
- ✅ dnsmasq's init refuses to serve a segment where another DHCP server answers (`found already running DHCP-server on interface 'br-lan' refusing to start, use 'option force 1'`), as an AP on its stock config does: render sets `force '1'` on the gateway's pools (D-068).
- ✅ procd runs reload triggers `PROCD_RELOAD_DELAY` (1 s) after `reload_config`; dnsmasq's reload starts it even when disabled, so a service that goes off must stop first. netifd keeps reporting the old interface until its trigger runs.
- ✅ rp-pppoe-server's stock options hold `login` (PAP against system accounts): the harness's ISP uses its own options and `pap-secrets`. PPPoE in containers needs the host's `pppoe` module and `--device /dev/ppp`; VLANs need `8021q`, and fw4 the nftables expression modules (ct, reject, masquerade): a container can't load them.

- ✅ Wi-Fi in containers (the hwsim lab, D-035): radios moved into a container's network namespace (`iw phy <phy> set netns <pid>`) appear in its `/sys/class/ieee80211`. netifd checks the Wi-Fi daemons it starts through `/proc/<pid>/exe`, which needs `--cap-add SYS_PTRACE`. hostapd and wpa_supplicant log harmless errors writing `/proc/sys`, which is read-only. A container's interfaces come in no fixed order when several Docker networks are attached before it starts, so the lab links nodes with a veth pair instead. hwsim radios are tri-band (2g, 5g, 6g, with EHT) and hear each other across namespaces.

## procd, netifd and sysfs

- ✅ batman-adv's mesh device has no DEVTYPE, and batman-adv has no sysfs or debugfs tables any more; its hard interfaces are its `lower_*` links, rtnetlink's link kind is `batadv`, and netifd names the device after its `proto batadv` interface.
- ✅ A phy's `device` link resolves (`fs.realpath`) to `/sys/devices/<path>`, and OpenWrt's radio `path` is that `<path>` (`virtual/mac80211_hwsim/hwsim0`); ⚠️ later phys of one device get `+1`, `+2`.
- ✅ `system info`: memory in bytes, `root` and `tmp` in KiB, and `load` scaled by 65536.
- ✅ `network.interface dump`: addresses are `{address, mask}`. A LAN's IPv6 addresses are the `local-address` of each `ipv6-prefix-assignment`, and delegated prefixes are under `ipv6-prefix`. Default routes have target `0.0.0.0` or `::` with mask 0.
- ✅ `/sys/class/net`: `DEVTYPE` is in `uevent` (`bridge`, `vlan`, `wlan`). `carrier` and `speed` can't be read while a device is down, and bridges and VLANs report their ports' speed. `master` links to the bridge or batman device, and `bonding_masters` may sit beside the devices.
- ⚠️ DSA user ports have `DEVTYPE=dsa`.
- ✅ An interface with two `list ipaddr` entries holds both; dnsmasq's init takes the pool's subnet from the first, and authoritative dnsmasq answers a request for a lease of the other subnet with `DHCPNAK … wrong network` (the pool on 24.10 and 25.12 in the system test; the NAK seen on 25.12, M4.3a2).
- ✅ `ubus call network.interface.<name> renew` (SIGUSR1 to udhcpc) sends one unicast and three broadcast renews 3 s apart; udhcpc ignores the NAKs, declares the lease lost after about 13 s and discovers again: a new lease about 16 s after the call (25.12). Losing the carrier restarts the client at once (a veth peer loses its carrier when the other end goes down).
- ✅ BusyBox's `ip link add <name> type veth` ignores `peer name …`: the peer gets a name of the kernel's (`veth0`); find it by `iflink` and rename it.
- ✅ A bridge (`device` of type `bridge`) whose only port has a carrier comes up with its static interface; a gateway with IPv6 forwarding takes no address from another router's RAs on it (the adoption port, 24.10 and 25.12 in the system test). ⚠️ The device option `ipv6 '0'` (netifd writes `disable_ipv6`) is unverified: a container's `/proc/sys` is read-only, so netifd's write fails silently there.
- ✅ Moving a configured bridge port at runtime (the adoption port's lend, D-107; 24.10 and 25.12): `network.interface.<name> remove_device {name}` answers 0 but leaves a port that the bridge's config lists where it is; `ifdown` of the bridge's interface frees it (netifd deletes the bridge and sets the port down), `add_device {name}` on another interface enslaves it (on a bridge that filters VLANs, the interface's VLAN as PVID, untagged), and `remove_device` then `ifup` of the first interface put it back. 24.10's netifd takes a port added this way for absent (`external`, `present: false`) until the kernel reports it again: `ip link set <port> up` does. `reload_config` doesn't bring an `ifdown`'d interface back up.
- ✅ A DHCP client on a LAN with a stock OpenWrt on it (at 192.168.1.1, serving 192.168.1.0/24) may take that device's offer over the gateway's and keeps it for its 12 hours (the system harness's stock AP, 25.12). ⚠️ A staged device's client could do the same with its own dnsmasq if it still served while the network reloaded: D-107's staging stops dnsmasq and odhcpd first, as an AP's render does.
- ⚠️ Unverified on hardware: `ip link set <port> down` on a DSA user port or a plain NIC drops the link partner's carrier (the PHY powers down); the renumbering's port bounce relies on it (D-081).
- ⚠️ `hostapd -v<feature>` and `wpa_supplicant -v<feature>` exit 0 when the feature is built in (LuCI checks it the same way).

## First boot and HTTPS from a node ✅

Checked on the 24.10.8 and 25.12.6 rootfs (D-102, `crates/node/tests/ztp.rs`):

- uci-defaults run in `boot` (START=10) after `config_generate`, so `network` exists; a service a script enables there doesn't start that boot (procd listed `/etc/rc.d` before): start it too.
- `uclient-fetch` (libustream-mbedtls, mbedtls 3.6) refuses a CA whose name constraints are critical (`--ca-certificate` then fails as "not signed by a trusted CA"); non-critical ones are ignored. `--post-data` is sent chunked, without `Content-Length`. `--header` works on both.
- musl fails a whole lookup when the AAAA query is refused (a forwarder without upstream): fetch names with `-4` where only an A record exists.
- The default dnsmasq has `local '/lan/'`, `domain 'lan'` and rebind protection, so `<name>.lan` from an upstream server never resolves on the node itself: `localuse 0` makes `/tmp/resolv.conf` follow the lease's DNS servers instead.
- Busybox has `hexdump` but no `base64` or `od`; `jsonfilter`, `jshn`, `dropbearkey`, `passwd` and `px5g` (`selfsigned -newkey ec -pkeyopt ec_paramgen_curve:P-256`) are there. uhttpd's CGI sees only standard headers.
- In Docker: `/etc/resolv.conf` and `/etc/hostname` are bind mounts (unmount with SYS_ADMIN), and the hostname can't change (`/proc/sys` is read-only).

## UCI syntax ✅

libuci's parser (`file.c`, strict mode as `uci` and rpcd run it), checked against `uci` of 24.10 and 25.12; `crates/uci` ports it and `crates/uci/fixtures/cases/` holds a case per rule.

- Statements: `config <type> [<name>]`, `option <name> [<value>]`, `list <name> [<value>]`, `package <name>`, or `c`, `o`, `l`, `p`. A file's `package` statement is checked and ignored; in `uci import` it starts a package. The command word ends only at a space or tab, so a bare `config` on its line is an "invalid command".
- Words: `'…'` is literal and may span lines (the newline is kept); in `"…"` and bare words `\` takes the next character literally; `\` at the end of a line continues it, but a continuation line that starts with whitespace ends the word. Parts concatenate (`un'quoted'"cat"`). `#` outside quotes ends the line, even glued to a word (`v#c` is `v`).
- `;` alone between words separates statements. Glued to an unquoted word without escapes (`1;option y 2`), it ends the line silently; after a quoted part (`'a';b`), it separates. libuci parses in place, so the two differ. `;;` is an "invalid command", and so is an empty quoted word after a statement (`option x 1 ''`).
- Names: section and option names are `[A-Za-z0-9_]+`; types are printable ASCII without spaces (`a.b` and `wifi-iface` pass); a `package` name is `[A-Za-z0-9_-]+`. A file whose name holds a `.` loads, but the CLI can't address it ("Entry not found").
- Values: an `option` without a value or with `''` is skipped, and doesn't clear an earlier one. A `list` without a value adds `''`. A `list` after an `option` of the same name makes a list of both; an `option` after a list replaces it in place. A named section that comes again is the same section (its options merge); with another type it is an error. `config a ''` is anonymous. Anonymous sections are `@type[i]`; `uci show` names them so, and `uci -X` shows the `cfgXXXXXX` names it hashes from type and options.
- Errors read `uci: Parse error (<reason>) at line <n>, byte <pos>`. The line counts only lines that end in a newline, and the byte is the offset in the (continued) line buffer.
- Commit writes `\nconfig <type> '<name>'\n`, then `\toption <name> '<value>'\n` per option and a line per list item, with `'` as `'\''`, then a final `\n`. Comments and formatting are lost. CRLF reads fine.

## UCI snippets ⚠️

VLAN-tagged WAN on DSA (Lumen uses VLAN 201):

```
config device
	option type '8021q'
	option ifname 'wan'
	option vid '201'
	option name 'wan.201'

config interface 'wan'
	option device 'wan.201'
	option proto 'dhcp'
```

6rd (Lumen/CenturyLink values, community-sourced; render names it `wan6`, with `tunlink 'wan'` and `mtu '1480'`):

```
config interface 'wan6rd'
	option proto '6rd'
	option peeraddr '205.171.2.64'
	option ip6prefix '2602::'
	option ip6prefixlen '24'
	option ip4prefixlen '0'
```

PPPoE with DHCPv6-PD over the PPP link (render's shape, so `reqprefix` applies; `ipv6 'auto'` would add a dynamic `wan_6` instead): `interface wan` with `option ipv6 '1'`, and `interface wan6` with `option device '@wan'`, `option proto 'dhcpv6'`.

DS-Lite ⚠️ (not yet checked against an AFTR): with a known AFTR, `interface wan` with `proto 'dslite'`, `peeraddr`, `tunlink 'wan6'`, `encaplimit 'ignore'`; with the AFTR from DHCPv6 (odhcp6c's `dhcpv6.script` adds the dynamic interface), `interface wan6` (`proto 'dhcpv6'`) with `iface_dslite 'wan'`, `zone_dslite 'wan'`, `encaplimit_dslite 'ignore'`.

802.1p on a VLAN device ⚠️: `list egress_qos_mapping '<skb prio>:<pcp>'` on the `8021q` device (netifd's vlandev attribute).

Remote syslog, on `system.@system[0]`: `log_ip`, `log_port`, `log_proto 'tcp'`, `log_hostname`.

Roaming options on a `wifi-iface`: `ieee80211r '1'`, `mobility_domain 'a1b2'`, `ft_over_ds '0'`, `ft_psk_generate_local '1'`, `ieee80211k '1'`, `bss_transition '1'`, `ieee80211w '0|1|2'`.

Roaming ✅ (24.10 and 25.12, D-085):
- wpad variants (`hostapd -v<feature>` and the binary's strings): `wpad-basic-mbedtls` has `11r`, `sae`, `owe`, `acs` and 802.11k (`rrm_neighbor_report`, ubus `rrm_nr_set`/`rrm_nr_get_own`), but no WNM: no `bss_transition`, `wnm_sleep_mode` or `bss_transition_request`, and no `eap`, `mesh` or `wps`. `wpad-mbedtls` (full) and `wpad-mesh-mbedtls` have WNM. There is no `-v11v`: the agent greps the binary for `wnm_sleep_mode` (`bss_transition` also matches basic's strings).
- netifd's scripts (25.12's ucode `wifi/ap.uc`, 24.10's `hostapd.sh`) write `bss_transition=1` and `wnm_sleep_mode=1` as asked, whatever hostapd was built with; a hostapd without WNM fails the BSS on the unknown key. `ieee80211k 1` sets `rrm_neighbor_report` (and on 25.12 `rrm_beacon_report`).
- With `ieee80211r 1`: `mobility_domain` defaults to `md5(ssid)`'s first 4 hex digits; `ft_psk_generate_local` defaults on for PSK alone (off for `sae-mixed` and `sae`); without `r0kh`/`r1kh` lists the scripts derive the key as `md5("<mobility_domain>/<key>")` into wildcard entries `ff:ff:ff:ff:ff:ff,*,<key>` and `00:00:00:00:00:00,00:00:00:00:00:00,<key>` (commas become spaces in hostapd's file); `ft_iface` is the network's bridge (the RRB frames' path), `reassociation_deadline` 20000 on 25.12 and 1000 on 24.10, and `nas_identifier` the BSSID without colons (25.12's `hostapd.uc`). With `sae-mixed`, `wpa_key_mgmt` becomes `SAE FT-SAE WPA-PSK WPA-PSK-SHA256 FT-PSK`.
- A station's wifi-iface takes `ieee80211r 1` for FT-SAE/FT-PSK. wpa_supplicant's ubus object `wpa_supplicant.<ifname>` has `control` (`{"command":"STATUS"}`, `SCAN`, `SCAN_RESULTS`, `ROAM <bssid>`): the images have no `wpa_cli`. `ubus subscribe hostapd.<ifname>` prints its notifications (no `timeout` in BusyBox: kill it).
- apk swaps the basic wpad for the full one in one transaction: `apk add wpad-mbedtls '!wpad-basic-mbedtls'` (checked in a 25.12 container; on hardware the post-install restarts Wi-Fi).

VLANs:
- DSA: `config bridge-vlan` with `device`, `vlan`, and `list ports 'lan1:u*'` / `'lan2:t'` (`u` untagged, `*` the port's PVID, `t` tagged). A port in no `bridge-vlan` of a filtering bridge passes nothing; render leaves such a port out of the bridge instead.
- swconfig: `config switch_vlan` with `device 'switch0'`, `vlan`, and `ports '0t 1 2'` (a port untagged in one VLAN takes it as its PVID).

DHCP options and network boot ✅ (25.12's `/etc/init.d/dnsmasq`): a pool's `list dhcp_option '6,10.0.0.1'` becomes `dhcp-option=<networkid>,6,10.0.0.1`, and a pool's `networkid` defaults to its `interface`; `config boot` with `filename`, `servername`, `serveraddress`, `networkid` becomes `dhcp-boot=tag:<networkid>,<file>,<name>,<address>`, and is skipped when it has an address but no name.

odhcpd ✅: `dhcpv6 'server|disabled'`, `ra 'server'`, `ra_slaac '0|1'` (the A flag), `list ra_flags` of `managed-config` (M), `other-config` (O), or `none`; without `ra_flags` it sets O. Stock 23.05+ LANs have `managed-config` and `other-config` (SLAAC and stateful DHCPv6).

Dropbear sets `SSH_CONNECTION` (`<client> <port> <server> <port>`) for every session, commands included: the controller's address as the node sees it.

Dumb AP: disable the `dnsmasq`, `odhcpd` and `firewall` services, set `lan` to `proto dhcp`, and bridge all ports.

Stock packages, 24.10 vs 25.12 (`crates/uci/fixtures/openwrt/`) ✅: `system.timezone` is a POSIX TZ string with the IANA name in `zonename` (25.12 writes `GMT0` and `UTC`; 24.10 only `timezone 'UTC'`); loopback and LAN addresses are `ipaddr`/`netmask` in 24.10 and `list ipaddr '<a>/<len>'` in 25.12 (both read either); odhcpd's file options differ (24.10 `leasefile '/tmp/hosts/odhcpd'`, `piofolder`; 25.12 `leasefile '/tmp/odhcpd.leases'`, `piodir`, `hostsdir`); the WAN zone forwards `REJECT` in 24.10 and `DROP` in 25.12; 25.12's `globals` holds a generated `dhcp_default_duid`. Render writes what both read and carries the node's own values over (features/lifecycle.md).

board.json ⚠️ (the swconfig shape is from OpenWrt's `config_generate`, not yet from a board): `network.<role>` has `ports` (DSA user ports) or `device`, and `macaddr`; `switch.<name>.ports[]` has `num`, and either `role` (`lan`/`wan`) with `index`, or `device` (a CPU port, with `need_tag`, `want_untag`); `switch.<name>.roles[]` has `role`, `ports` (`"1 2 3 4 0t"`, the CPU port tagged) and `device` (`eth0.1`); `system.compat_version` is what sysupgrade compares. The facts keep this as `board.layout`.

Render's assumptions, checked by the system harness (M3.3b, `crates/system`) on 24.10 and 25.12:
- ✅ A VLAN-filtering `br-lan` with each network on `br-lan.<vid>` and Ethernet trunks (`eth0:u*` for the untagged network, `:t` for the others) comes up as rendered: `bridge vlan show` lists `1 PVID Egress Untagged`, `30`, `40`, `50` on the trunk, on the gateway and the AP. The WAN on an `8021q` device (`eth1.201`) leases from an ISP's VLAN; static and PPPoE (`pppoe-wan`) come up too.
- ✅ fw4 takes a 12-character zone name whole (`input_securitycams`), and `fw4 check` passes every rendered firewall.
- ✅ An AP with dnsmasq, odhcpd and the firewall off leases its management address from the gateway, and reaches the ISP through the gateway's NAT.
- ✅ In the hwsim lab (a on 25.12 the gateway, b on 24.10 the AP): a `wifi-iface` whose network sits on `br-lan.<vid>` joins `br-lan` untagged in that VLAN (`99 PVID Egress Untagged`); `bat0` is a trunk (`1 PU`, `99`) once batman-adv creates it, though the bridge names it before; the rendered 802.11s mesh and WLANs come up, and a client's lease crosses `bat0` tagged.
- ✅ (M4.2b1, both releases) fw4 takes the rendered `redirect`s (a forward's `list proto`, the DMZ's `proto 'all'`) and rules with two ports in `dest_port` (`'1900 5351'`); `miniupnpd-nftables` (the same `upnpd` package on 23.05, 24.10 and 25.12: `config upnpd 'config'` and `perm_rule`s; init script `miniupnpd`, which writes a `uuid` into the package on its first start, finds the WAN as netifd's default route, writes `/var/etc/miniupnpd.conf` with `listening_ip=<device>` per `internal_iface` network, and puts its rules in fw4's `upnp_forward`, `upnp_prerouting` and `upnp_postrouting` chains) starts from render's package; `fw4 check` then notes miniupnpd's includes on stderr (`[!] Automatically including …`).
- ⚠️ netifd creates `<cpu>.<vid>` devices named in a bridge's ports (swconfig): containers have no switch; it waits for a swconfig board.

- ✅ Wired backhaul (D-094, D-096) in the hwsim lab (a on 25.12, b on 24.10, a veth cable `bh0` at 1532): an 802.1Q device on a port that stays a trunk in the VLAN-filtering `br-lan` gets its VID's frames (the kernel hands tagged frames to a VLAN upper before the bridge's rx handler), and batman-adv sees the other end over `batadv_hardif` on `bh0.4094` beside the trunk, then with the port out of the bridge. fw4 rejects input from a device in no zone (neighbour discovery included): a `batadv_hardif` interface in a zone resolves to its device (`iifname`). BusyBox's ping has no `-M do`; a multicast ping on `ff02::1` from a link-local gets replies from the node itself and its peers (`(DUP!)`), each `<size - 40> bytes from <addr>`. A veth drops frames above its peer's MTU. ⚠️ Still unverified on hardware: DSA programming the VID for the upper and raising the conduit for a port at 1532; a port or switch that can't take 1532 leaves the VLAN device at 1500, where batman-adv fragments.

802.11s + batman: see [features/mesh.md](../features/mesh.md).

## Wireless option names ⚠️

- `wifi-device`: `band` 2g|5g|6g|60g (replaces `hwmode`), `htmode` HT20…HE160 / EHT20…EHT320, `channel` N|auto, `country`, `cell_density` 0–3, `txpower` (dBm).
- `wifi-iface`: `mode` ap|sta|mesh|monitor, `encryption` psk2|sae|sae-mixed|owe|wpa2|wpa3…, `ieee80211w` 0|1|2. `maxassoc` N is hostapd's `max_num_sta` for the BSS (24.10, 25.12 ✅ in the scripts). OWE transition mode: two `wifi-iface` sections, an open one and an `owe` one, each with `ifname` and `owe_transition_ifname` naming the other (both releases' `hostapd.sh`; `owe_transition_ssid`/`_bssid` are the alternative). 25.12 alone also takes `owe_transition 1` on the `owe` section and makes the pair itself (the hidden BSS is `<ssid>OWE`). ⚠️ The rendered pair is unverified on a running hostapd (the Wi-Fi lab has no OWE client yet).

## Sysupgrade ⚠️

- Keeps `/etc/config` and the keep lists but drops installed packages. ASU images bake packages in; `-k` saves the package list in the backup.
- The image is staged in `/tmp`, so free RAM must exceed the image size plus a margin.
- `-T` tests an image without flashing it.

## Attended Sysupgrade (ASU) API

Read from ASU's OpenAPI (`/openapi.json`) and source (`asu/routers/api.py`, `asu/build.py`), and its live JSON (2026-11-12); the build itself is checked only against `uniwrt_firmware::testing::FakeAsu` ⚠️.

- `POST {asu}/api/v1/build` with JSON fields `version`, `target` (`mediatek/filogic`), `profile` (the image builder's, `glinet_gl-mt6000`), `packages[]` (added to the defaults; `-name` removes one, unless `diff_packages` makes the list absolute, which can drop Wi-Fi drivers), `defaults` (the first-boot uci-defaults script, at most 40960 bytes; the public server allows it: `server.allow_defaults`), `client` (`uniwrt/<version>`), optional `rootfs_size_mb`, `filesystem`, `repositories`.
  - Answers 200 with the build when it has it (cached by request: the same request returns at once), 202 while queued (`detail: "queued"`, `queue_position`) or started (`imagebuilder_status`: `init`, `container_setup`, `validate_revision`, `validate_manifest`, `building_image`, `signing_images`), 500 when it failed (`detail`, `stderr`), 400 when invalid (`detail`: unsupported version, target or profile), 422 for a malformed body (FastAPI's list of `{msg}`), 529 when its queue is full. Every answer carries `request_hash` (hex).
- `GET {asu}/api/v1/build/{request_hash}` answers the same way, and 404 once the build is gone. A done build holds the profile's `profiles.json` entry: `images[]` (`name`, `type` such as `factory`, `sysupgrade`, `kernel`, `combined-efi`; `filesystem`, `size`, `sha256`), `bin_dir` (the hash), `version_code`, `manifest`. Download images from `{asu}/store/{bin_dir}/{name}`.
- Public instance: https://sysupgrade.openwrt.org. Never send secrets to it (D-022).
- Catalog: `{asu}/json/v1/overview.json` gives `latest` (each branch's latest release: `["25.12.6","24.10.8"]`), `branches` (each with `versions`, release candidates and `-SNAPSHOT` included) and `upstream_url` (`https://downloads.openwrt.org`). `{upstream}/releases/<release>/.overview.json` lists every device of a release (about 1,900 for 25.12: `id`, `target`, `titles[]` of `vendor`, `model`, `variant`, or a `title` of its own such as `Generic x86/64`; about 390 KB). `{asu}/json/v1/releases/<release>/targets/<target>/<profile>.json` is one device's: `default_packages`, `device_packages` (`-name` removes a default), `images`, `supported_devices` (board names, `glinet,gl-mt6000`), `titles`. `{upstream}/.versions.json` lists every release, older ones included.
- Package variants: `batctl-tiny` lacks the debug tables and JSON queries; `batctl-default` has them (`*_json`), `batctl-full` adds extra tools (openwrt/routing `batctl/Makefile`).
