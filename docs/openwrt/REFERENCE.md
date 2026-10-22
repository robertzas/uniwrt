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
| `network.wireless` | status |
| `iwinfo` (rpcd-mod-iwinfo) | devices, info, assoclist, scan, freqlist, txpowerlist |
| `hostapd.{ifname}` | get_clients, get_status, del_client, bss_transition_request, rrm_nr_get_own, rrm_nr_set, rrm_nr_list, rrm_beacon_req, switch_chan, notify_response; probe/auth/assoc/disassoc events via subscribe |
| `service` (procd) | list, set, delete |
| `uci` (rpcd) | get, set, commit, apply, confirm, rollback |
| `luci-rpc` (only with LuCI) | getHostHints, getDHCPLeases, getBoardJSON |
| `dnsmasq` | lease events (availability unverified) |

## CLI

`uci` · `ubus call|listen|subscribe` · `iw dev|phy|station dump|survey dump|scan` · `iwinfo` · `batctl o|n|tg|gwl|s|tp` · `logread [-f] [-e regex]` · `sysupgrade -T|-b|-n|-k` · `firstboot` · `reload_config` · `wifi reload|up|down|status` · `fw4 check|print|reload` · `jsonfilter` · `uclient-fetch` · `owut` · `apk` / `opkg`

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
