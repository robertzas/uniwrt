import type { EventRecord } from "../../api/gen/EventRecord";
import type { Failover } from "../../api/gen/Failover";
import type { Grade } from "../../api/gen/Grade";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { Sqm } from "../../api/gen/Sqm";
import type { WanConfig } from "../../api/gen/WanConfig";
import { ApiError, api } from "../../lib/api";
import { strings } from "../../lib/strings";
import type { Tone } from "../../ui/Badge";

const t = strings.internet;

/** The link layers' overheads cake adds per packet, in bytes (wan.md, SQM). */
export const OVERHEADS = { none: null, fiber: 38, pppoe: 46, docsis: 18, vdsl: 34 } as const;
export type OverheadKey = keyof typeof OVERHEADS;

/** SQM as its form holds it: rates in Mbit/s as typed. */
export interface SqmDraft {
  on: boolean;
  down: string;
  up: string;
  overhead: OverheadKey;
  autotune: boolean;
}

/** kbit/s as Mbit/s for a field: `450`, `0.5`. */
export function mbps(kbps: number): string {
  return String(Math.round(kbps / 10) / 100);
}

/** Mbit/s as typed → kbit/s, or `undefined` outside 64 kbit/s … 10 Gbit/s. */
export function kbpsOf(text: string): number | undefined {
  const value = Number(text.trim().replace(",", "."));
  if (text.trim() === "" || !Number.isFinite(value)) return undefined;
  const kbps = Math.round(value * 1000);
  return kbps >= 64 && kbps <= 10_000_000 ? kbps : undefined;
}

export function sqmDraft(sqm: Sqm | null): SqmDraft {
  const overhead =
    (Object.entries(OVERHEADS).find(([, bytes]) => bytes === (sqm?.overhead ?? null))?.[0] as
      | OverheadKey
      | undefined) ?? "none";
  return {
    on: sqm !== null,
    down: sqm ? mbps(sqm.download_kbps) : "",
    up: sqm ? mbps(sqm.upload_kbps) : "",
    overhead,
    autotune: sqm?.autotune ?? true,
  };
}

/** The form's SQM, or which rates are wrong. */
export function sqmOf(
  draft: SqmDraft,
  current: Sqm | null,
): { sqm: Sqm | null; errors: { down?: string; up?: string } } {
  if (!draft.on) return { sqm: null, errors: {} };
  const down = kbpsOf(draft.down);
  const up = kbpsOf(draft.up);
  const errors = {
    ...(down === undefined && { down: t.sqm.rateError }),
    ...(up === undefined && { up: t.sqm.rateError }),
  };
  if (down === undefined || up === undefined) return { sqm: current, errors };
  // A custom overhead set elsewhere stays as it was while the form shows "none".
  const kept = current?.overhead ?? null;
  const known = Object.values(OVERHEADS).includes(kept as never);
  const overhead = draft.overhead === "none" && !known ? kept : OVERHEADS[draft.overhead];
  return {
    sqm: { download_kbps: down, upload_kbps: up, overhead, autotune: draft.autotune },
    errors: {},
  };
}

/** Failover as its form holds it. */
export interface FailoverDraft {
  on: boolean;
  link: "port" | "usb";
  port: string;
  vlan: string;
  device: string;
  proto: "dhcp" | "static";
  address: string;
  gateway: string;
  dns: string;
}

export type FailoverField = "port" | "vlan" | "device" | "address" | "gateway";

export function failoverDraft(failover: Failover | null): FailoverDraft {
  const link = failover?.link;
  const proto = failover?.proto;
  return {
    on: failover !== null,
    link: link?.kind ?? "port",
    port: link?.kind === "port" ? link.port : "",
    vlan: link?.kind === "port" && link.vlan !== null ? String(link.vlan) : "",
    device: link?.kind === "usb" ? link.device : "usb0",
    proto: proto?.kind ?? "dhcp",
    address: proto?.kind === "static" ? proto.address : "",
    gateway: proto?.kind === "static" ? proto.gateway : "",
    dns: proto?.kind === "static" ? proto.dns.join(", ") : "",
  };
}

const DEVICE = /^[A-Za-z0-9_.-]{1,15}$/;

