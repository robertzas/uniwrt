/**
 * A revision's changes in words (features/lifecycle.md, Desired state & revisions): paths as
 * `networks › iot › vlan` with devices named, values as text, and secrets never shown. The
 * document holds only vault references (`config/<32 hex>`) for secrets; the page shows that one
 * changed, not which reference it now holds.
 */
import type { Change } from "../../../api/gen/Change";
import type { ChangeOp } from "../../../api/gen/ChangeOp";
import type { RevisionMeta } from "../../../api/gen/RevisionMeta";
import { strings } from "../../../lib/strings";

const t = strings.settings.revisions;

/** A configuration secret's vault reference, as `SecretRef` checks it. */
const SECRET = /^config\/[0-9a-f]{32}$/;

/** Whether `value` is a vault reference to a configuration secret. */
export function isSecret(value: unknown): boolean {
  return typeof value === "string" && SECRET.test(value);
}

/** One leaf of an added or removed item: its path below the change's, and its value. */
export interface Field {
  path: string[];
  value: Shown;
}

/** A value as shown: text, a secret (never its reference), or an item's fields. */
export type Shown =
  | { kind: "text"; text: string }
  | { kind: "secret" }
  | { kind: "fields"; fields: Field[] };

/** One change, ready to show. */
export interface Entry {
  /** The path in words: field names and list items by key, devices by name. */
  path: string[];
  op: ChangeOp;
  /** `null` when added; `before` and `after` are both `null` for a secret that changed. */
  before: Shown | null;
  after: Shown | null;
  /** A secret was replaced by another: say so, show neither. */
  secretChanged: boolean;
}

/** The fields that key list items, in the order the server tries them (`site::diff`). */
const KEYS = ["id", "node", "radio", "mac"] as const;

type Plain = null | boolean | number | string | Plain[] | { [key: string]: Plain };

function isObject(value: unknown): value is Record<string, Plain> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** A scalar in words: `none`, `on`/`off`, numbers and strings as they are. */
function scalar(value: unknown): string {
  if (value === null || value === undefined) return t.none;
  if (typeof value === "boolean") return value ? t.on : t.off;
  return String(value);
}

/** An item's key, if it has one of the server's key fields. */
function keyOf(item: unknown): string | undefined {
  if (!isObject(item)) return undefined;
  for (const key of KEYS) {
    const value = item[key];
    if (typeof value === "string" || typeof value === "number") return String(value);
  }
  return undefined;
}

/** Fields that say which item it is, shown first; the server sends the rest by name. */
const FIRST = [...KEYS, "name", "ssid"];

/** An item's field names with its key and name first, the rest as they came. */
function ordered(keys: string[]): string[] {
  const rank = (key: string) => {
    const i = FIRST.indexOf(key);
    return i < 0 ? FIRST.length : i;
  };
  return keys
    .map((key, i) => [key, i] as const)
    .sort((a, b) => rank(a[0]) - rank(b[0]) || a[1] - b[1])
    .map(([key]) => key);
}

/** Every leaf below `value`, by path; list items by key, else by position from 1. */
function leaves(value: unknown, path: string[], out: Field[]) {
  if (isSecret(value)) {
    out.push({ path, value: { kind: "secret" } });
  } else if (Array.isArray(value)) {
    if (value.every((item) => !isObject(item) && !Array.isArray(item))) {
      out.push({ path, value: { kind: "text", text: list(value) } });
    } else {
      value.forEach((item, i) => {
        leaves(item, [...path, keyOf(item) ?? String(i + 1)], out);
      });
    }
  } else if (isObject(value)) {
    const keys = ordered(Object.keys(value));
    if (keys.length === 0) out.push({ path, value: { kind: "text", text: t.none } });
    for (const key of keys) leaves(value[key], [...path, key], out);
  } else {
    out.push({ path, value: { kind: "text", text: scalar(value) } });
  }
}

/** A list of scalars: `a, b, c`, `none` when empty; secrets in it are never shown. */
function list(items: unknown[]): string {
  if (items.length === 0) return t.none;
  return items.map((item) => (isSecret(item) ? t.secret : scalar(item))).join(", ");
}

/** A value as shown; `null` when there is none (added or removed). */
export function shown(value: unknown): Shown | null {
  if (value === null || value === undefined) return null;
  if (isSecret(value)) return { kind: "secret" };
  if (Array.isArray(value) || isObject(value)) {
    const fields: Field[] = [];
    leaves(value, [], fields);
    // A list of scalars, or an object of one leaf at the top, reads best as text.
    const only = fields[0];
    if (fields.length === 1 && only && only.path.length === 0) return only.value;
    return { kind: "fields", fields };
  }
  return { kind: "text", text: scalar(value) };
}

/** Names a path's segments: a device under `nodes` by its name, when it is known. */
export function pathWords(path: readonly string[], nodeName: (id: number) => string | undefined) {
  return path.map((segment, i) => {
    if (i !== 1 || path[0] !== "nodes") return segment;
    const id = Number(segment);
    return (Number.isInteger(id) && nodeName(id)) || t.node(segment);
  });
}

/**
 * A change ready to show. A changed value that was `null` reads as `none` (a field that was unset);
 * one whose `before` and `after` are both secrets says only that the secret changed.
 */
export function entry(change: Change, nodeName: (id: number) => string | undefined): Entry {
  const path = pathWords(change.path, nodeName);
  const secretChanged =
    change.op === "changed" && isSecret(change.before) && isSecret(change.after);
  const side = (value: unknown, present: boolean): Shown | null =>
    present ? (shown(value) ?? { kind: "text", text: t.none }) : null;
  return {
    path,
    op: change.op,
    before: secretChanged ? null : side(change.before, change.op !== "added"),
    after: secretChanged ? null : side(change.after, change.op !== "removed"),
    secretChanged,
  };
}

/** How many changes are additions, removals and edits. */
export function counts(changes: readonly Change[]): Record<ChangeOp, number> {
  const out: Record<ChangeOp, number> = { added: 0, removed: 0, changed: 0 };
  for (const change of changes) out[change.op] += 1;
  return out;
}

/** A revision's title: its message, or what made it when it has none. */
export function title(revision: RevisionMeta): string {
  if (revision.message.trim() !== "") return revision.message;
  if (revision.kind === "defaults") return t.defaultsTitle;
  if (revision.kind === "restore" && revision.restored_from !== null)
    return t.restoredTitle(revision.restored_from);
  return t.untitled;
}

/** Pages merged newest first by id: `next` replaces what it repeats, then the rest follow. */
export function merge(known: readonly RevisionMeta[], next: readonly RevisionMeta[]) {
  const ids = new Set(next.map((r) => r.id));
  return [...next, ...known.filter((r) => !ids.has(r.id))].sort((a, b) => b.id - a.id);
}
