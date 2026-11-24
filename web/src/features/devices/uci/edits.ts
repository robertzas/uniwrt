import type { RawSection } from "../../../api/gen/RawSection";
import type { RawValue } from "../../../api/gen/RawValue";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import type { UciTreePackage } from "../../../api/gen/UciTreePackage";
import type { Value } from "../../../api/gen/Value";
import { strings } from "../../../lib/strings";
import { entryOf } from "../config/schedules";
import { holds, sameValue } from "./uci";

const e = strings.device.uci.errors;

/** An edit of a section as read: plain options set, masked ones typed anew, options deleted. */
export interface SectionDelta {
  set: Record<string, Value>;
  secrets: Record<string, string>;
  unset: string[];
}

/** The node's raw sections after an edit, with the values its save seals: `[index, option, value]`. */
export interface RawEdit {
  raw: RawSection[];
  secrets: [number, string, string][];
}

/** How a merge or removal finds a section: its name, its type's key options, or what it holds. */
type Selector =
  | { name: string }
  | { key: Record<string, string> }
  | { matching: Record<string, Value> };

/** The options raw sections set or delete on a section, which may differ when render writes it. */
interface Touched {
  options: Record<string, RawValue>;
  unset: Iterable<string>;
}

const NONE: Touched = { options: {}, unset: [] };

/**
 * How a merge or removal finds the package's section `index` as render finds it (`raw::target`):
 * by name; for an anonymous one by its type's key when the key's options find it first and
 * `touched` leaves them as read, else by the plain options it holds that `touched` leaves
 * alone (D-143), when they find it first; otherwise what is wrong.
 */
export function selector(
  pkg: UciTreePackage,
  index: number,
  touched: Touched = NONE,
): Selector | string {
  const section = pkg.sections[index];
  if (!section) return e.gone;
  if (section.name !== null) return { name: section.name };
  const unset = new Set(touched.unset);
  const key = section.key;
  if (key?.length === 0) {
    if (pkg.sections.findIndex((s) => s.type === section.type) === index) return { key: {} };
  } else if (key) {
    const values: Record<string, string> = {};
    for (const k of key) {
      const option = section.options.find((o) => o.name === k);
      const set = touched.options[k];
      if (!option || option.secret || typeof option.value !== "string" || unset.has(k)) break;
      if (set !== undefined && !sameValue(set, option.value)) break;
      values[k] = option.value;
    }
    const complete = Object.keys(values).length === key.length;
    const first = pkg.sections.findIndex(
      (s) =>
        s.type === section.type &&
        s.name === null &&
        key.every((k) => holds(s, k, values[k] ?? "")),
    );
    if (complete && first === index) return { key: values };
  }
  const matching: Record<string, Value> = {};
  for (const option of section.options) {
    if (option.secret || unset.has(option.name) || option.name in touched.options) continue;
    matching[option.name] = option.value;
  }
  const entries = Object.entries(matching);
  if (entries.length === 0) return e.noIdentity;
  const first = pkg.sections.findIndex(
    (s) => s.type === section.type && s.name === null && entries.every(([n, v]) => holds(s, n, v)),
  );
  return first === index ? { matching } : e.notFirst;
}

/** A raw section of the package's section `index` that finds it by `found`. */
function finding(pkg: UciTreePackage, index: number, found: Selector): RawSection {
  const section = pkg.sections[index];
  return {
    package: pkg.name,
    type: section?.type ?? "",
    name: "name" in found ? found.name : null,
    options: "key" in found ? { ...found.key } : {},
    merge: false,
    unset: [],
    remove: false,
    matching: "matching" in found ? found.matching : {},
  };
}

/** The secrets of `delta` as sealed into the raw section at `at` in `raw`. */
function sealed(raw: RawSection[], at: RawSection, delta: SectionDelta): RawEdit {
  const index = raw.indexOf(at);
  return {
    raw,
    secrets: Object.entries(delta.secrets).map(([option, value]) => [index, option, value]),
  };
}

/**
 * The node's raw sections with `delta` made to the package's section `index` as read. The last
 * raw section that finds it takes the edit: a whole section's options change, and a merge's
 * options and deletions; without one, a new merge does (D-143). An anonymous whole section of a
 * patched package is written on the node's file already, which would then hold it twice: a
 * removal of the section as read comes first (replacing the one an earlier edit put there).
 */
