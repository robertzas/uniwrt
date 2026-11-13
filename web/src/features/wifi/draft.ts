import type { Band } from "../../api/gen/Band";
import type { Issue } from "../../api/gen/Issue";
import type { NodeScope } from "../../api/gen/NodeScope";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { Span } from "../../api/gen/Span";
import type { Weekday } from "../../api/gen/Weekday";
import type { Wlan } from "../../api/gen/Wlan";
import type { WlanSecurity } from "../../api/gen/WlanSecurity";
import { strings } from "../../lib/strings";

const e = strings.wifi.errors;

export const BANDS: readonly Band[] = ["2g", "5g", "6g"];
export const WEEKDAYS: readonly Weekday[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
export const SECURITIES: readonly WlanSecurity[] = [
  "wpa2_wpa3",
  "wpa3",
  "wpa2",
  "owe",
  "owe_transition",
  "open",
];
export type Preset = "main" | "guest" | "iot";

/** Whether the mode has a passphrase. */
export function needsPassphrase(security: WlanSecurity): boolean {
  return security === "wpa2" || security === "wpa2_wpa3" || security === "wpa3";
}

/** A span of the schedule as typed: `from` and `to` are `HH:MM`. */
export interface SpanDraft {
  days: Weekday[];
  from: string;
  to: string;
}

/** A WLAN as the editor holds it. */
export interface WlanDraft {
  ssid: string;
  enabled: boolean;
  network: string;
  security: WlanSecurity;
  /** A new passphrase; empty keeps the one the WLAN has. */
  passphrase: string;
  bands: Band[];
  scope: NodeScope["scope"];
  nodes: number[];
  hidden: boolean;
  isolation: boolean;
  maxClients: string;
  scheduled: boolean;
  spans: SpanDraft[];
  /** 802.11r: `auto` follows the security mode. */
  fast: FastRoaming;
  /** 802.11k and 802.11v. */
  hints: boolean;
  /** Takes part in the site's steering (D-090). */
  steer: boolean;
}

export type FastRoaming = "auto" | "on" | "off";

export type DraftErrors = Partial<
  Record<"ssid" | "network" | "passphrase" | "bands" | "nodes" | "maxClients" | "schedule", string>
>;

/** Minutes after midnight as `HH:MM`. */
export const clock = (minutes: number) =>
  `${String(Math.floor(minutes / 60)).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;

/** `HH:MM` as minutes after midnight. */
export function minutesOf(text: string): number | undefined {
  const match = /^([01]\d|2[0-3]):([0-5]\d)$/.exec(text.trim());
  return match ? Number(match[1]) * 60 + Number(match[2]) : undefined;
}

/** The span a new schedule starts with: every day, 07:00 to 22:00. */
export const newSpan = (): SpanDraft => ({ days: [...WEEKDAYS], from: "07:00", to: "22:00" });

export function draftOf(wlan: Wlan): WlanDraft {
  return {
    ssid: wlan.ssid,
    enabled: wlan.enabled,
    network: wlan.network,
    security: wlan.security,
    passphrase: "",
    bands: [...wlan.bands],
    scope: wlan.nodes.scope,
    nodes: wlan.nodes.scope === "all" ? [] : [...wlan.nodes.nodes],
    hidden: wlan.hidden,
    isolation: wlan.isolation,
    maxClients: wlan.max_clients === null ? "" : String(wlan.max_clients),
    scheduled: wlan.schedule !== null,
    spans: wlan.schedule
      ? wlan.schedule.spans.map((span) => ({
          days: [...span.days],
          from: clock(span.from),
          to: clock(span.to),
        }))
      : [newSpan()],
    fast: wlan.roaming.fast === null ? "auto" : wlan.roaming.fast ? "on" : "off",
    hints: wlan.roaming.hints,
    steer: wlan.roaming.steer,
  };
}

/** Whether a WLAN of `security` fast-roams with `fast`, as the controller decides (D-085): only
 * with a passphrase, and when left to it, on for WPA2 alone. */
export function fastRoams(security: WlanSecurity, fast: FastRoaming): boolean {
  if (!needsPassphrase(security)) return false;
  return fast === "auto" ? security === "wpa2" : fast === "on";
}

/** A new WLAN of a preset: the main one, a guest one (open but encrypted) or one for things. */
export function presetWlan(preset: Preset, config: SiteConfig): Wlan {
  const of = (purpose: string) => config.networks.find((n) => n.purpose === purpose)?.id;
  const main = of("default") ?? config.networks[0]?.id ?? "lan";
  const base: Wlan = {
    id: "",
    ssid: "",
    enabled: true,
    network: main,
    security: "wpa2_wpa3",
    passphrase: null,
    bands: ["2g", "5g"],
    nodes: { scope: "all" },
    hidden: false,
    isolation: false,
    max_clients: null,
    schedule: null,
    roaming: { fast: null, hints: true, steer: true },
  };
  if (preset === "guest") {
    return { ...base, network: of("guest") ?? main, security: "owe_transition", isolation: true };
  }
  if (preset === "iot") {
    return { ...base, network: of("iot") ?? main, security: "wpa2", bands: ["2g"] };
  }
  return base;
}

/** An id for a new WLAN from its SSID (`[a-z][a-z0-9_]{0,11}`), which no other WLAN has. */
export function idOf(ssid: string, taken: readonly string[]): string {
  const slug = ssid
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^[^a-z]+/, "")
    .replace(/_+$/, "");
  const stem = (slug || "wlan").slice(0, 12).replace(/_+$/, "");
  if (!taken.includes(stem)) return stem;
  for (let n = 2; ; n++) {
    const id = `${stem.slice(0, 12 - String(n).length)}${n}`;
    if (!taken.includes(id)) return id;
  }
}

/** The modes 6 GHz allows. */
const SIX_GHZ: readonly WlanSecurity[] = ["wpa3", "owe", "owe_transition"];

export const bytes = (text: string) => new TextEncoder().encode(text).length;

/** A passphrase the server takes: 8 to 63 printable ASCII characters, or a 64-digit hex key. */
export function passphraseOk(value: string): boolean {
  return /^[\x20-\x7e]{8,63}$/.test(value) || /^[0-9a-fA-F]{64}$/.test(value);
}

function spansOf(drafts: readonly SpanDraft[]): Span[] | string {
  if (drafts.length === 0) return e.noSpans;
  const spans: Span[] = [];
  for (const draft of drafts) {
    const from = minutesOf(draft.from);
    const to = minutesOf(draft.to);
    if (from === undefined || to === undefined) return e.time;
    if (draft.days.length === 0) return e.noDays;
    spans.push({ days: WEEKDAYS.filter((day) => draft.days.includes(day)), from, to });
  }
  return spans;
}

/**
 * The WLAN a draft describes, with the secrets its save carries, or what is wrong with it.
 * `current` is the WLAN being edited (with its id and passphrase reference), or a new one.
 */
export function wlanOf(
  draft: WlanDraft,
  current: Wlan,
  others: readonly Wlan[],
): { wlan?: Wlan; secrets?: Record<string, string | null>; errors: DraftErrors } {
  const errors: DraftErrors = {};
  const ssid = draft.ssid;
  const transition = draft.security === "owe_transition";
  if (ssid.length === 0 || bytes(ssid) > 32) errors.ssid = e.ssid;
  else if (transition && bytes(ssid) > 29) errors.ssid = e.ssidTransition;
  else if (others.some((other) => other.ssid === ssid)) errors.ssid = e.ssidTaken;
  const secret = needsPassphrase(draft.security);
  if (secret) {
    if (draft.passphrase === "" && current.passphrase === null) errors.passphrase = e.passphrase;
    else if (draft.passphrase !== "" && !passphraseOk(draft.passphrase)) {
      errors.passphrase = e.passphrase;
    }
  }
  if (draft.bands.length === 0) errors.bands = e.bands;
  else if (draft.bands.includes("6g") && !SIX_GHZ.includes(draft.security)) {
    errors.bands = e.sixGhz;
  }
  if (draft.scope !== "all" && draft.nodes.length === 0) errors.nodes = e.nodes;
  const max = draft.maxClients.trim();
  if (max !== "" && !(/^\d{1,4}$/.test(max) && Number(max) >= 1 && Number(max) <= 2007)) {
    errors.maxClients = e.maxClients;
  }
  const spans = draft.scheduled ? spansOf(draft.spans) : null;
  if (typeof spans === "string") errors.schedule = spans;
  if (Object.keys(errors).length > 0 || typeof spans === "string") return { errors };
  const id =
    current.id ||
    idOf(
      ssid,
      others.map((other) => other.id),
    );
  const nodes = [...draft.nodes].sort((a, b) => a - b);
  const wlan: Wlan = {
    id,
    ssid,
    enabled: draft.enabled,
    network: draft.network,
    security: draft.security,
    // A new value's reference is the server's to make.
    passphrase: secret ? current.passphrase : null,
    bands: BANDS.filter((band) => draft.bands.includes(band)),
    nodes: draft.scope === "all" ? { scope: "all" } : { scope: draft.scope, nodes },
    hidden: draft.hidden,
    isolation: draft.isolation,
    max_clients: max === "" ? null : Number(max),
    schedule: spans === null ? null : { spans },
    roaming: {
      fast: draft.fast === "auto" ? null : draft.fast === "on",
      hints: draft.hints,
      steer: draft.steer,
    },
  };
  const secrets =
    secret && draft.passphrase !== "" ? { [`wlans/${id}/passphrase`]: draft.passphrase } : {};
  return { wlan, ...(Object.keys(secrets).length > 0 && { secrets }), errors };
}

/** `config` with `wlan` in place of the one with its id, or added last; `remove` drops `id`. */
export function withWlan(config: SiteConfig, wlan: Wlan | { remove: string }): SiteConfig {
  if ("remove" in wlan) {
    return { ...config, wlans: config.wlans.filter((w) => w.id !== wlan.remove) };
  }
  const known = config.wlans.some((w) => w.id === wlan.id);
  return {
    ...config,
    wlans: known ? config.wlans.map((w) => (w.id === wlan.id ? wlan : w)) : [...config.wlans, wlan],
  };
}

const FIELDS: Record<string, keyof DraftErrors> = {
  ssid: "ssid",
  network: "network",
  passphrase: "passphrase",
  bands: "bands",
  security: "bands",
  nodes: "nodes",
  max_clients: "maxClients",
  schedule: "schedule",
};

/** The server's errors on the fields they concern; the rest as messages. */
export function errorsOfIssues(
  issues: readonly Issue[],
  id: string,
): { errors: DraftErrors; other: string[] } {
  const errors: DraftErrors = {};
  const other: string[] = [];
  for (const issue of issues) {
    const prefix = `wlans/${id}/`;
    const field = issue.path.startsWith(prefix)
      ? FIELDS[issue.path.slice(prefix.length).split("/")[0] ?? ""]
      : undefined;
    if (field && errors[field] === undefined) errors[field] = issue.message;
    else other.push(issue.message);
  }
  return { errors, other };
}

/** What a phone's camera joins the WLAN with (the `WIFI:` scheme). */
export function joinCode(wlan: Wlan, passphrase: string | undefined): string {
  const escaped = (text: string) => text.replace(/([\\;,":])/g, "\\$1");
  const type = !needsPassphrase(wlan.security)
    ? "nopass"
    : wlan.security === "wpa3"
      ? "SAE"
      : "WPA";
  return [
    `WIFI:T:${type}`,
    `S:${escaped(wlan.ssid)}`,
    ...(passphrase === undefined ? [] : [`P:${escaped(passphrase)}`]),
    ...(wlan.hidden ? ["H:true"] : []),
    ";",
  ].join(";");
}
