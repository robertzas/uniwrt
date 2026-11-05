import type { ImportedSection } from "../../../api/gen/ImportedSection";
import type { ImportPreview } from "../../../api/gen/ImportPreview";
import type { Issue } from "../../../api/gen/Issue";
import type { NodeImportState } from "../../../api/gen/NodeImportState";
import type { SectionFate } from "../../../api/gen/SectionFate";

/** How many nodes ended in each state. */
export function stateCounts(preview: ImportPreview): Record<NodeImportState, number> {
  const out: Record<NodeImportState, number> = { imported: 0, failed: 0, unread: 0 };
  for (const node of preview.nodes) out[node.state] += 1;
  return out;
}

/** One message and how many times it was raised. */
export interface Repeated {
  message: string;
  count: number;
}

/** The preview's issues: the site-wide ones, and per node those at `nodes/<id>/…`. */
export interface SortedIssues {
  errors: Issue[];
  warnings: Issue[];
  /** A node's warnings, the same message once with its count (one per raw section, D-073). */
  byNode: Map<number, Repeated[]>;
}

const NODE_PATH = /^nodes\/(\d+)(\/|$)/;

/**
 * Sorts the imported document's issues: errors stay site-wide (they block the accept and must
 * be read whole); a node's warnings go to its card, each message once with its count, as an
 * import into nodes that manage nothing warns once per raw section.
 */
export function sortIssues(issues: readonly Issue[]): SortedIssues {
  const out: SortedIssues = { errors: [], warnings: [], byNode: new Map() };
  for (const issue of issues) {
    if (issue.severity === "error") {
      out.errors.push(issue);
      continue;
    }
    const id = NODE_PATH.exec(issue.path)?.[1];
    if (id === undefined) {
      out.warnings.push(issue);
      continue;
    }
    const node = Number(id);
    const list = out.byNode.get(node) ?? [];
    const same = list.find((seen) => seen.message === issue.message);
    if (same) same.count += 1;
    else list.push({ message: issue.message, count: 1 });
    out.byNode.set(node, list);
  }
  return out;
}

/** Sections by fate. */
export function fateCounts(sections: readonly ImportedSection[]): Record<SectionFate, number> {
  const out: Record<SectionFate, number> = { mapped: 0, raw: 0, dropped: 0 };
  for (const section of sections) out[section.fate] += 1;
  return out;
}

/** Sections grouped by package, in the order they come. */
export function byPackage(
  sections: readonly ImportedSection[],
): { package: string; sections: ImportedSection[] }[] {
  const out: { package: string; sections: ImportedSection[] }[] = [];
  for (const section of sections) {
    const group = out.find((g) => g.package === section.package);
    if (group) group.sections.push(section);
    else out.push({ package: section.package, sections: [section] });
  }
  return out;
}
