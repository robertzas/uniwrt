import type { Medium } from "../../api/gen/Medium";
import type { SeriesInfo } from "../../api/gen/SeriesInfo";
import { strings } from "../../lib/strings";

/** What an entity names (observability.md, Metrics: `node:<id>/dev:<name>` and so on). */
export type Entity =
  | { kind: "site" }
  | { kind: "node"; node: number }
  | { kind: "device"; node: number; name: string }
  | { kind: "radio"; node: number; name: string }
  | { kind: "link"; a: string; b: string; medium: string }
  | { kind: "client"; mac: string };

export type EntityKind = Entity["kind"];

/** Reads an entity's text; `undefined` for text that names none. */
export function parseEntity(text: string): Entity | undefined {
  if (text === "site") return { kind: "site" };
  const below = /^node:(\d+)(?:\/(dev|radio):(.+))?$/.exec(text);
  if (below) {
    const node = Number(below[1]);
    if (below[2] === undefined || below[3] === undefined) return { kind: "node", node };
    return { kind: below[2] === "dev" ? "device" : "radio", node, name: below[3] };
  }
  if (text.startsWith("link:")) {
    const [a, b, medium] = text.slice(5).split("~");
    if (a && b && medium) return { kind: "link", a, b, medium };
  }
  if (text.startsWith("client:")) return { kind: "client", mac: text.slice(7) };
  return undefined;
}

/** Names the explorer shows: nodes and clients as the user knows them. */
export interface Names {
  node(id: number): string | undefined;
  client(mac: string): string | undefined;
}

/** A vertex of a link (`node:2`, `client:…`, `peer:…`) by name. */
function vertexName(vertex: string, names: Names): string {
  const node = /^node:(\d+)$/.exec(vertex);
  if (node) return names.node(Number(node[1])) ?? strings.metrics.node(node[1] ?? "");
  if (vertex.startsWith("client:")) return names.client(vertex.slice(7)) ?? vertex.slice(7);
  return vertex.replace(/^peer:/, "");
}

export function entityLabel(entity: Entity, names: Names): string {
  const m = strings.metrics;
  const nodeName = (id: number) => names.node(id) ?? m.node(String(id));
  switch (entity.kind) {
    case "site":
      return m.site;
    case "node":
      return nodeName(entity.node);
    case "device":
    case "radio":
      return `${nodeName(entity.node)} · ${entity.name}`;
    case "link": {
      const medium = strings.topology.medium[entity.medium as Medium] ?? entity.medium;
      return `${m.link(vertexName(entity.a, names), vertexName(entity.b, names))} · ${medium}`;
    }
    case "client":
      return names.client(entity.mac) ?? entity.mac;
  }
}

/** Where an entity's own page is, if it has one. */
export function entityPage(entity: Entity): string | undefined {
  switch (entity.kind) {
    case "node":
    case "device":
    case "radio":
      return `/devices/${entity.node}`;
    case "client":
      return `/clients/${entity.mac}`;
    default:
      return undefined;
  }
}

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
