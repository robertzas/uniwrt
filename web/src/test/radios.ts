import type { PhyFacts } from "../api/gen/PhyFacts";
import type { RadioFacts } from "../api/gen/RadioFacts";

/** A radio as the node reports it, up on its channel. */
export function radioFacts(
  extra: Partial<RadioFacts> & Pick<RadioFacts, "name" | "band">,
): RadioFacts {
  return {
    phy: "phy0",
    path: "platform/soc/wifi",
    up: true,
    disabled: false,
    restart_pending: false,
    channel: 36,
    frequency_mhz: 5180,
    center_frequency_mhz: 5210,
    width: 80,
    standard: "he",
    tx_power_dbm: 20,
    noise_dbm: -95,
    channel_busy_percent: 12,
    country: "US",
    ifaces: [],
    ...extra,
  };
}

/** A 5 GHz phy: 36 and 40 plain, 100 DFS (radar heard), 144 no-IR; up to 23 dBm and 80 MHz. */
export function phy5(name = "phy0"): PhyFacts {
  const channel = (number: number, extra: object = {}) => ({
    number,
    frequency_mhz: 5000 + number * 5,
    dfs: false,
    dfs_state: null,
    no_ir: false,
    max_tx_power_dbm: 23,
    ...extra,
  });
  return {
    name,
    bands: [
      {
        band: "5g",
        standards: ["ht", "vht", "he"],
        widths: [20, 40, 80],
        spatial_streams: 2,
        channels: [
          channel(36),
          channel(40),
          channel(100, { dfs: true, dfs_state: "unavailable" }),
          channel(144, { no_ir: true }),
        ],
      },
    ],
  };
}
