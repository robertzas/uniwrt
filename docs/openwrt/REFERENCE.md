# OpenWrt reference

These are the facts uniwrt relies on, collected here so sessions don't research them again.
Marks: ✅ verified (release notes, a test, or a real device) · ⚠️ from documentation or memory. Verify ⚠️ items in the system harness (M3.7) or on a real node before depending on them, then flip the mark.

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
| `/etc/board.json` | Model, LAN/WAN port names, switch layout, LEDs |
| `/etc/openwrt_release`, `/etc/os-release` | Version, target, architecture |
| `/tmp/dhcp.leases` | dnsmasq leases (gateway) |
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
| `network.interface`, `network.interface.{name}` | dump, status, up, down |
| `network.device` | status |
| `network.wireless` | status ✅: per radio `up`, `disabled`, `config` (UCI options, the Wi-Fi keys included) and `interfaces` (`section`, `ifname`, `config`). 25.12 images without wifi-scripts have no such object |
| `iwinfo` (rpcd-mod-iwinfo) | devices, info, assoclist, scan, freqlist, txpowerlist |
| `hostapd.{ifname}` | get_clients ✅ (`clients.<mac>`: `authorized`, `wmm`, `ht`, `vht`, `he`, `mfp`, `rrm` (5 bytes of RRM capabilities), `extended_capabilities` (bit 19: BSS transition), `aid`, `bytes`, `packets`, `rate`, `signal`; nothing says whether FT was used), get_status ✅ (`freq`, `channel`, `op_class`, `airtime`, `dfs`),  get_status, del_client, bss_transition_request, rrm_nr_get_own, rrm_nr_set, rrm_nr_list, rrm_beacon_req, switch_chan, notify_response; probe/auth/assoc/disassoc events via subscribe |
| `service` (procd) | list, set, delete |
| `uci` (rpcd) | get, set, commit, apply, confirm, rollback |
| `luci-rpc` (only with LuCI) | getHostHints, getDHCPLeases, getBoardJSON |
| `dnsmasq` | lease events (availability unverified); its only method is `metrics` ✅ |
| `dhcp` (odhcpd) ✅ | `ipv6leases` (`device.<dev>.leases[]`: `duid`, `iaid`, `hostname`, `ipv6-addr[]` with `valid-lifetime`, `valid`); `ipv4leases` only where odhcpd may serve DHCPv4 (in 24.10, not in 25.12); `add_lease`, `ipv6ra` |
| `log` (logd) ✅ | `read` with `{"lines":N,"stream":false}`: `log[]` entries with `id` (a sequence), `priority`, `source`, `time` (ms) and `msg` (`program[pid]: text`) |

## CLI

`uci` · `ubus call|listen|subscribe` · `batctl meshif <dev> mj|hj|oj|nj|gwj|tgj|tlj` (JSON ✅: throughputs and gateway bandwidths in 100 kbit/s, translation-table VLAN -1 for untagged; `batctl s` has no JSON) · `iw dev|phy|station dump|survey dump|scan` · `iwinfo` · `batctl o|n|tg|gwl|s|tp` · `logread [-f] [-e regex]` · `sysupgrade -T|-b|-n|-k` · `firstboot` · `reload_config` · `wifi reload|up|down|status` · `fw4 check|print|reload` · `jsonfilter` · `uclient-fetch` · `owut` · `apk` / `opkg`

## ucode ✅

Verified in the 24.10.8 and 25.12.6 rootfs containers (M1.3).

