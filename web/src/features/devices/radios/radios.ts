import type { BandPolicy } from "../../../api/gen/BandPolicy";
import type { ChannelWidth } from "../../../api/gen/ChannelWidth";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { PhyBand } from "../../../api/gen/PhyBand";
import type { RadioFacts } from "../../../api/gen/RadioFacts";
import type { RadioOverride } from "../../../api/gen/RadioOverride";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import {
  BAND_CHANNELS,
  BAND_WIDTHS,
  channelOf,
  fixedChannel,
  fixedPower,
  isDfs,
  MAX_DBM,
  powerOf,
  type RadioBand,
} from "../../../lib/radio";
import { withNode } from "../../../lib/site";

/**
 * A radio's override as its dialog edits it: `"site"` follows the band's setting on the Wi-Fi
 * page (no override), `"auto"` lets the node or the RF planner choose whatever the band says.
 */
export interface RadioDraft {
  enabled: boolean;
  channel: "site" | "auto" | number;
  width: "site" | ChannelWidth;
  power: "site" | "auto" | number;
}

/** The node's override of `radio`, if any. */
export function overrideOf(
  config: SiteConfig,
  node: number,
  radio: string,
): RadioOverride | undefined {
  return config.nodes.find((n) => n.node === node)?.radios.find((r) => r.radio === radio);
}

/** The band's policy on the Wi-Fi page. */
export function policyOf(config: SiteConfig, band: string): BandPolicy | undefined {
  return config.radio.bands.find((p) => p.band === band);
}

export function radioDraft(override: RadioOverride | undefined): RadioDraft {
  const channel = override?.channel ?? null;
  const power = override?.tx_power ?? null;
  return {
    enabled: !override?.disabled,
    channel: channel === null ? "site" : (fixedChannel(channel) ?? "auto"),
    width: override?.width ?? "site",
    power: power === null ? "site" : (fixedPower(power) ?? "auto"),
  };
}

/** The draft's override, or `null` when it follows the site in everything. */
export function overrideFrom(radio: string, draft: RadioDraft): RadioOverride | null {
  if (
    draft.enabled &&
    draft.channel === "site" &&
    draft.width === "site" &&
    draft.power === "site"
  ) {
    return null;
  }
  return {
    radio,
    disabled: !draft.enabled,
    channel:
      draft.channel === "site" ? null : channelOf(draft.channel === "auto" ? null : draft.channel),
    width: draft.width === "site" ? null : draft.width,
    tx_power: draft.power === "site" ? null : powerOf(draft.power === "auto" ? null : draft.power),
  };
}

/** `config` with the node's override of `radio` replaced, or removed (`null`). */
export function withRadio(
  config: SiteConfig,
  node: number,
  radio: string,
  override: RadioOverride | null,
): SiteConfig {
  return withNode(config, node, (entry) => ({
    ...entry,
    radios: [
      ...entry.radios.filter((r) => r.radio !== radio),
      ...(override ? [override] : []),
    ].sort((a, b) => a.radio.localeCompare(b.radio)),
  }));
}

/** The radio's hardware on its band: channels, widths and power its phy reports. */
export function phyBandOf(facts: NodeFacts | null, radio: RadioFacts): PhyBand | undefined {
  return facts?.phys
    .find((phy) => phy.name === radio.phy)
    ?.bands.find((b) => b.band === radio.band);
}

/** A channel the radio may be fixed to. */
export interface ChannelChoice {
  number: number;
  /** Radar detection first: a minute's wait at each start (ten on weather channels). */
  dfs: boolean;
  /** Radar was heard on it lately: unusable until its non-occupancy period ends. */
  radar: boolean;
}

const isRadioBand = (band: string): band is RadioBand => band in BAND_CHANNELS;

/**
 * The channels the radio can start a network on: its phy's, without the ones the regulatory
 * domain forbids starting on (no-IR), else the band's whole list. A channel fixed already
 * stays offered, so the dialog can show it.
 */
export function channelChoices(
  phy: PhyBand | undefined,
  band: string,
  keep: number | null,
): ChannelChoice[] {
  const choices: ChannelChoice[] = phy
    ? phy.channels
        .filter((c) => !c.no_ir || c.number === keep)
        .map((c) => ({ number: c.number, dfs: c.dfs, radar: c.dfs_state === "unavailable" }))
    : isRadioBand(band)
      ? BAND_CHANNELS[band].map((number) => ({ number, dfs: isDfs(band, number), radar: false }))
      : [];
  if (keep !== null && !choices.some((c) => c.number === keep)) {
    choices.push({ number: keep, dfs: isDfs(band as RadioBand, keep), radar: false });
    choices.sort((a, b) => a.number - b.number);
  }
  return choices;
}

/** The widths the radio's hardware has on its band (the band's own where it doesn't say). */
export function widthChoices(phy: PhyBand | undefined, band: string): ChannelWidth[] {
  const widths = isRadioBand(band) ? BAND_WIDTHS[band] : [];
  if (!phy || phy.widths.length === 0) return [...widths];
  return widths.filter((w) => phy.widths.includes(w));
}

/** The most power to offer: the highest any of its channels allows, at most the site's 30 dBm. */
export function maxPowerOf(phy: PhyBand | undefined): number {
  const most = Math.max(...(phy?.channels.map((c) => c.max_tx_power_dbm ?? 0) ?? []), 0);
  return most > 0 ? Math.min(MAX_DBM, most) : MAX_DBM;
}

/** What the radio is fixed to by hand (its own setting, else its band's), for its row. */
export interface Pins {
  off: boolean;
  channel: number | null;
  width: ChannelWidth | null;
  power: number | null;
}

export function pinsOf(override: RadioOverride | undefined, policy: BandPolicy | undefined): Pins {
  const channel = override?.channel ?? policy?.channel ?? "auto";
  const power = override?.tx_power ?? policy?.tx_power ?? "auto";
  return {
    off: override?.disabled ?? false,
    channel: fixedChannel(channel),
    width: override?.width ?? null,
    power: fixedPower(power),
  };
}

/** Whether the radio carries an 802.11s mesh point, which keeps it on the mesh's channel. */
export const carriesMesh = (radio: RadioFacts) => radio.ifaces.some((i) => i.mode === "mesh");
