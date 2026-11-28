import type { BandPolicy } from "../../api/gen/BandPolicy";
import type { ChannelWidth } from "../../api/gen/ChannelWidth";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import {
  channelOf,
  fixedChannel,
  fixedPower,
  powerOf,
  RADIO_BANDS,
  type RadioBand,
} from "../../lib/radio";

/** A band's defaults as the Radios card edits them: `null` is Auto. */
export interface BandDraft {
  band: RadioBand;
  channel: number | null;
  width: ChannelWidth;
  power: number | null;
}

/** The width a band takes when the site has no policy for it (`RadioPolicy::default`). */
const DEFAULT_WIDTH: Record<RadioBand, ChannelWidth> = { "2g": 20, "5g": 80, "6g": 160 };

/** One draft per band, from the site's policies (a band without one is Auto). */
export function bandDrafts(config: SiteConfig): BandDraft[] {
  return RADIO_BANDS.map((band) => {
    const policy = config.radio.bands.find((p) => p.band === band);
    return {
      band,
      channel: policy ? fixedChannel(policy.channel) : null,
      width: policy?.width ?? DEFAULT_WIDTH[band],
      power: policy ? fixedPower(policy.tx_power) : null,
    };
  });
}

export function policyFrom(draft: BandDraft): BandPolicy {
  return {
    band: draft.band,
    channel: channelOf(draft.channel),
    width: draft.width,
    tx_power: powerOf(draft.power),
  };
}

/** `config` with the drafts' bands replaced, other bands' policies kept as they are. */
export function withBands(config: SiteConfig, drafts: readonly BandDraft[]): SiteConfig {
  const edited = new Set<string>(drafts.map((d) => d.band));
  return {
    ...config,
    radio: {
      ...config.radio,
      bands: [
        ...config.radio.bands.filter((p) => !edited.has(p.band)),
        ...drafts.map(policyFrom),
      ].sort((a, b) => a.band.localeCompare(b.band, undefined, { numeric: true })),
    },
  };
}

/** Whether the drafts differ from the site's policies. */
export const bandsChanged = (config: SiteConfig, drafts: readonly BandDraft[]) =>
  JSON.stringify(bandDrafts(config)) !== JSON.stringify(drafts);
