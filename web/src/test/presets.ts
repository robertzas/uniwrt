import type { Preset } from "../api/gen/Preset";
import type { WanConfig } from "../api/gen/WanConfig";

/** ISP presets as `GET /api/v1/isp/presets` lists them (crates/isp/data). */
export const LUMEN: Preset = {
  id: "us-lumen-ipoe",
  name: "Lumen / Quantum Fiber / CenturyLink Fiber",
  variant: "IPoE",
  country: "US",
  status: "community",
  sources: ["https://minus273.org/project/openwrt-centurylink/"],
  notes: "Replaces the ISP router.",
  prompts: [],
  ipv4: { proto: "dhcp", vlan: 201, pcp: null, mtu: null, dhcp: null, service: null, aftr: null },
  ipv6: {
    mode: "auto",
    reqprefix: null,
    sixrd: { prefix: "2602::/24", border_relay: "205.171.2.64", ipv4_mask_len: 0 },
  },
  hints: {},
};

export const PPPOE: Preset = {
  id: "generic-pppoe",
  name: "DSL and PPPoE fiber",
  variant: "PPPoE",
  country: null,
  status: "verified",
  sources: [],
  notes: null,
  prompts: ["username", "password"],
  ipv4: { proto: "pppoe", vlan: null, pcp: null, mtu: 1492, dhcp: null, service: null, aftr: null },
  ipv6: { mode: "dhcpv6", reqprefix: null, sixrd: null },
  hints: {},
};

export const DEFAULT_WAN: WanConfig = {
  proto: { kind: "dhcp", vendor_class: null, client_id: null, send_options: [], hostname: null },
  vlan: null,
  pcp: null,
  mtu: null,
  mac: null,
  ipv6: { mode: "dhcpv6", reqprefix: null },
  preset: null,
};
