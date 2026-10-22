# Product

## Vision

uniwrt makes a home full of OpenWrt routers feel like one premium, self-driving network. You adopt devices in minutes, see every link and client live, change anything safely from one place, and fix problems before anyone asks "is the Wi-Fi down?". It is self-hosted, open and efficient.

## Goals

1. **Complete management.** Everything needed to build, run and maintain an OpenWrt mesh, so the owner never logs into an individual router. Escape hatches exist, but they should feel unnecessary.
2. **Safety.** Transactional changes, automatic rollback, previews and history. Nodes are observe-only until trusted.
3. **Insight.** Live topology, client experience, the RF environment, logs and metrics, each with explanations.
4. **Delight.** A professional, modern, fast UI with signature visualizations.
5. **Efficiency.** A tiny static binary with low RAM and CPU use on both the controller and the nodes.

## Non-goals (v1)

- Multi-tenant/MSP setups or cloud hosting.
- Managing non-OpenWrt devices. They appear only as clients or inferred switches.
- Deep packet inspection or application identification.
- Replacing LuCI for every niche package. Raw UCI covers the long tail.
- Enterprise RADIUS/802.1X (backlog).

## Users and scale

The primary user is a technically minded home owner or prosumer running 1 gateway and N APs. The reference deployment is a gateway plus 3 mesh APs. uniwrt is designed for 1–50 nodes, up to 1,000 clients and one site; multi-site support is in the backlog.

## Product principles

1. **Safe by default:** preview, transaction, rollback, audit.
2. **Live:** the UI reflects reality within about a second, with no refresh buttons.
3. **Explain:** every automatic decision (RF plan, steering, alert) shows why it was made.
4. **Progressive disclosure:** strong defaults, with an advanced drawer for everything else.
5. **No dead ends:** every problem view offers a fix or the next diagnostic step.
6. **One visual language:** the same colors mean the same things everywhere (bands, status, link quality).
7. **Fast:** interactions under 100 ms, small bundles, a small binary.
8. **Data-driven extensibility:** ISP presets, log patterns, collectors and alert rules are data files.

## Capability catalog

| Area | Capabilities | Milestones | Spec |
|---|---|---|---|
| Adoption | adopt existing nodes (observe) · ZTP images · adoption port · one-liner · gateway first-run · pending-adoption inbox · roles (gateway/AP/bridge) | M1.7, M4.4, M5.4 | features/adoption.md |
| Internet | ISP presets · DHCP/PPPoE/static/VLAN/6rd/DHCPv6-PD · live connectivity test · SQM · failover | M4.1, M4.5 | features/wan.md |
| Networks | 10.0.0.0/16 IPAM · VLAN networks (LAN/IoT/Guest) · DHCP and reservations · local DNS · IPv6 · firewall matrix · port forwards · UPnP · WireGuard · DNS filtering · DDNS · mDNS · WoL | M4.2–M4.3, M8.1–M8.3 | features/network.md |
| Wi-Fi | SSIDs and security · 802.11r/k/v · steering · RF insight · auto channel/width/power · floor-plan heatmap | M2.7, M5.1–M5.5, M6 | features/wifi.md |
| Mesh | batman-adv over 802.11s and wired links · VLANs over the mesh · mesh health and single-point-of-failure detection | M2, M5.3 | features/mesh.md |
| Observability | live topology with replay · clients and experience score · metrics · logs · events and alerts · dashboard | M1.4, M2 | features/observability.md |
| Troubleshooting | diagnostics toolkit · packet capture · web terminal · connection doctor · WAN quality monitor · support bundle | M8.4–M8.5 | features/observability.md |
| Lifecycle | revisions · review and apply · drift detection · brownfield import · rolling upgrades via ASU · backups · replace/RMA | M3, M7 | features/lifecycle.md |
| Platform | auth and TLS · secrets vault · demo mode · scratch multi-arch image · a release per commit | M0, M1 | features/security.md, dev/BUILD.md |
| UI | design system · ⌘K command palette · staged changes · signature visualizations | M0.3, M1.8, all | features/ui.md |
