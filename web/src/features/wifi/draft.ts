import type { Band } from "../../api/gen/Band";
import type { Issue } from "../../api/gen/Issue";
import type { NodeScope } from "../../api/gen/NodeScope";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { Wlan } from "../../api/gen/Wlan";
import type { WlanSecurity } from "../../api/gen/WlanSecurity";
import { type SpanDraft, spanDrafts, spansOf } from "../../lib/schedule";
import { strings } from "../../lib/strings";

const e = strings.wifi.errors;

export const BANDS: readonly Band[] = ["2g", "5g", "6g"];
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

/** A band's own security modes, overriding the WLAN's (D-157). */
export type BandSecurity = Partial<Record<Band, WlanSecurity>>;

/** The modes `bands` run, each once: a band's own, else `security`; `security` alone without bands. */
export function modesOf(
  security: WlanSecurity,
  overrides: BandSecurity,
  bands: readonly Band[],
): WlanSecurity[] {
  const modes = bands.length === 0 ? [security] : bands.map((band) => overrides[band] ?? security);
  return [...new Set(modes)];
}

/** The modes a WLAN's bands run. */
export const wlanModes = (wlan: Wlan) => modesOf(wlan.security, wlan.band_security, wlan.bands);

/** The modes a draft's bands run. */
export const draftModes = (draft: Pick<WlanDraft, "security" | "bandSecurity" | "bands">) =>
  modesOf(draft.security, draft.bandSecurity, draft.bands);

/** A WLAN as the editor holds it. */
export interface WlanDraft {
  ssid: string;
  enabled: boolean;
  network: string;
  security: WlanSecurity;
  /** Bands that run a mode of their own; the bands left out run `security`. */
  bandSecurity: BandSecurity;
  /** A new passphrase; empty keeps the one the WLAN has. */
  passphrase: string;
  bands: Band[];
  scope: NodeScope["scope"];
  nodes: number[];
  hidden: boolean;
  isolation: boolean;
  /** Group traffic as unicast frames to each client that joined it (D-136). */
  multicastToUnicast: boolean;
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

export function draftOf(wlan: Wlan): WlanDraft {
  return {
    ssid: wlan.ssid,
    enabled: wlan.enabled,
    network: wlan.network,
    security: wlan.security,
    bandSecurity: { ...wlan.band_security },
    passphrase: "",
    bands: [...wlan.bands],
    scope: wlan.nodes.scope,
    nodes: wlan.nodes.scope === "all" ? [] : [...wlan.nodes.nodes],
    hidden: wlan.hidden,
    isolation: wlan.isolation,
    multicastToUnicast: wlan.multicast_to_unicast,
    maxClients: wlan.max_clients === null ? "" : String(wlan.max_clients),
    scheduled: wlan.schedule !== null,
    spans: spanDrafts(wlan.schedule),
    fast: wlan.roaming.fast === null ? "auto" : wlan.roaming.fast ? "on" : "off",
    hints: wlan.roaming.hints,
    steer: wlan.roaming.steer,
  };
}

/** Whether a WLAN whose bands run `modes` fast-roams with `fast`, as the controller decides
 * (D-085): only with a passphrase, and when left to it, while every band runs WPA2 alone. */
export function fastRoams(modes: readonly WlanSecurity[], fast: FastRoaming): boolean {
  if (!modes.some(needsPassphrase)) return false;
  return fast === "auto" ? modes.every((mode) => mode === "wpa2") : fast === "on";
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
    band_security: {},
    passphrase: null,
    bands: ["2g", "5g"],
    nodes: { scope: "all" },
    hidden: false,
    isolation: false,
    max_clients: null,
    schedule: null,
    roaming: { fast: null, hints: true, steer: true },
    multicast_to_unicast: true,
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
export const SIX_GHZ: readonly WlanSecurity[] = ["wpa3", "owe", "owe_transition"];

export const bytes = (text: string) => new TextEncoder().encode(text).length;

/** A passphrase the server takes: 8 to 63 printable ASCII characters, or a 64-digit hex key. */
export function passphraseOk(value: string): boolean {
  return /^[\x20-\x7e]{8,63}$/.test(value) || /^[0-9a-fA-F]{64}$/.test(value);
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
  const modes = draftModes(draft);
  const transition = modes.includes("owe_transition");
  if (ssid.length === 0 || bytes(ssid) > 32) errors.ssid = e.ssid;
  else if (transition && bytes(ssid) > 29) errors.ssid = e.ssidTransition;
  else if (others.some((other) => other.ssid === ssid)) errors.ssid = e.ssidTaken;
  const secret = modes.some(needsPassphrase);
  if (secret) {
    if (draft.passphrase === "" && current.passphrase === null) errors.passphrase = e.passphrase;
    else if (draft.passphrase !== "" && !passphraseOk(draft.passphrase)) {
      errors.passphrase = e.passphrase;
    }
  }
  if (draft.bands.length === 0) errors.bands = e.bands;
  else if (
    draft.bands.includes("6g") &&
    !SIX_GHZ.includes(draft.bandSecurity["6g"] ?? draft.security)
  ) {
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
    // Only the chosen bands' own modes that differ from the WLAN's.
    band_security: Object.fromEntries(
      BANDS.flatMap((band) => {
        const mode = draft.bandSecurity[band];
        return draft.bands.includes(band) && mode && mode !== draft.security ? [[band, mode]] : [];
      }),
    ),
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
    multicast_to_unicast: draft.multicastToUnicast,
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
  band_security: "bands",
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
  // The WLAN's own mode, else a band's that has a passphrase (D-157).
  const type = needsPassphrase(wlan.security)
    ? wlan.security === "wpa3"
      ? "SAE"
      : "WPA"
    : wlanModes(wlan).some(needsPassphrase)
      ? "WPA"
      : "nopass";
  return [
    `WIFI:T:${type}`,
    `S:${escaped(wlan.ssid)}`,
    ...(passphrase === undefined ? [] : [`P:${escaped(passphrase)}`]),
    ...(wlan.hidden ? ["H:true"] : []),
    ";",
  ].join(";");
}
