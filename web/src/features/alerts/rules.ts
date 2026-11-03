/**
 * Alert rules, pure: the editor's draft to and from a rule, checked as the server checks it
 * (`RuleSpec::validate`), the series each kind of entity has (`EntityPattern::measures`), and a
 * rule in words.
 */
import type { Comparison } from "../../api/gen/Comparison";
import type { EventCategory } from "../../api/gen/EventCategory";
import type { EventKind } from "../../api/gen/EventKind";
import type { EventLevel } from "../../api/gen/EventLevel";
import type { Metric } from "../../api/gen/Metric";
import type { RuleSpec } from "../../api/gen/RuleSpec";
import type { RuleView } from "../../api/gen/RuleView";
import type { Names } from "../../lib/alerts";
import { formatDuration } from "../../lib/format";
import { formatMetric, metricLabel } from "../../lib/metrics";
import { strings } from "../../lib/strings";

const t = strings.alerts;
const e = t.editor.errors;

export type KindName = EventKind["kind"];
export type TargetKind = "site" | "node" | "device" | "radio" | "link" | "client";

export const TARGETS: readonly TargetKind[] = ["node", "radio", "device", "link", "client", "site"];

/** The series each kind of entity has, as `EntityPattern::measures` says. */
export const MEASURES: Record<TargetKind, readonly Metric[]> = {
  site: ["down_bps", "up_bps", "clients", "nodes_online"],
  node: ["cpu", "memory", "load", "temperature", "stations"],
  device: ["rx_bps", "tx_bps"],
  radio: ["busy", "noise", "stations"],
  link: ["signal", "tq", "quality", "capacity"],
  client: ["signal", "snr", "tx_rate", "rx_rate", "down_bps", "up_bps", "score"],
};

/** The event kinds the editor offers, in groups. */
export const KIND_GROUPS: readonly { group: keyof typeof t.kindGroups; kinds: KindName[] }[] = [
  {
    group: "devices",
    kinds: [
      "node_offline",
      "node_online",
      "node_degraded",
      "node_recovered",
      "node_rebooted",
      "flash_low",
      "flash_ok",
      "node_adopted",
      "job_failed",
      "controller_started",
    ],
  },
  { group: "internet", kinds: ["wan_down", "wan_up", "wan_address_changed"] },
  {
    group: "mesh",
    kinds: ["mesh_link_down", "mesh_link_up", "mesh_link_weak", "mesh_link_recovered"],
  },
  { group: "clients", kinds: ["client_joined", "client_left", "client_roamed", "new_client"] },
  { group: "logs", kinds: ["log_pattern"] },
];

const MIN = 60;
const HOUR = 3600;
const DAY = 86_400;
/** How long a metric must stay beyond its threshold. */
export const FOR_CHOICES = [
  0,
  MIN,
  2 * MIN,
  5 * MIN,
  10 * MIN,
  15 * MIN,
  30 * MIN,
  HOUR,
  2 * HOUR,
  6 * HOUR,
  DAY,
];
/** The window events are counted in. */
export const WINDOW_CHOICES = [
  MIN,
  5 * MIN,
  10 * MIN,
  15 * MIN,
  30 * MIN,
  HOUR,
  2 * HOUR,
  6 * HOUR,
  12 * HOUR,
  DAY,
  7 * DAY,
];
/** How soon after resolving a firing reopens the alert. */
export const COOLDOWN_CHOICES = [
  0,
  MIN,
  5 * MIN,
  10 * MIN,
  15 * MIN,
  30 * MIN,
  HOUR,
  6 * HOUR,
  DAY,
];

/** `choices` with `value` among them, in order: a rule edited elsewhere keeps its value. */
export function withChoice(choices: readonly number[], value: number): number[] {
  return choices.includes(value) ? [...choices] : [...choices, value].sort((a, b) => a - b);
}

/** A metric rule's entities, as the editor holds them: a kind, a node or any, a name or any. */
export interface Target {
  kind: TargetKind;
  /** null: any node. */
  node: number | null;
  /** A device's or radio's name, a client's MAC or a link's id; "" for any. */
  part: string;
}

