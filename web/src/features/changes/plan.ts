import type { ApplyPlan } from "../../api/gen/ApplyPlan";
import type { NodePlan } from "../../api/gen/NodePlan";
import type { PlanAction } from "../../api/gen/PlanAction";
import { strings } from "../../lib/strings";

const t = strings.changes;

/** How many nodes take each action. */
export function counts(plan: ApplyPlan): Record<PlanAction, number> {
  const out: Record<PlanAction, number> = { change: 0, unchanged: 0, skipped: 0, refused: 0 };
  for (const node of plan.nodes) out[node.action] += 1;
  return out;
}

/** The nodes the apply acts on, in its order (refused ones too), and the ones it leaves alone. */
export function split(plan: ApplyPlan): { acted: NodePlan[]; alone: NodePlan[] } {
  const alone = (node: NodePlan) => node.action === "unchanged" || node.action === "skipped";
  return { acted: plan.nodes.filter((n) => !alone(n)), alone: plan.nodes.filter(alone) };
}

/** The longest dead-man timer among the nodes that change. */
export function longestTimer(plan: ApplyPlan): number | undefined {
  const timers = plan.nodes.filter((n) => n.action === "change").map((n) => n.timeout_s);
  return timers.length === 0 ? undefined : Math.max(...timers);
}

export type ImpactKind =
  | "network"
  | "wireless"
  | "channels"
  | "dhcp"
  | "firewall"
  | "system"
  | "other"
  | "service"
  | "address"
  | "drift"
  | "warnings";

export interface Impact {
  kind: ImpactKind;
  text: string;
  /** It interrupts traffic or clients for a moment. */
  disruptive: boolean;
}

/** The packages in the order their impact reads best: what interrupts traffic first. */
const PACKAGES = ["network", "wireless", "dhcp", "firewall", "system"] as const;
type Known = (typeof PACKAGES)[number];
const isKnown = (pkg: string): pkg is Known => (PACKAGES as readonly string[]).includes(pkg);

/** The impact summary: what each change does to the devices, in words. */
export function impacts(plan: ApplyPlan): Impact[] {
  const changing = plan.nodes.filter((n) => n.action === "change");
  const names = (nodes: readonly NodePlan[]) => t.impact.names(nodes.map((n) => n.name));
  const out: Impact[] = [];

  const byPackage = new Map<string, NodePlan[]>();
  for (const node of changing) {
    for (const diff of node.packages) {
      // Its radios move by a channel switch, one device at a time (below).
      if (node.air && diff.package === "wireless") continue;
      byPackage.set(diff.package, [...(byPackage.get(diff.package) ?? []), node]);
    }
  }
  for (const pkg of PACKAGES) {
    const nodes = byPackage.get(pkg);
    if (nodes) {
      out.push({
        kind: pkg,
        text: t.impact[pkg](names(nodes)),
        disruptive: pkg === "network" || pkg === "wireless",
      });
    }
  }
  const moving = changing.filter((n) => n.air);
  if (moving.length > 0) {
    out.push({ kind: "channels", text: t.impact.channels(names(moving)), disruptive: false });
  }
  for (const [pkg, nodes] of byPackage) {
    if (!isKnown(pkg)) {
      out.push({ kind: "other", text: t.impact.other(pkg, names(nodes)), disruptive: false });
    }
  }

  const services = new Map<string, NodePlan[]>();
  for (const node of changing) {
    for (const change of node.services) {
      const key = `${change.enable ? "on" : "off"}:${change.name}`;
      services.set(key, [...(services.get(key) ?? []), node]);
    }
  }
  for (const [key, nodes] of [...services].sort(([a], [b]) => a.localeCompare(b))) {
    const [state, name = ""] = key.split(":");
    const text = state === "on" ? t.impact.serviceOn : t.impact.serviceOff;
    out.push({ kind: "service", text: text(name, names(nodes)), disruptive: false });
  }

  for (const node of changing) {
    if (node.address) {
      out.push({
        kind: "address",
        text: t.impact.address(node.name, node.address.from, node.address.to),
        disruptive: false,
      });
    }
  }
  const drifted = changing.filter((node) => overwrites(node).length > 0);
  if (drifted.length > 0) {
    const n = drifted.reduce((sum, node) => sum + overwrites(node).length, 0);
    out.push({ kind: "drift", text: t.impact.drift(n, names(drifted)), disruptive: false });
  }
  const warnings = plan.nodes.reduce((sum, n) => sum + n.warnings.length, 0);
  if (warnings > 0)
    out.push({ kind: "warnings", text: t.impact.warnings(warnings), disruptive: false });
  return out;
}

/** The node's drifted packages this apply rewrites: those that change (lifecycle.md, Drift). */
export function overwrites(node: NodePlan): string[] {
  return node.drift.filter((pkg) => node.packages.some((diff) => diff.package === pkg));
}
