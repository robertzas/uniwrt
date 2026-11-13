/**
 * An RF view for tests: a gateway (1) and an attic AP (2). On 5 GHz the gateway's channel 36
 * (80 MHz) is shared by a strong neighbor, a lookalike of the site's network sits on 44, radar
 * was seen on 52, and the nodes hear each other (2 → 1 over the live mesh). On 2.4 GHz a café
 * sits on channel 6.
 */
import type { Breakdown } from "../api/gen/Breakdown";
import type { Bss } from "../api/gen/Bss";
import type { ChannelReport } from "../api/gen/ChannelReport";
import type { ChannelWidth } from "../api/gen/ChannelWidth";
import type { Neighbor } from "../api/gen/Neighbor";
import type { Plan } from "../api/gen/Plan";
import type { RadioReport } from "../api/gen/RadioReport";
import type { RadioScan } from "../api/gen/RadioScan";
import type { RfView } from "../api/gen/RfView";
import type { Setting } from "../api/gen/Setting";

export const RF_NOW = Date.now();
const HOUR = 3_600_000;

export function bss(extra: Partial<Bss> & Pick<Bss, "bssid" | "channel">): Bss {
  const g2 = extra.channel <= 14;
  const frequency = g2 ? 2407 + 5 * extra.channel : 5000 + 5 * extra.channel;
  return {
    ssid: null,
    mesh_id: null,
    frequency_mhz: frequency,
    center_frequency_mhz: frequency,
    width: 20,
    signal_dbm: -70,
    standard: "he",
    security: "wpa2",
    stations: null,
    utilization_percent: null,
    ...extra,
  };
}

export function neighbor(b: Bss, extra: Partial<Neighbor> = {}): Neighbor {
  return {
    bss: b,
    kind: "foreign",
    node: null,
    radio: null,
    vendor: null,
    heard_by: [{ node: 1, radio: "radio1", signal_dbm: b.signal_dbm, at: RF_NOW - 2 * HOUR }],
    impact: 0,
    ...extra,
  };
}

export function radio(
  extra: Partial<RadioReport> & Pick<RadioReport, "node" | "radio">,
): RadioReport {
  return {
    up: true,
    channel: 36,
    frequency_mhz: 5180,
    center_frequency_mhz: 5180,
    width: 20,
    tx_power_dbm: 20,
    busy_percent: 30,
    noise_dbm: -95,
    on_channel: [],
    interference: 0,
    ...extra,
  };
}

export function channel(number: number, extra: Partial<ChannelReport> = {}): ChannelReport {
  return {
    channel: number,
    frequency_mhz: number <= 14 ? 2407 + 5 * number : 5000 + 5 * number,
    allowed: true,
    dfs: false,
    dfs_state: null,
    radar_hits: 0,
    last_radar: null,
    networks: 0,
    strongest_dbm: null,
    pressure: 0,
    busy_percent: null,
    noise_dbm: null,
    ours: [],
    ...extra,
  };
}

export function radioScan(
  extra: Partial<RadioScan> & Pick<RadioScan, "node" | "radio">,
): RadioScan {
  return {
    band: "5g",
    state: "scheduled",
    last: null,
    // Half a minute over, so a slow test still reads "in 2h".
    next_at: RF_NOW + 2 * HOUR + 30_000,
    error: null,
    ...extra,
  };
}

const strong = bss({
  bssid: "aa:bb:cc:00:00:01",
  ssid: "Neighbor-5G",
  channel: 36,
  center_frequency_mhz: 5210,
  width: 80,
  signal_dbm: -58,
});
const attic = bss({ bssid: "02:00:00:00:02:01", ssid: "Home", channel: 149, signal_dbm: -61 });
const lookalike = bss({ bssid: "aa:bb:cc:00:00:02", ssid: "Home", channel: 44, signal_dbm: -80 });
const cafe = bss({ bssid: "aa:bb:cc:00:00:03", ssid: "Cafe", channel: 6, signal_dbm: -72 });

