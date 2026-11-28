import type { NodeSummary } from "../../api/gen/NodeSummary";
import type { Graph } from "../../viz/topology/graph";

/** The *Get started* checklist's steps, in the order they are best taken (D-148). */
export const STEPS = ["aps", "wifi", "mesh", "manage", "backups", "notify"] as const;
export type StepId = (typeof STEPS)[number];

export interface Step {
  id: StepId;
  done: boolean;
  /** Where the step is taken. */
  href: string;
  /** How far it got, where it counts something (the managed nodes). */
  progress?: { done: number; total: number };
}

/** What the steps tick from; `undefined` where it hasn't been read (yet, or it failed). */
export interface GetStartedInput {
  nodes: readonly NodeSummary[];
  graph: Graph | undefined;
  /** The site's WLANs, and whether its mesh is on. */
  site: { wlans: number; mesh: boolean } | undefined;
  /** Whether the controller's backups have their passphrase (they run nightly once it is set). */
  backups: boolean | undefined;
  notifiers: number | undefined;
}

const isAp = (node: NodeSummary) => node.role !== "gateway";

/** Whether `id`'s vertex has a cable that is up: the node reaches the network without the mesh. */
function wired(graph: Graph, id: number): boolean {
  const vertex = `node:${id}`;
  for (const edge of graph.edges.values()) {
    if (edge.medium === "ethernet" && edge.up && (edge.a === vertex || edge.b === vertex)) {
      return true;
    }
  }
  return false;
}

/**
 * The checklist from live state, or `undefined` while a step can't be told yet. Steps tick
 * themselves: access points adopted, a WLAN in the site, the mesh on (or seen, or every access
 * point on a cable), every node managed, the backups' passphrase set, a notifier.
 */
export function getStarted(input: GetStartedInput): Step[] | undefined {
  const { site, backups, notifiers, graph } = input;
  if (site === undefined || backups === undefined || notifiers === undefined) return undefined;
  const aps = input.nodes.filter(isAp);
  const meshLinks =
    graph !== undefined &&
    [...graph.edges.values()].some(
      (edge) => edge.medium === "mesh" && edge.a.startsWith("node:") && edge.b.startsWith("node:"),
    );
  const allWired = graph !== undefined && aps.length > 0 && aps.every((ap) => wired(graph, ap.id));
  const managed = input.nodes.filter((node) => node.mode === "managed").length;
  return [
    { id: "aps", done: aps.length > 0, href: "/devices?adopt=1" },
    { id: "wifi", done: site.wlans > 0, href: "/wifi" },
    { id: "mesh", done: site.mesh || meshLinks || allWired, href: "/wifi" },
    {
      id: "manage",
      done: input.nodes.length > 0 && managed === input.nodes.length,
      href: "/devices",
      progress: { done: managed, total: input.nodes.length },
    },
    { id: "backups", done: backups, href: "/settings?tab=backups" },
    { id: "notify", done: notifiers > 0, href: "/settings?tab=notifications" },
  ];
}