export function editSection(
  raw: readonly RawSection[],
  pkg: UciTreePackage,
  index: number,
  delta: SectionDelta,
): RawEdit | string {
  const section = pkg.sections[index];
  if (!section) return e.gone;
  const list = [...raw];
  const last = [...section.overrides].reverse().find((i) => raw[i] && !raw[i].remove);
  const held = last === undefined ? undefined : raw[last];
  if (last !== undefined && held && !held.merge) {
    const options = { ...held.options, ...delta.set };
    for (const name of [...delta.unset, ...Object.keys(delta.secrets)]) delete options[name];
    const whole: RawSection = { ...held, options };
    list[last] = whole;
    if (whole.name === null && pkg.owner !== "rendered") {
      const found = selector(pkg, index);
      if (typeof found === "string") return found;
      const removal: RawSection = { ...finding(pkg, index, found), remove: true };
      const before = list[last - 1];
      const earlier =
        before?.remove &&
        before.name === null &&
        before.package === whole.package &&
        before.type === whole.type;
      if (earlier) list[last - 1] = removal;
      else list.splice(last, 0, removal);
    }
    return sealed(list, whole, delta);
  }
  const options: Record<string, RawValue> = { ...held?.options };
  const unset = new Set(held?.unset);
  for (const [name, value] of Object.entries(delta.set)) {
    options[name] = value;
    unset.delete(name);
  }
  for (const name of Object.keys(delta.secrets)) {
    delete options[name];
    unset.delete(name);
  }
  for (const name of delta.unset) {
    delete options[name];
    unset.add(name);
  }
  const touched = { options: { ...options, ...delta.secrets }, unset };
  const found = selector(pkg, index, touched);
  if (typeof found === "string") return found;
  const base = finding(pkg, index, found);
  const merge: RawSection = {
    ...base,
    options: { ...options, ...base.options },
    merge: true,
    unset: [...unset].sort(),
  };
  if (last === undefined) list.push(merge);
  else list[last] = merge;
  return sealed(list, merge, delta);
}

/**
 * The node's raw sections with the package's section `index` deleted: those that find it go,
 * and a removal finds it, unless it was only an anonymous whole section of a rendered package
 * (render writes the package anew, without it). In a rendered package the options the dropped
 * merges set aren't render's, so they don't identify it.
 */
export function deleteSection(
  raw: readonly RawSection[],
  pkg: UciTreePackage,
  index: number,
): RawEdit | string {
  const section = pkg.sections[index];
  if (!section) return e.gone;
  const dropped = new Set(section.overrides);
  const kept = raw.filter((_, i) => !dropped.has(i));
  const ours = section.overrides.map((i) => raw[i]).filter((r) => r !== undefined);
  const rendered = pkg.owner === "rendered";
  if (rendered && ours.some((r) => !r.merge && !r.remove && r.name === null)) {
    return { raw: kept, secrets: [] };
  }
  const merges = ours.filter((r) => r.merge);
  const touched: Touched = rendered
    ? {
        options: Object.assign({}, ...merges.map((r) => r.options)),
        unset: merges.flatMap((r) => r.unset),
      }
    : NONE;
  const found = selector(pkg, index, touched);
  if (typeof found === "string") return found;
  return { raw: [...kept, { ...finding(pkg, index, found), remove: true }], secrets: [] };
}

/** The node's raw sections with a whole section added to `pkg` (its secrets sealed). */
export function addSection(
  raw: readonly RawSection[],
  pkg: string,
  type: string,
  name: string | null,
  delta: SectionDelta,
): RawEdit {
  const section: RawSection = {
    package: pkg,
    type,
    name,
    options: { ...delta.set },
    merge: false,
    unset: [],
    remove: false,
    matching: {},
  };
  return sealed([...raw, section], section, delta);
}

/** The node's raw sections without those at `indices`. */
export function dropRaw(raw: readonly RawSection[], indices: readonly number[]): RawEdit {
  const gone = new Set(indices);
  return { raw: raw.filter((_, i) => !gone.has(i)), secrets: [] };
}

/** The save's secrets by path (`nodes/<node>/raw/<i>/<option>`). */
export function secretPaths(node: number, edit: RawEdit): Record<string, string> | undefined {
  if (edit.secrets.length === 0) return undefined;
  return Object.fromEntries(
    edit.secrets.map(([i, option, value]) => [`nodes/${node}/raw/${i}/${option}`, value]),
  );
}

/**
 * `config` with the node's raw sections replaced by `raw`, edited from `base` (what the explorer
 * read them as); throws when they changed meanwhile, as the indices then mean other sections.
 */
export function withRaw(
  config: SiteConfig,
  node: number,
  base: readonly RawSection[],
  raw: RawSection[],
): SiteConfig {
  const entry = entryOf(config, node);
  if (JSON.stringify(entry.raw) !== JSON.stringify(base)) throw new Error(e.changed);
  const known = config.nodes.some((n) => n.node === node);
  const nodes = known ? config.nodes : [...config.nodes, entry];
  return { ...config, nodes: nodes.map((n) => (n.node === node ? { ...n, raw } : n)) };
}