export function rfView(): RfView {
  return {
    settings: { scanning: true, interval_h: 4, dfs: "all", nightly: true, nightly_at_min: 210 },
    radios: [
      radioScan({ node: 1, radio: "radio0", band: "2g" }),
      radioScan({
        node: 1,
        radio: "radio1",
        last: {
          id: 7,
          node: 1,
          radio: "radio1",
          band: "5g",
          at: RF_NOW - 2 * HOUR,
          trigger: "scheduled",
          duration_ms: 4_800,
          networks: 3,
        },
      }),
      radioScan({ node: 2, radio: "radio1", state: "busy" }),
    ],
    report: {
      bands: [
        {
          band: "2g",
          radios: [
            radio({
              node: 1,
              radio: "radio0",
              channel: 1,
              frequency_mhz: 2412,
              center_frequency_mhz: 2412,
            }),
          ],
          channels: [1, 6, 11].map((n) => channel(n)),
          networks: [
            neighbor(cafe, {
              heard_by: [{ node: 1, radio: "radio0", signal_dbm: -72, at: RF_NOW }],
            }),
          ],
          matrix: [],
        },
        {
          band: "5g",
          radios: [
            radio({
              node: 1,
              radio: "radio1",
              width: 80,
              center_frequency_mhz: 5210,
              busy_percent: 42,
              interference: 1.2,
              on_channel: [
                {
                  bssid: strong.bssid,
                  co_channel: true,
                  overlap: 1,
                  signal_dbm: -58,
                  impact: 0.98,
                },
                {
                  bssid: lookalike.bssid,
                  co_channel: false,
                  overlap: 0.25,
                  signal_dbm: -80,
                  impact: 0.22,
                },
              ],
            }),
            radio({
              node: 2,
              radio: "radio1",
              channel: 149,
              frequency_mhz: 5745,
              center_frequency_mhz: 5745,
            }),
          ],
          channels: [
            channel(36, {
              busy_percent: 42,
              networks: 1,
              strongest_dbm: -58,
              ours: [{ node: 1, radio: "radio1", primary: true }],
            }),
            channel(44, { networks: 1, strongest_dbm: -80 }),
            channel(52, {
              dfs: true,
              dfs_state: "unavailable",
              radar_hits: 2,
              last_radar: RF_NOW - 3 * HOUR,
            }),
            channel(149, { ours: [{ node: 2, radio: "radio1", primary: true }] }),
          ],
          networks: [
            neighbor(strong, { impact: 0.98, vendor: "Netgear" }),
            neighbor(lookalike, { kind: "lookalike", impact: 0.22 }),
            neighbor(attic, { kind: "own", node: 2, radio: "radio1" }),
          ],
          matrix: [
            { from: 1, to: 2, signal_dbm: -61, source: "scan", at: RF_NOW - 2 * HOUR },
            { from: 2, to: 1, signal_dbm: -63, source: "mesh", at: RF_NOW },
          ],
        },
      ],
    },
  };
}

/**
 * A plan for {@link rfView}'s site: on 5 GHz the gateway moves off the crowded 36 to 149 at
 * 80 MHz and turns down; the attic AP, in observe mode, stays. 2.4 GHz has nothing to gain.
 */
export function rfPlan(): Plan {
  const cost = (total: number, extra: Partial<Breakdown> = {}): Breakdown => ({
    own: 0,
    external: total,
    dfs: 0,
    change: 0,
    width: 0,
    total,
    ...extra,
  });
  const setting = (channel: number, width: ChannelWidth, power: number | null): Setting => ({
    channel,
    width,
    center_frequency_mhz: 5000 + 5 * channel,
    tx_power_dbm: power,
  });
  return {
    bands: [
      {
        band: "2g",
        radios: [],
        before: 0,
        after: 0,
        improvement: 0,
        search: "fixed",
        optimal: true,
        skipped: [],
      },
      {
        band: "5g",
        radios: [
          {
            node: 1,
            radio: "radio1",
            current: setting(36, 80, 23),
            planned: setting(149, 80, 20),
            changed: true,
            before: cost(1.4),
            after: cost(0.3, { external: 0.2, change: 0.1 }),
            reasons: [{ kind: "power_lowered", heard_dbm: -58 }],
          },
          {
            node: 2,
            radio: "radio1",
            current: setting(149, 20, 20),
            planned: setting(149, 20, 20),
            changed: false,
            before: cost(0.2),
            after: cost(0.2),
            reasons: [{ kind: "observed" }],
          },
        ],
        before: 1.6,
        after: 0.5,
        improvement: 0.69,
        search: "exhaustive",
        optimal: true,
        skipped: [{ node: 2, radio: "radio2" }],
      },
    ],
  };
}