/** The form's failover, or which fields are wrong (the server checks addresses fully). */
export function failoverOf(draft: FailoverDraft): {
  failover: Failover | null;
  errors: Partial<Record<FailoverField, string>>;
} {
  if (!draft.on) return { failover: null, errors: {} };
  const f = t.failover;
  const errors: Partial<Record<FailoverField, string>> = {};
  const vlan = draft.vlan.trim() === "" ? null : Number(draft.vlan);
  if (draft.link === "port") {
    if (!DEVICE.test(draft.port.trim())) errors.port = f.required;
    if (vlan !== null && !(Number.isInteger(vlan) && vlan >= 1 && vlan <= 4094)) {
      errors.vlan = strings.internet.errors.vlan;
    }
  } else if (!DEVICE.test(draft.device.trim())) {
    errors.device = f.required;
  }
  if (draft.proto === "static") {
    if (!draft.address.includes("/")) errors.address = f.required;
    if (draft.gateway.trim() === "") errors.gateway = f.required;
  }
  if (Object.keys(errors).length > 0) return { failover: null, errors };
  return {
    failover: {
      link:
        draft.link === "port"
          ? { kind: "port", port: draft.port.trim(), vlan }
          : { kind: "usb", device: draft.device.trim() },
      proto:
        draft.proto === "dhcp"
          ? { kind: "dhcp" }
          : {
              kind: "static",
              address: draft.address.trim(),
              gateway: draft.gateway.trim(),
              dns: draft.dns
                .split(",")
                .map((d) => d.trim())
                .filter((d) => d !== ""),
            },
    },
    errors: {},
  };
}

/**
 * `config` with SQM's and mwan3's packages managed on the gateway when they are on, where it
 * manages `network` (migrated sites already do; D-076).
 */
export function withManaged(config: SiteConfig, gateway: number | undefined): SiteConfig {
  const wanted = [
    ...(config.wan.sqm ? (["sqm"] as const) : []),
    ...(config.wan.failover ? (["mwan3"] as const) : []),
  ];
  if (gateway === undefined || wanted.length === 0) return config;
  return {
    ...config,
    nodes: config.nodes.map((node) =>
      node.node === gateway && node.managed.includes("network")
        ? {
            ...node,
            managed: [...node.managed, ...wanted.filter((p) => !node.managed.includes(p))],
          }
        : node,
    ),
  };
}

/** Saves the latest revision with its WAN changed by `edit`; the new revision's id. */
export async function saveWan(
  edit: (wan: WanConfig) => WanConfig,
  message: string,
  gateway: number | undefined,
): Promise<number> {
  const once = async () => {
    const latest = await api.site.get();
    const config = withManaged({ ...latest.config, wan: edit(latest.config.wan) }, gateway);
    return api.site.save({ base: latest.revision.id, message, config });
  };
  const view = await once().catch((err) => {
    if (err instanceof ApiError && err.status === 409) return once();
    throw err;
  });
  return view.revision.id;
}

export function gradeTone(grade: Grade): Tone {
  switch (grade) {
    case "A+":
    case "A":
      return "ok";
    case "B":
    case "C":
      return "warn";
    default:
      return "crit";
  }
}

/** A stretch of the timeline: the WAN down, or the traffic on the backup line. */
export interface Stretch {
  kind: "down" | "backup";
  start: number;
  end: number;
}

/**
 * The stretches `events` (any order) describe within `from` … `to`: the WAN without a default
 * route (`wan_down` → `wan_up`), and the primary line offline for mwan3 (its log patterns).
 * One still open ends at `to`.
 */
export function stretches(events: readonly EventRecord[], from: number, to: number): Stretch[] {
  const sorted = [...events].sort((a, b) => a.at - b.at);
  const out: Stretch[] = [];
  const open: Partial<Record<Stretch["kind"], number>> = {};
  const close = (kind: Stretch["kind"], end: number, start?: number) => {
    const begun = open[kind] ?? start;
    delete open[kind];
    if (begun !== undefined && end > from) {
      out.push({ kind, start: Math.max(begun, from), end: Math.min(end, to) });
    }
  };
  for (const event of sorted) {
    const what = event.what;
    if (what.kind === "wan_down") open.down ??= event.at;
    else if (what.kind === "wan_up") {
      close("down", event.at, what.down_ms === null ? undefined : event.at - what.down_ms);
    } else if (what.kind === "log_pattern" && what.pattern === "mwan3-offline") {
      if (what.text.includes("Interface wan ")) open.backup ??= event.at;
    } else if (what.kind === "log_pattern" && what.pattern === "mwan3-online") {
      if (what.text.includes("Interface wan ")) close("backup", event.at);
    }
  }
  for (const kind of Object.keys(open) as Stretch["kind"][]) close(kind, to);
  return out.sort((a, b) => a.start - b.start);
}
