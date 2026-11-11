# Wi-Fi: WLANs, roaming, steering & RF planning

Read this when working on SSIDs, security modes, 802.11r/k/v, steering, scans/surveys, the channel planner or floor plans.
Milestones: M2.6 (RF insight, read-only), M5.1 (WLANs, client kick/block), M5.2 (roaming, steering), M6.1 (planner, apply), M6.2 (heatmap).

## WLANs (SSIDs) [M5.1]

Fields (✅ in the model and the editor; D-083):
- ✅ SSID, enabled, network (VLAN), security, passphrase (vault)
- ✅ bands (2.4/5/6 GHz) and nodes (all | only | all but)
- ✅ hidden, client isolation, max clients per radio (`maxassoc`)
- ✅ schedule: weekly on-times in the site's time zone, daylight saving included. The controller enforces the saved site's (no apply needed): outside its times a WLAN's BSSes on managed nodes go off through the agent's `bss` op (hostapd's `config_set` with netifd's config less those BSSes; no UCI change, the radio's other BSSes and its mesh point keep running), and on again at the start; a `wifi reload` or an apply brings them back on, and the controller turns them off again within half a minute. While the controller is down a WLAN stays as it is. Events: `wlan_scheduled` (site-wide), `wlan_schedule_failed`/`wlan_schedule_ok` per node and phy (D-083).
- 802.11w (PMF) follows the security mode (transition 1, WPA3 and OWE 2)
- ✅ roaming (schema v7, D-085): fast roaming (802.11r) Auto, On or Off, and roaming hints (802.11k/v) on or off; see Roaming
- advanced, later: DTIM, multicast-to-unicast, `disassoc_low_ack`, minimum basic rate, airtime fairness

Security modes: WPA2-PSK, WPA2/WPA3 transition (`sae-mixed`), WPA3-SAE (required on 6 GHz), OWE for open guest networks, OWE in transition mode (an open companion for devices without OWE), plain open (warned about), and WPA2/3-Enterprise later. PPSK is open option O-8.

Rendering: one `wifi-iface` per WLAN × radio on every in-scope node, named `uw_<wlan>_<band>`, attached to the bridge of the WLAN's VLAN. SSID, key and mobility domain are identical across nodes. OWE in transition mode is two sections that name each other's interface (`owe_transition_ifname`): `uw_<wlan>_<band>o`, open under the WLAN's name on `uw<n>-open`, and `uw_<wlan>_<band>`, hidden as `<ssid>OWE` on `uw<n>-owe`; 6 GHz has no open BSS, so it gets the encrypted one alone.

UI: `/wifi` lists the WLANs (security, bands in the band colors, network, devices, schedule, the clients on each now, the document's warnings) and warns while the site has no country or a device doesn't manage `wireless`. Admins edit a WLAN in a dialog; a new one starts from a preset (Main: WPA2/WPA3 on 2.4 and 5 GHz; Guest: OWE in transition mode, isolated, on the guest network; IoT: WPA2 on 2.4 GHz, on the IoT network) and takes its id from its name. A QR code joins a phone: for a WLAN with a passphrase it is read from the vault (admins, audited). The site's name, country (the regulatory domain) and time zone are in Settings › Site.

Packages: `wpad-basic-mbedtls` has SAE, OWE, ACS, 802.11r and 802.11k on 24.10 and 25.12; 802.11v (BSS transition, WNM sleep), 802.11s mesh, EAP and WPS need the full `wpad-mbedtls` (the mesh also runs on `wpad-mesh-mbedtls`, which has 802.11v too). Each admin swaps it per device (D-085, M5.2's second part): the swap briefly drops that device's Wi-Fi.

## Client actions [M5.1]

✅ Admins kick and block clients from the client page (D-084). Neither changes configuration: the controller asks hostapd on every node that manages `wireless`, through the agent's `ban` op (hostapd's `del_client` on each BSS, with a ban time). Nodes in observe mode are never touched.

- **Kick:** deauthenticates the client wherever it is connected and keeps it off every managed access point for a time the admin picks (no time, 1 min (the default), 10 min, 1 hour; the API takes up to a day). The answer says where it was kicked from, that it wasn't connected to a managed access point, or that no device manages Wi-Fi. Kicks are kept in the controller's memory.
- **Block / unblock:** a block is kept with the client's record (`clients.blocked`, migration 0017; blocked clients are never pruned) and lasts until an admin lifts it. It belongs to the address alone: a device with private addresses may come back under a new one. The client page shows a Blocked badge with its time, the clients list marks blocked clients and filters them.
- **Enforcement** (`uniwrt_engine::ClientBans`): hostapd keeps bans in memory and its timer overflows past about 35 minutes, so the controller sends bans of at most 10 minutes and renews each node's bans within 5 minutes of lapsing (every 30 s it compares). A node that goes offline, a job's end (an apply's `wifi reload` restarts BSSes) and a banned client seen connected to a managed node send every ban again at once (at most every 10 s). A lift is a ban of 0, which skips the BSS the client is connected to (`del_client` would kick it). While the controller is down, bans lapse within 10 minutes.
- **Events:** `client_kicked` (with the ban and the user), `client_blocked`, `client_unblocked`; `client_bans_failed` raises `node:<id>/client-bans` when a node refuses, cleared by `client_bans_ok`. Kicks, blocks and unblocks are audited (`kick_client`, `block_client`).
- **API:** `POST v1/clients/{mac}/kick` (`ClientKick`: `ban_secs`, answered with a `KickOutcome`), `PUT`/`DELETE v1/clients/{mac}/block`.

## Roaming [M5.2]

Split (D-085): roaming a ✅ (render, keys, the editor, the lab's FT roam) | roaming b (controller-pushed neighbor reports, client capabilities shown, the per-device wpad swap) | steering.

- ✅ **Per WLAN** (`Wlan.roaming`, schema v7): `fast` is 802.11r, `null` (Auto) following the security mode: on for WPA2-PSK, **off for WPA3 and transition**, because OpenWrt 25.12's release notes list clients that fail 802.11r with WPA3; the admin can force it on (validation warns) or off. Modes without a passphrase never fast-roam. `hints` (on by default) is 802.11k and 802.11v. The editor has a Roaming section: Auto/On/Off with what Auto means for the mode, and a Roaming hints checkbox.
- ✅ **802.11r (Fast Transition)**, over the air (`ft_over_ds 0`, the most compatible), in a mobility domain of 4 hex digits from the WLAN's id (FNV-1a folded to 16 bits: the same on every node, kept when the SSID is renamed). WPA2-PSK generates its keys locally (`ft_psk_generate_local 1`). SAE and the transition mode hand the keys between access points (`ft_psk_generate_local 0`, wildcard `r0kh`/`r1kh`, so the APs learn each other over the network's bridge) under the WLAN's **FT key**: HMAC-SHA256 of the controller's roaming key (32 random bytes in the vault, `controller/roaming_key`, made the first time a WLAN fast-roams) over the WLAN's id and its passphrase's vault name. A new passphrase brings a new key, and clients that know the passphrase can't derive it (OpenWrt's default derives it from the passphrase). The key holders are secret options (masked on review screens). A node without 802.11r renders the WLAN without it, with a warning.
- ✅ **802.11k:** `ieee80211k 1` wherever hostapd has it (every OpenWrt hostapd since 23.05, the basic wpad's too). Next (roaming b): the controller computes each BSS's neighbor list (the other nodes' BSSIDs for the same SSID, ranked by measured inter-node RSSI) and pushes it with hostapd's ubus `rrm_nr_set`. No config churn; it refreshes whenever the topology changes.
- ✅ **802.11v:** `bss_transition 1`, `wnm_sleep_mode 1` and `wnm_sleep_mode_no_keys 1` (no keys in WNM sleep responses, for stations that would reinstall them), only where the agent found WNM in hostapd (the `11v` capability): netifd writes the options whatever hostapd was built with, and a hostapd without WNM refuses the whole BSS. A node without it warns once per WLAN (install the full wpad).
- ✅ **Import** reads `ieee80211r` into `fast` (unset where it matches the mode's default) and `ieee80211k`/`bss_transition` into `hints`; one access point with roaming brings it to the whole WLAN, so roaming never splits one. A node's own `r0kh`/`r1kh` are dropped: the controller's keys replace them (the import's residual shows them, masked).
- ✅ **Lab:** the system test fast-roams client c's 5 GHz uplink with FT-SAE from b (24.10) to a (25.12) and back on the rendered config; each AP's hostapd reports `sta-authorized` with `auth-alg` `ft`.
- **Client capabilities** (11k/v/r support, bands, Wi-Fi generation) are learned from association events, shown per client, and used by steering (roaming b).

## Steering & balancing [M5.2]

- **Phase 1 (default, O-2):** usteer on every AP, configured by the controller through profiles (Off / Conservative / Balanced / Aggressive). Each profile maps to thresholds: minimum SNR, roam scan/trigger SNR, band-steering threshold, load kick. usteer events feed the roaming and steering timeline.
- **Phase 2 (optional):** controller-driven smart steering that uses global history (sticky-client detection) and sends BTM requests through hostapd ubus. Only clients that advertise 11v are steered, and steering simply stops while the controller is offline.

## RF insight (read-only) [M2.6]

`/insights/rf` (`features/rf`), the dashboard's RF card, and the pure `rf` crate behind them (D-064).

- **Scans** (`scan` op; `node-agent/scan.uc` in a process of its own, with its own nl80211 socket): one radio through its access point's interface (else its mesh point's), as an AP may scan (nl80211's AP and flush flags), over the band's channels its phy has; up to 20 s, aborted after. The answer carries each BSS's information elements (base64), which the controller parses (`crates/node/src/facts/ies.rs`: SSID, mesh ID, DS/HT/VHT/HE/EHT operation for the channel, width and center, the newest standard, RSN AKMs for the security, BSS load), and the phy's survey before and after the scan, whose difference is each channel's busy share and noise while it listened. Results the kernel cached from an earlier scan are left out.
- **Schedule** (D-064): background scans run each radio every 4 h (1 to 24, the `rf` setting; on by default), only while it is idle: its stations (mesh peers included) moved under 500 kbit/s in the two latest samples. A busy radio waits 10 minutes and tries again; a driver that cannot scan, 24 h. New radios are due 2 minutes after they appear, a minute apart. One scan runs at a time across the site, 30 s apart, on online nodes only. Admins queue scans of every radio, a node's or one at any time; those run first, busy or not. Scans are kept 30 days (`rf_scans`, migration 0012).
- **Report** (`uniwrt_rf::report`, rebuilt at most every 5 s and announced when it changes), per band:
  - *Radios:* channel, width, power, live busy share and noise, the foreign networks its node hears overlapping its channel, and its interference: the sum of their impacts, `weight(signal) × overlap`, where `weight` is a logistic curve from about nothing at −90 dBm to about everything at −60 dBm and `overlap` the share of the radio's channel the network covers. Under 0.3 is a clean channel, under 1 some overlap, else crowded.
  - *Channels* (each 20 MHz channel): allowed by any phy, DFS and its worst state, radar hits in 30 days (from `DFS-RADAR-DETECTED` lines and the `dfs-radar` events), foreign networks covering it, the strongest, the pressure, busy (live on a radio's channel, else from the latest survey), noise, and the site's radios on it.
  - *Networks:* every BSS heard, as heard best, with who heard it: the site's own (by BSSID), lookalikes (named like the site's networks but not adopted: waiting for adoption, or an impostor) and neighbors, with the vendor from the OUI, most disruptive first.
  - *Inter-node RSSI matrix:* how each node hears each other node: a live mesh peer link's signal first, else the strongest of the other node's BSSs in the latest scans. It is the link budget behind 11k neighbor reports and the planner (M5.2, M6.1).
- **Events** (observability.md, Events): channel changes (marked when radar came first) and congestion; radar and CAC from the log patterns.
- **API:** `GET v1/rf` (the view: report, each radio's scanning state with its last scan and next time, the settings); WS topic `rf` (the view, then each new one); `POST v1/rf/scans` (admins: `{node?, radio?}`, 202 with the radios queued); `GET v1/rf/scans?node=&radio=&before=&limit=` (stored scans, without what they heard) and `GET v1/rf/scans/{id}`; `PUT v1/rf/settings` (admins). Scans and settings changes are audited.
- **UI:** the band in the URL (`?band=2g`, 5 GHz by default). The spectrum: each network a hill over the frequencies it occupies, as high as it is heard (own in the accent, lookalikes in warning, neighbors muted), contiguous channels in segments with breaks over gaps, DFS channels and radar marked under the axis; hovering a hill or a row selects the network everywhere. The radios as cards (crowding verdict, busy, noise, interference, who shares the channel, last and next scan, a scan button for admins). The channels in use (all on demand), the matrix, each radio's busy share over time (the metric `busy`), and the networks table (all, neighbors, yours). Admins turn background scans on or off, pick the interval and scan everything now.
- Later: scan history views (a network's signal over time) and the "who is on my channel" list per client's AP (M5.2).

## RF planner: auto channel, width and power [M6.1]

**Inputs:** radios and their capabilities (channels, widths, DFS flags, max tx power), the country's regulatory domain, foreign BSSs and survey data per channel, the inter-node RSSI matrix, and client load/airtime.

**Constraints:**
- **Mesh channel groups.** Radios carrying 802.11s backhaul must share one channel per band. On dual-band nodes the 5 GHz AP shares that radio, so planning 5 GHz comes down to choosing one channel for the whole mesh group.
- **DFS policy:** avoid | allow non-weather channels | allow all. The planner accounts for CAC time and penalizes channels with a history of radar hits.
- **Pins:** a manual channel set on a radio is a hard constraint.

**Objective (minimize):**

```
Σ_pairs w(rssi_ij) · overlap(ch_i, width_i, ch_j, width_j)
+ Σ_i external_i(ch_i)
+ α · dfs_risk + β · change_cost − γ · width_benefit

w(rssi)    logistic: −90 dBm → 0, −60 dBm → 1
overlap    fraction of shared 20 MHz sub-channels
external   foreign BSSs weighted by RSSI + survey busy %
```

**Search:** exhaustive for small deployments (typically 4–6 radios per band), otherwise simulated annealing with a fixed seed. Results are deterministic and explainable: each radio gets a cost breakdown plus before/after scores.

**Widths:**
- 2.4 GHz: always 20 MHz on non-overlapping channels (1/6/11, or 1/5/9/13 where allowed).
- 5 GHz: 80 MHz by default; drop to 40/20 MHz in crowded spectrum; 160 MHz when the spectrum is clean and DFS is allowed. In the US only two 80 MHz blocks are free of DFS.
- 6 GHz: 160 MHz, or 320 MHz on Wi-Fi 7 when clean.

**Power:** aim for neighbor-AP RSSI of about −65 to −72 dBm at each AP, which gives enough cell overlap for roaming. Keep 2.4 GHz below 5 GHz to bias clients toward 5 GHz, and never go below a floor.

**Apply [M6.1]:**
1. Show a preview, then run a job.
2. Non-mesh radios change one node at a time, using CSA through hostapd `switch_chan` where supported, otherwise a reload.
3. A mesh group changes in a **synchronized cutover**: every node switches at an agreed, NTP-synced timestamp, and everything rolls back if the mesh doesn't re-form within the window.
4. A nightly optimizer runs in the maintenance window (default 03:30 local). It applies only if the score improves by at least 15% and no heavy traffic is active. This hysteresis prevents churn.

## Coverage & floor plans [M6.2]

- Upload a floor plan per floor, set its scale, and place the nodes.
- The heatmap uses a log-distance path-loss model whose per-band exponent is fitted to the measured inter-node RSSI matrix. Walls can optionally be drawn with materials.
- It shows dead zones and suggests where to add a node. Client positions are not estimated (RSSI trilateration is too noisy).

## Packages

`wpad-mbedtls` (full), `usteer`, `iw`.
