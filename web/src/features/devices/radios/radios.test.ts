import { describe, expect, it } from "vitest";
import { phy5, radioFacts } from "../../../test/radios";
import { siteConfig } from "../../../test/site";
import {
  channelChoices,
  maxPowerOf,
  overrideFrom,
  pinsOf,
  radioDraft,
  widthChoices,
  withRadio,
} from "./radios";

const band = phy5().bands[0];

describe("radio overrides", () => {
  it("round-trips a draft, and drops an override that follows the site", () => {
    const override = {
      radio: "radio1",
      disabled: false,
      channel: "auto" as const,
      width: 40 as const,
      tx_power: { dbm: 17 },
    };
    const draft = radioDraft(override);
    expect(draft).toEqual({ enabled: true, channel: "auto", width: 40, power: 17 });
    expect(overrideFrom("radio1", draft)).toEqual(override);
    expect(radioDraft(undefined)).toEqual({
      enabled: true,
      channel: "site",
      width: "site",
      power: "site",
    });
    expect(overrideFrom("radio1", radioDraft(undefined))).toBeNull();
    expect(overrideFrom("radio1", { ...radioDraft(undefined), enabled: false })).toEqual({
      radio: "radio1",
      disabled: true,
      channel: null,
      width: null,
      tx_power: null,
    });
  });

  it("replaces a node's override, adding the node's entry when it has none", () => {
    const override = overrideFrom("radio1", {
      enabled: true,
      channel: 36,
      width: "site",
      power: "site",
    });
    const config = withRadio(siteConfig(), 7, "radio1", override);
    const entry = config.nodes.find((n) => n.node === 7);
    expect(entry?.radios).toEqual([override]);
    expect(entry?.managed).toEqual([]);
    const cleared = withRadio(config, 7, "radio1", null);
    expect(cleared.nodes.find((n) => n.node === 7)?.radios).toEqual([]);
  });

  it("offers the phy's channels without no-IR ones, and its widths and power", () => {
    expect(channelChoices(band, "5g", null)).toEqual([
      { number: 36, dfs: false, radar: false },
      { number: 40, dfs: false, radar: false },
      { number: 100, dfs: true, radar: true },
    ]);
    // A channel fixed already stays offered.
    expect(channelChoices(band, "5g", 144).map((c) => c.number)).toEqual([36, 40, 100, 144]);
    expect(channelChoices(band, "5g", 165).map((c) => c.number)).toContain(165);
    // Without the phy, the band's list.
    expect(channelChoices(undefined, "2g", null)).toHaveLength(13);
    expect(channelChoices(undefined, "5g", null).find((c) => c.number === 52)?.dfs).toBe(true);
    expect(widthChoices(band, "5g")).toEqual([20, 40, 80]);
    expect(widthChoices(undefined, "6g")).toEqual([20, 40, 80, 160, 320]);
    expect(maxPowerOf(band)).toBe(23);
    expect(maxPowerOf(undefined)).toBe(30);
  });

  it("pins the radio's own settings over its band's", () => {
    const policy = {
      band: "5g" as const,
      channel: { fixed: 149 },
      width: 80 as const,
      tx_power: { dbm: 20 },
    };
    expect(pinsOf(undefined, policy)).toEqual({ off: false, channel: 149, width: null, power: 20 });
    const own = {
      radio: "radio1",
      disabled: false,
      channel: "auto" as const,
      width: 40 as const,
      tx_power: null,
    };
    expect(pinsOf(own, policy)).toEqual({ off: false, channel: null, width: 40, power: 20 });
    expect(radioFacts({ name: "radio1", band: "5g" }).band).toBe("5g");
  });
});
