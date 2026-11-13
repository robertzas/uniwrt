import type { BandPlan } from "../../api/gen/BandPlan";
import type { PlannedRadio } from "../../api/gen/PlannedRadio";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { TxPower } from "../../api/gen/TxPower";

/** A planned power as the site stores it: the radio's maximum, or 1 to 30 dBm. */
function power(dbm: number | null): TxPower {
  return dbm === null || dbm < 1 ? "auto" : { dbm: Math.min(dbm, 30) };
}

/**
 * The site's document following the band's plan (features/wifi.md, RF planner: Apply; D-112):
 * the band's entries replaced by every radio the controller may change (observed ones keep what
 * they run, so they aren't stored), and the mesh's channel and width from its group. Where the
 * plan has no mesh radio (all down), the mesh keeps the channel stored before.
 */
export function withBandPlan(config: SiteConfig, plan: BandPlan): SiteConfig {
  const ours = plan.radios.filter((r) => !r.reasons.some((reason) => reason.kind === "observed"));
  const radios: PlannedRadio[] = [
    ...config.radio.plan.radios.filter((r) => r.band !== plan.band),
    ...ours.map((r) => ({
      node: r.node,
      radio: r.radio,
      band: plan.band,
      channel: r.planned.channel,
      width: r.planned.width,
      tx_power: power(r.planned.tx_power_dbm),
    })),
  ].sort((a, b) => a.node - b.node || a.radio.localeCompare(b.radio));
  const mesh = plan.radios.find((r) => r.reasons.some((reason) => reason.kind === "mesh_group"));
  return {
    ...config,
    radio: {
      ...config.radio,
      plan: {
        radios,
        mesh: mesh
          ? { band: plan.band, channel: mesh.planned.channel, width: mesh.planned.width }
          : config.radio.plan.mesh,
      },
    },
  };
}

/** Whether the plan has a radio the controller may change. */
export function usable(plan: BandPlan): boolean {
  return plan.radios.some((r) => !r.reasons.some((reason) => reason.kind === "observed"));
}