/** An entity pattern's text (such as `node:3/radio:radio1`, `*` for any) as a target; every node when it names none. */
export function parseTarget(text: string): Target {
  if (text === "site") return { kind: "site", node: null, part: "" };
  const below = /^node:(\*|\d+)(?:\/(dev|radio):(.+))?$/.exec(text);
  if (below) {
    const node = below[1] === "*" ? null : Number(below[1]);
    const part = below[3] === undefined || below[3] === "*" ? "" : below[3];
    if (below[2] === undefined) return { kind: "node", node, part: "" };
    return { kind: below[2] === "dev" ? "device" : "radio", node, part };
  }
  const other = /^(link|client):(.+)$/.exec(text);
  if (other?.[2] !== undefined) {
    return {
      kind: other[1] === "link" ? "link" : "client",
      node: null,
      part: other[2] === "*" ? "" : other[2],
    };
  }
  return { kind: "node", node: null, part: "" };
}

/** A target's pattern text. */
export function targetText(target: Target): string {
  const node = target.node === null ? "*" : String(target.node);
  const part = target.part.trim() === "" ? "*" : target.part.trim();
  switch (target.kind) {
    case "site":
      return "site";
    case "node":
      return `node:${node}`;
    case "device":
      return `node:${node}/dev:${part}`;
    case "radio":
      return `node:${node}/radio:${part}`;
    case "link":
      return `link:${part}`;
    case "client":
      return `client:${part.toLowerCase()}`;
  }
}

/** A target in words: `Every radio`, `radio1 of ap-garden`, `The network`. */
export function targetLabel(target: Target, names: Names): string {
  const w = t.target;
  const node =
    target.node === null
      ? undefined
      : (names.node(target.node) ?? strings.events.node(target.node));
  const part = target.part.trim();
  switch (target.kind) {
    case "site":
      return w.site;
    case "node":
      return node ?? w.node;
    case "device":
    case "radio": {
      const every = target.kind === "device" ? w.device : w.radio;
      if (node === undefined) return part === "" ? every : w.named(part);
      return w.ofNode(part === "" ? every : part, node);
    }
    case "link":
      return part === "" ? w.link : part;
    case "client":
      return part === "" ? w.client : (names.client(part) ?? part);
  }
}

/** What the editor edits: every field of both kinds, so switching kind keeps what was typed. */
export interface Draft {
  name: string;
  enabled: boolean;
  level: EventLevel;
  cooldown: number;
  type: "metric" | "event";
  target: Target;
  metric: Metric;
  op: Comparison;
  threshold: string;
  clear: string;
  forS: number;
  kinds: KindName[];
  categories: EventCategory[];
  min: EventLevel | null;
  nodes: number[];
  patterns: string[];
  count: string;
  windowS: number;
}

/** The draft of a rule, or of a new one (a device's CPU above 90 % for 5 minutes). */
export function draftOf(rule?: RuleView): Draft {
  const draft: Draft = {
    name: rule?.name ?? "",
    enabled: rule?.enabled ?? true,
    level: rule?.level ?? "warning",
    cooldown: rule?.cooldown_s ?? 15 * MIN,
    type: rule?.when.type ?? "metric",
    target: { kind: "node", node: null, part: "" },
    metric: "cpu",
    op: "above",
    threshold: "90",
    clear: "",
    forS: 5 * MIN,
    kinds: [],
    categories: [],
    min: null,
    nodes: [],
    patterns: [],
    count: "1",
    windowS: 15 * MIN,
  };
  const when = rule?.when;
  if (when?.type === "metric") {
    Object.assign(draft, {
      target: parseTarget(when.entities),
      metric: when.metric,
      op: when.op,
      threshold: String(when.threshold),
      clear: when.clear === null ? "" : String(when.clear),
      forS: when.for_s,
    });
  } else if (when?.type === "event") {
    Object.assign(draft, {
      kinds: when.kinds as KindName[],
      categories: when.categories,
      min: when.min,
      nodes: when.nodes,
      patterns: when.patterns,
      count: String(when.count),
      windowS: when.window_s,
    });
  }
  return draft;
}

export type DraftErrors = Partial<
  Record<"name" | "threshold" | "clear" | "part" | "count" | "nodes" | "patterns" | "kinds", string>
>;

const MAC = /^[0-9a-f]{2}(:[0-9a-f]{2}){5}$/i;

