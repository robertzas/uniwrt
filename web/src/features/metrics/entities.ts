import type { SeriesInfo } from "../../api/gen/SeriesInfo";
import {
  type Entity,
  type EntityKind,
  entityLabel,
  type Names,
  parseEntity,
} from "../../lib/entities";
import { strings } from "../../lib/strings";

export {
  type Entity,
  type EntityKind,
  entityLabel,
  entityPage,
  type Names,
  parseEntity,
} from "../../lib/entities";

export interface EntityRow {
  text: string;
  entity: Entity;
  label: string;
  info: SeriesInfo;
  /** Indented under its node (interfaces and radios). */
  nested: boolean;
}

export interface EntityGroup {
  kind: EntityKind;
  rows: EntityRow[];
}

const ORDER: readonly ("site" | "node" | "link" | "client")[] = ["site", "node", "link", "client"];

/**
 * The listed series as the explorer's groups: the site, each node followed by its radios and
 * interfaces, the mesh links, then the clients, each by name; `filter` keeps the rows whose name,
 * entity or kind contains it.
 */
export function groupEntities(
  listing: readonly SeriesInfo[],
  names: Names,
  filter = "",
): EntityGroup[] {
  const needle = filter.trim().toLowerCase();
  const rows: EntityRow[] = [];
  for (const info of listing) {
    const entity = parseEntity(info.entity);
    if (!entity) continue;
    const label = entityLabel(entity, names);
    const kind = strings.metrics.kind[entity.kind];
    if (needle && ![label, info.entity, kind].some((t) => t.toLowerCase().includes(needle)))
      continue;
    rows.push({
      text: info.entity,
      entity,
      label,
      info,
      nested: entity.kind === "device" || entity.kind === "radio",
    });
  }
  const nodeOf = (row: EntityRow) => ("node" in row.entity ? row.entity.node : 0);
  const rank = (row: EntityRow) =>
    row.entity.kind === "node" ? 0 : row.entity.kind === "radio" ? 1 : 2;
  const groups: EntityGroup[] = [];
  for (const kind of ORDER) {
    const members = rows.filter((row) =>
      kind === "node" ? "node" in row.entity : row.entity.kind === kind,
    );
    if (kind === "node") {
      const nodeLabel = (id: number) => names.node(id) ?? strings.metrics.node(String(id));
      members.sort(
        (x, y) =>
          nodeLabel(nodeOf(x)).localeCompare(nodeLabel(nodeOf(y))) ||
          nodeOf(x) - nodeOf(y) ||
          rank(x) - rank(y) ||
          x.label.localeCompare(y.label),
      );
    } else {
      members.sort((x, y) => x.label.localeCompare(y.label));
    }
    if (members.length > 0) groups.push({ kind, rows: members });
  }
  return groups;
}