- Modules: `fs`, `ubus`, `uci`, `math` and `html` in both. The 25.12 rootfs adds `uloop`, `log` and `uclient`, and the 24.10 one has no `uloop`. ⚠️ Wi-Fi images have `uloop` through hostapd's dependencies.
- Modules export with `export { a, b };`, because `export function` doesn't parse. `import { x } from "./file.uc"` resolves relative to the importing file, `sourcepath(0, true)` is the script's directory, and `ARGV` holds the script's arguments.
- `sprintf("%J", v)` prints one-line JSON. Bytes that aren't UTF-8 come out re-encoded as Latin-1 on 24.10 and raw on 25.12, so the controller decodes lossily.
- `system(argv, timeout_ms)` runs without a shell. On timeout it kills with SIGKILL and returns -9, and a process ended by a signal returns `-signal`. The child inherits stdout, so redirect it.
- `fs.readfile(path, limit)`, `fs.readlink`, `fs.lsdir`, `fs.glob`, `fs.access`, `b64enc` and `clock()` (`[s, ns]`; `clock(true)` is monotonic) exist in both.
- uloop: a buffered `read()` on a non-blocking descriptor drops data at EAGAIN, so stdin needs blocking reads in a forked `uloop.task`. A task's stdio is `/dev/null`; it reads the pipe through `/proc/self/fd/0`, reopened before the fork. `uloop.process` reports `WEXITSTATUS` (0 after a signal) and has no `kill()`.
- nl80211 and rtnl (ucode-mod-nl80211 and ucode-mod-rtnl, which hostapd depends on, so on Wi-Fi images only) ✅: attributes are named in lower case without their prefix (`wiphy_freq`, `channel_width`, `wiphy_tx_power_level`, `sta_info.signal_avg`, `plink_state`). Bitrates are in 100 kbit/s, with the flags `width_40|80|160|320`, `40_mhz_width`, `short_gi`, and `he_gi`/`eht_gi`; powers are in mBm; `sta_flags` is `[mask, set]`. The survey has no `in_use`, so match the interface's frequency. A split `GET_WIPHY` dump starts with a null entry, and `wiphy_bands` is indexed by `enum nl80211_band`. An `AF_UNSPEC` neighbor dump has no bridge FDB; ask with `family: AF_BRIDGE`.
- `fs.realpath` exists in both releases ✅.
- ubus: `conn.call()` returns null both for a failure and for an empty reply. `conn.error()` returns the last error and clears it; messages start with the `ubus_strerror()` text, such as `Not found: …` or `Method not found: …`.

## Containers as test nodes ✅

- An `openwrt/rootfs` image boots with `/sbin/init` as PID 1 in an unprivileged container: procd, ubusd, rpcd, logd, netifd, odhcpd, uhttpd and dropbear run. dnsmasq crash-loops: procd jails it with ujail, which cannot make namespaces in a container ✅. With `--cap-add NET_ADMIN`, netifd builds `br-lan` from eth0.
- The first boot generates `/etc/config/network` (`config_generate`) and then runs `/etc/uci-defaults/*`. Change the network config there, not before boot.
- BusyBox has `tar`, `sha256sum -c -s`, `flock`, `ulimit -f` (512-byte blocks) and `set -o pipefail`. It lacks `timeout`, `stat`, `od` and fractional `sleep`.
- The 24.10 rootfs has opkg (`/bin/opkg`), and the 25.12 one has apk (`/usr/bin/apk`).

- ✅ Wi-Fi in containers (the hwsim lab, D-035): radios moved into a container's network namespace (`iw phy <phy> set netns <pid>`) appear in its `/sys/class/ieee80211`. netifd checks the Wi-Fi daemons it starts through `/proc/<pid>/exe`, which needs `--cap-add SYS_PTRACE`. hostapd and wpa_supplicant log harmless errors writing `/proc/sys`, which is read-only. A container's interfaces come in no fixed order when several Docker networks are attached before it starts, so the lab links nodes with a veth pair instead. hwsim radios are tri-band (2g, 5g, 6g, with EHT) and hear each other across namespaces.

## procd, netifd and sysfs

