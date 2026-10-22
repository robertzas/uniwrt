# Wi-Fi: WLANs, roaming, steering & RF planning

Read this when working on SSIDs, security modes, 802.11r/k/v, steering, scans/surveys, the channel planner or floor plans.
Milestones: M2.7 (RF insight, read-only), M5.1 (WLANs), M5.2 (roaming), M5.5 (steering), M6.1–M6.3 (planner, apply, heatmap).

## WLANs (SSIDs) [M5.1]

Fields:
- SSID, enabled, network (VLAN), security, passphrase (vault)
- bands (2.4/5/6 GHz) and nodes (all | include | exclude)
- hidden, client isolation, max clients, schedule (on/off times)
- 802.11w (PMF) and roaming options
- advanced: DTIM, multicast-to-unicast, `disassoc_low_ack`, minimum basic rate, airtime fairness

Security modes: WPA2-PSK, WPA2/WPA3 transition (`sae-mixed`), WPA3-SAE (required on 6 GHz), OWE for open guest networks (with a transition SSID), and WPA2/3-Enterprise later. PPSK is open option O-8.

Rendering: one `wifi-iface` per WLAN × radio on every in-scope node, named `uw_<wlan>_<band>`, attached to the bridge of the WLAN's VLAN. SSID, key and mobility domain are identical across nodes.

Packages: 802.11k/v, SAE with FT, and OWE need full `wpad-mbedtls` instead of `wpad-basic-mbedtls`. The controller manages the swap, which briefly drops Wi-Fi, so it happens on the bench, during a maintenance window, or through a ZTP image.

## Roaming [M5.2]

- **802.11r (Fast Transition):**
  - The mobility domain is derived from the site ID (4 hex digits). `ft_over_ds 0` (over the air) is the most compatible choice.
  - WPA2-PSK uses `ft_psk_generate_local 1`. SAE uses r0kh/r1kh entries built from a site FT key the controller generates.
  - **Defaults:** on for WPA2-PSK SSIDs. **Off, with a warning,** for WPA3 and transition SSIDs, because the OpenWrt 25.12 release notes list client problems with 802.11r on WPA3. The user can turn it on per SSID.
- **802.11k:** `ieee80211k 1`. The controller computes each BSS's neighbor list (the other nodes' BSSIDs for the same SSID, ranked by measured inter-node RSSI) and pushes it with hostapd's ubus `rrm_nr_set`. No config churn; it refreshes whenever the topology changes.
- **802.11v:** `bss_transition 1` and `wnm_sleep_mode 1`.
- **Client capabilities** (11k/v/r support, bands, Wi-Fi generation) are learned from association events, shown per client, and used by steering.

## Steering & balancing [M5.5]

- **Phase 1 (default, O-2):** usteer on every AP, configured by the controller through profiles (Off / Conservative / Balanced / Aggressive). Each profile maps to thresholds: minimum SNR, roam scan/trigger SNR, band-steering threshold, load kick. usteer events feed the roaming and steering timeline.
- **Phase 2 (optional):** controller-driven smart steering that uses global history (sticky-client detection) and sends BTM requests through hostapd ubus. Only clients that advertise 11v are steered, and steering simply stops while the controller is offline.

## RF insight (read-only) [M2.7]

- **Channel survey** (`iw survey dump`) every few minutes: channel utilization and noise floor.
- **Neighbor scans** of foreign BSSs (channel, width, RSSI, SSID): staggered per radio, scheduled off-peak by default, kept 30 days.
- **Inter-node RSSI matrix:** each node's scans hear the other nodes' BSSIDs. This link-budget map also feeds 11k and the planner.
- **DFS events** (radar detected, CAC, channel switch), from hostapd events and logs.
- Views: channel occupancy per band, utilization over time, interference sources, "who is on my channel".

## RF planner: auto channel, width and power [M6.1–M6.2]

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

**Apply [M6.2]:**
1. Show a preview, then run a job.
2. Non-mesh radios change one node at a time, using CSA through hostapd `switch_chan` where supported, otherwise a reload.
3. A mesh group changes in a **synchronized cutover**: every node switches at an agreed, NTP-synced timestamp, and everything rolls back if the mesh doesn't re-form within the window.
4. A nightly optimizer runs in the maintenance window (default 03:30 local). It applies only if the score improves by at least 15% and no heavy traffic is active. This hysteresis prevents churn.

## Coverage & floor plans [M6.3]

- Upload a floor plan per floor, set its scale, and place the nodes.
- The heatmap uses a log-distance path-loss model whose per-band exponent is fitted to the measured inter-node RSSI matrix. Walls can optionally be drawn with materials.
- It shows dead zones and suggests where to add a node. Client positions are not estimated (RSSI trilateration is too noisy).

## Packages

`wpad-mbedtls` (full), `usteer`, `iw`.
