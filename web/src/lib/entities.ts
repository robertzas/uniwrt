/**
 * Entities in words: the text of what a series or an alert is about (`node:3/radio:radio1`,
 * `link:node:2~node:3~wifi`, `client:<mac>`) parsed, named and linked to its page. Shared by the
 * Metrics explorer and the alerts.
 */
import type { Medium } from "../api/gen/Medium";
import { strings } from "./strings";

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