- ✅ batman-adv's mesh device has no DEVTYPE, and batman-adv has no sysfs or debugfs tables any more; its hard interfaces are its `lower_*` links, rtnetlink's link kind is `batadv`, and netifd names the device after its `proto batadv` interface.
- ✅ A phy's `device` link resolves (`fs.realpath`) to `/sys/devices/<path>`, and OpenWrt's radio `path` is that `<path>` (`virtual/mac80211_hwsim/hwsim0`); ⚠️ later phys of one device get `+1`, `+2`.
- ✅ `system info`: memory in bytes, `root` and `tmp` in KiB, and `load` scaled by 65536.
- ✅ `network.interface dump`: addresses are `{address, mask}`. A LAN's IPv6 addresses are the `local-address` of each `ipv6-prefix-assignment`, and delegated prefixes are under `ipv6-prefix`. Default routes have target `0.0.0.0` or `::` with mask 0.
- ✅ `/sys/class/net`: `DEVTYPE` is in `uevent` (`bridge`, `vlan`, `wlan`). `carrier` and `speed` can't be read while a device is down, and bridges and VLANs report their ports' speed. `master` links to the bridge or batman device, and `bonding_masters` may sit beside the devices.
- ⚠️ DSA user ports have `DEVTYPE=dsa`.
- ⚠️ `hostapd -v<feature>` and `wpa_supplicant -v<feature>` exit 0 when the feature is built in (LuCI checks it the same way).

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

6rd (Lumen/CenturyLink values, community-sourced):

```
config interface 'wan6rd'
	option proto '6rd'
	option peeraddr '205.171.2.64'
	option ip6prefix '2602::'
	option ip6prefixlen '24'
	option ip4prefixlen '0'
```

Remote syslog, on `system.@system[0]`: `log_ip`, `log_port`, `log_proto 'tcp'`, `log_hostname`.

Roaming options on a `wifi-iface`: `ieee80211r '1'`, `mobility_domain 'a1b2'`, `ft_over_ds '0'`, `ft_psk_generate_local '1'`, `ieee80211k '1'`, `bss_transition '1'`, `ieee80211w '0|1|2'`.

VLANs:
- DSA: `config bridge-vlan` with `device`, `vlan`, and `list ports 'lan1:u*'` / `'lan2:t'`.
- swconfig: `config switch_vlan` with `device 'switch0'`, `vlan`, and `ports '0t 1 2'`.

Dumb AP: disable the `dnsmasq`, `odhcpd` and `firewall` services, set `lan` to `proto dhcp`, and bridge all ports.

802.11s + batman: see [features/mesh.md](../features/mesh.md).

## Wireless option names ⚠️

- `wifi-device`: `band` 2g|5g|6g|60g (replaces `hwmode`), `htmode` HT20…HE160 / EHT20…EHT320, `channel` N|auto, `country`, `cell_density` 0–3, `txpower` (dBm).
- `wifi-iface`: `mode` ap|sta|mesh|monitor, `encryption` psk2|sae|sae-mixed|owe|wpa2|wpa3…, `ieee80211w` 0|1|2.

## Sysupgrade ⚠️

- Keeps `/etc/config` and the keep lists but drops installed packages. ASU images bake packages in; `-k` saves the package list in the backup.
- The image is staged in `/tmp`, so free RAM must exceed the image size plus a margin.
- `-T` tests an image without flashing it.

## Attended Sysupgrade (ASU) API ⚠️

- `POST {asu}/api/v1/build` with JSON fields `target`, `profile`, `version`, `packages[]`, `defaults` (first-boot uci-defaults script), optional `rootfs_size_mb`, and `diff_packages`.
  - Returns 200 for a cached build, or 202 when queued (poll), with a `request_hash`.
- `GET {asu}/api/v1/build/{request_hash}` returns the images, each with a `sha256`. Download them from `{asu}/store/{bin_dir}/{name}`.
- Public instance: https://sysupgrade.openwrt.org. Never send secrets to it.
- Catalog: https://downloads.openwrt.org/.versions.json plus per-target `profiles.json`.
