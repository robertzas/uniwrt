import type { Band } from "../api/gen/Band";
import type { ChannelWidth } from "../api/gen/ChannelWidth";
import type { RadioChannel } from "../api/gen/RadioChannel";
import type { TxPower } from "../api/gen/TxPower";

/** The bands a site sets radios on (60 GHz isn't supported). */
export const RADIO_BANDS = ["2g", "5g", "6g"] as const satisfies readonly Band[];
export type RadioBand = (typeof RADIO_BANDS)[number];

const range = (from: number, to: number, step: number) =>
  Array.from({ length: Math.floor((to - from) / step) + 1 }, (_, i) => from + i * step);

/**
 * Each band's 20 MHz channels, as a radio whose node doesn't report its own would offer them:
 * the site's validation (`Band::has_channel`) takes these, and each node's regulatory domain
 * narrows them.
 */
export const BAND_CHANNELS: Record<RadioBand, readonly number[]> = {
  "2g": range(1, 13, 1),
  "5g": [...range(36, 64, 4), ...range(100, 144, 4), ...range(149, 177, 4)],
  "6g": range(1, 233, 4),
};

/** The widths each band has (`Band::has_width`). */
export const BAND_WIDTHS: Record<RadioBand, readonly ChannelWidth[]> = {
  "2g": [20, 40],
  "5g": [20, 40, 80, 160],
  "6g": [20, 40, 80, 160, 320],
};

/** Whether a 5 GHz channel needs radar detection (DFS), where the node doesn't say. */
export const isDfs = (band: Band, channel: number) =>
  band === "5g" && channel >= 52 && channel <= 144;

/** The site's transmit power range, in dBm (validation's). */
export const MAX_DBM = 30;

/** A fixed channel's number, or `null` for Auto. */
export const fixedChannel = (channel: RadioChannel): number | null =>
  channel === "auto" ? null : channel.fixed;

/** A fixed power's dBm, or `null` for Auto. */
export const fixedPower = (power: TxPower): number | null => (power === "auto" ? null : power.dbm);

export const channelOf = (n: number | null): RadioChannel => (n === null ? "auto" : { fixed: n });
export const powerOf = (dbm: number | null): TxPower => (dbm === null ? "auto" : { dbm });
