import type { NodeConfig } from "../../../api/gen/NodeConfig";
import type { NodeReboot } from "../../../api/gen/NodeReboot";
import type { RebootTime } from "../../../api/gen/RebootTime";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import { type TimeDraft, timeDraft, timeOf } from "../../../lib/schedule";

/** A device's schedules as typed (D-138). */
export interface NodeSchedulesDraft {
  reboot: NodeReboot["kind"];
  own: TimeDraft;
  leds: boolean;
}

/** The node's entry, or the one it has when the site lists none. */
export function entryOf(config: SiteConfig, node: number): NodeConfig {
  return (
    config.nodes.find((entry) => entry.node === node) ?? {
      node,
      hostname: null,
      managed: [],
      radios: [],
      ports: [],
      raw: [],
      reboot: { kind: "site" },
      led_schedule: true,
      ssh: { key_only: false, management_only: false },
    }
  );
}

/** The entry's schedules as typed; an own time starts from the site's, if it has one. */
export function nodeSchedulesDraft(entry: NodeConfig, site: RebootTime | null): NodeSchedulesDraft {
  const reboot = entry.reboot;
  return {
    reboot: reboot.kind,
    own: timeDraft(reboot.kind === "own" ? reboot : site),
    leds: entry.led_schedule,
  };
}

/** The draft's `reboot`, or what is wrong with its own time. */
export function rebootOf(draft: NodeSchedulesDraft): NodeReboot | string {
  if (draft.reboot !== "own") return { kind: draft.reboot };
  const time = timeOf(draft.own);
  return typeof time === "string" ? time : { kind: "own", ...time };
}

/** `config` with the node's schedules replaced; a node without an entry gets one. */
export function withSchedules(
  config: SiteConfig,
  node: number,
  reboot: NodeReboot,
  ledSchedule: boolean,
): SiteConfig {
  const known = config.nodes.some((entry) => entry.node === node);
  const nodes = known ? config.nodes : [...config.nodes, entryOf(config, node)];
  return {
    ...config,
    nodes: nodes.map((entry) =>
      entry.node === node ? { ...entry, reboot, led_schedule: ledSchedule } : entry,
    ),
  };
}