function number(text: string): number | undefined {
  if (text.trim() === "") return undefined;
  const value = Number(text);
  return Number.isFinite(value) ? value : undefined;
}

/** The spec to send, or what is wrong with the draft. */
export function checkDraft(draft: Draft): { spec: RuleSpec } | { errors: DraftErrors } {
  const errors: DraftErrors = {};
  const name = draft.name.trim();
  if (name === "" || [...name].length > 80 || /\p{Cc}/u.test(name)) errors.name = e.name;
  let when: RuleSpec["when"] | undefined;
  if (draft.type === "metric") {
    const threshold = number(draft.threshold);
    const clear = draft.clear.trim() === "" ? null : number(draft.clear);
    const part = draft.target.part.trim();
    if (threshold === undefined) errors.threshold = e.number;
    if (clear === undefined) errors.clear = e.number;
    else if (clear !== null && threshold !== undefined) {
      if (draft.op === "above" && clear > threshold) errors.clear = e.clearAbove;
      if (draft.op === "below" && clear < threshold) errors.clear = e.clearBelow;
    }
    if (draft.target.kind === "client" && part !== "" && !MAC.test(part)) errors.part = e.mac;
    else if (part.length > 64 || part.includes("/")) errors.part = e.part;
    if (threshold !== undefined && clear !== undefined) {
      when = {
        type: "metric",
        metric: MEASURES[draft.target.kind].includes(draft.metric)
          ? draft.metric
          : (MEASURES[draft.target.kind][0] ?? "cpu"),
        entities: targetText(draft.target),
        op: draft.op,
        threshold,
        clear,
        for_s: draft.forS,
      };
    }
  } else {
    const count = number(draft.count);
    if (count === undefined || !Number.isInteger(count) || count < 1 || count > 10_000) {
      errors.count = e.count;
    }
    if (draft.nodes.length > 32) errors.nodes = e.tooMany;
    if (draft.patterns.length > 32) errors.patterns = e.tooMany;
    if (draft.kinds.length > 32) errors.kinds = e.tooMany;
    if (count !== undefined) {
      when = {
        type: "event",
        kinds: draft.kinds,
        categories: draft.categories,
        min: draft.min,
        nodes: draft.nodes,
        patterns:
          draft.kinds.length === 0 || draft.kinds.includes("log_pattern") ? draft.patterns : [],
        count,
        window_s: draft.windowS,
      };
    }
  }
  if (Object.keys(errors).length > 0 || when === undefined) return { errors };
  return {
    spec: { name, enabled: draft.enabled, level: draft.level, when, cooldown_s: draft.cooldown },
  };
}

/** A rule in words: `CPU above 90 % for 5m · Every device`, `Device offline, 3 times within 1h`. */
export function ruleSummary(
  rule: Pick<RuleView, "when">,
  names: Names,
  pattern: (id: string) => string | undefined,
): string {
  const r = t.rules;
  const when = rule.when;
  if (when.type === "metric") {
    const value = formatMetric(when.metric, when.threshold);
    const head = `${metricLabel(when.metric)} ${t.cause[when.op]} ${value}`;
    const lasting = when.for_s > 0 ? ` ${r.forDuration(formatDuration(when.for_s))}` : "";
    return `${head}${lasting} · ${targetLabel(parseTarget(when.entities), names)}`;
  }
  const what =
    when.kinds.length > 0
      ? when.kinds.map((kind) => t.kinds[kind as KindName] ?? kind).join(", ")
      : when.categories.length > 0
        ? when.categories.map((c) => strings.events.category[c]).join(", ")
        : r.anyEvent;
  const parts = [what];
  // A pattern's meaning is a sentence: one is named, several are counted.
  const [only] = when.patterns;
  if (when.patterns.length > 1) parts.push(r.patterns(when.patterns.length));
  else if (only !== undefined) parts.push((pattern(only) ?? only).replace(/\.$/, ""));
  if (when.min !== null) parts.push(r.atLeast(strings.events.levelName[when.min]));
  if (when.count > 1) parts.push(r.times(when.count, formatDuration(when.window_s)));
  const text = parts.join(" · ");
  if (when.nodes.length === 0) return text;
  const nodes = when.nodes.map((id) => names.node(id) ?? strings.events.node(id)).join(", ");
  return `${text} · ${r.on_(nodes)}`;
}
