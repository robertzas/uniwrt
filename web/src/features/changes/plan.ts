import type { ApplyPlan } from "../../api/gen/ApplyPlan";
import type { Diff } from "../../api/gen/Diff";
import type { NodePlan } from "../../api/gen/NodePlan";
import type { OptionDiff } from "../../api/gen/OptionDiff";
import type { PlanAction } from "../../api/gen/PlanAction";
import type { SectionDiff } from "../../api/gen/SectionDiff";
import type { SectionId } from "../../api/gen/SectionId";
import type { Value } from "../../api/gen/Value";
import { strings } from "../../lib/strings";

const t = strings.changes;

/** What the plan shows in place of a secret's value (`uniwrt_model::apply::MASKED`). */
export const MASKED = "••••••••";

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
  | "dhcp"
  | "firewall"
  | "system"
  | "other"
  | "service"
  | "address"
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
  const warnings = plan.nodes.reduce((sum, n) => sum + n.warnings.length, 0);
  if (warnings > 0)
    out.push({ kind: "warnings", text: t.impact.warnings(warnings), disruptive: false });
  return out;
}

export type SectionChange = "added" | "removed" | "changed" | "moved";

/** How a section changed: added, removed, its options changed, or only its place. */
export function sectionChange(diff: SectionDiff): SectionChange {
  if (!diff.before) return "added";
  if (!diff.after) return "removed";
  return diff.options.length > 0 ? "changed" : "moved";
}

/** A section as `uci` addresses it: by its name, else `@type[index]`. */
export function sectionLabel(id: SectionId): string {
  return id.name === undefined ? `@${id.type}[${id.index}]` : `'${id.name}'`;
}

/** The section it was, when it was renamed (or a named one replaced an anonymous one). */
export function renamedFrom(diff: SectionDiff): string | undefined {
  const { before, after } = diff;
  if (!before || !after || before.name === after.name) return undefined;
  return sectionLabel(before);
}

/** Sections added, removed and changed (moved counts as changed) in a package. */
export function diffCounts(diff: Diff): { added: number; removed: number; changed: number } {
  const out = { added: 0, removed: 0, changed: 0 };
  for (const section of diff.sections) {
    const change = sectionChange(section);
    out[change === "moved" ? "changed" : change] += 1;
  }
  return out;
}

/** One line of an option's change, as `uci` would write the option. */
export interface OptionLine {
  op: "+" | "-" | "~";
  keyword: "option" | "list";
  name: string;
  /** The value (the new one when changed). */
  value: string;
  /** The old value of a changed option. */
  before?: string;
}

const items = (value: Value): string[] => (Array.isArray(value) ? value : [value]);

/**
 * An option's change as lines: added and removed options whole, a changed option as old → new,
 * and a changed list item by item (a list that only reordered, as old → new).
 */
export function optionLines(option: OptionDiff): OptionLine[] {
  const { name, before, after } = option;
  const lines = (op: "+" | "-", value: Value): OptionLine[] =>
    Array.isArray(value)
      ? value.map((item) => ({ op, keyword: "list", name, value: item }))
      : [{ op, keyword: "option", name, value }];
  if (before === null) return after === null ? [] : lines("+", after);
  if (after === null) return lines("-", before);
  if (!Array.isArray(before) && !Array.isArray(after)) {
    return [{ op: "~", keyword: "option", name, value: after, before }];
  }
  if (Array.isArray(before) !== Array.isArray(after)) {
    return [...lines("-", before), ...lines("+", after)];
  }
  const old = items(before);
  const now = items(after);
  const removed = old.filter((item) => !now.includes(item));
  const added = now.filter((item) => !old.includes(item));
  if (removed.length === 0 && added.length === 0) {
    return [{ op: "~", keyword: "list", name, value: now.join(" "), before: old.join(" ") }];
  }
  return [
    ...removed.map((value) => ({ op: "-" as const, keyword: "list" as const, name, value })),
    ...added.map((value) => ({ op: "+" as const, keyword: "list" as const, name, value })),
  ];
}
