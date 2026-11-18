import type { Ddns } from "../../../api/gen/Ddns";
import type { DdnsFacts } from "../../../api/gen/DdnsFacts";
import type { DdnsProvider } from "../../../api/gen/DdnsProvider";
import type { DdnsSource } from "../../../api/gen/DdnsSource";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import { formatDateTime } from "../../../lib/format";
import { isDnsName } from "../../../lib/names";
import { strings } from "../../../lib/strings";
import type { Tone } from "../../../ui/Badge";

const t = strings.networks.ddns;
const e = t.errors;

export type ProviderKind = DdnsProvider["kind"];
export const PROVIDERS: readonly ProviderKind[] = [
  "cloudflare",
  "duckdns",
  "desec",
  "no_ip",
  "dynu",
  "freedns",
  "custom",
];

/** An entry as the editor holds it; `secret` is a newly typed token, empty keeps the stored one. */
export interface DdnsDraft {
  enabled: boolean;
  hostname: string;
  provider: ProviderKind;
  zone: string;
  username: string;
  url: string;
  secret: string;
  ipv6: boolean;
  source: DdnsSource;
}

export type DdnsErrors = Partial<
  Record<"hostname" | "zone" | "username" | "url" | "secret", string>
>;

export function ddnsDraft(ddns: Ddns | undefined): DdnsDraft {
  const p = ddns?.provider;
  return {
    enabled: ddns?.enabled ?? true,
    hostname: ddns?.hostname ?? "",
    provider: p?.kind ?? "cloudflare",
    zone: p?.kind === "cloudflare" ? p.zone : "",
    username:
      p?.kind === "no_ip" || p?.kind === "dynu"
        ? p.username
        : p?.kind === "custom"
          ? (p.username ?? "")
          : "",
    url: p?.kind === "custom" ? p.url : "",
    secret: "",
    ipv6: ddns?.ipv6 ?? false,
    source: ddns?.source ?? "wan",
  };
}

/** Whether the provider's update needs the token or password (`needs_secret` in the model). */
export function needsSecret(draft: Pick<DdnsDraft, "provider" | "url">): boolean {
  return draft.provider !== "custom" || draft.url.includes("[PASSWORD]");
}

/** A zone guessed from a hostname: its last two labels. */
export function zoneOf(hostname: string): string {
  return hostname.trim().toLowerCase().split(".").filter(Boolean).slice(-2).join(".");
}

const printable = (text: string) => /^[!-~]+$/.test(text);

/** An update URL ddns-scripts can call (`update_url_ok` in the model). */
function updateUrlOk(url: string): boolean {
  return /^https?:\/\/[^/]/.test(url) && url.length <= 512 && printable(url) && !/['"\\]/.test(url);
}

/** A new entry's id (`[a-z][a-z0-9_]{0,11}`) from its hostname's first label, not in `taken`. */
export function newId(hostname: string, taken: ReadonlySet<string>): string {
  const label = (hostname.trim().toLowerCase().split(".")[0] ?? "").replace(/[^a-z0-9_]/g, "_");
  const base = (/^[a-z]/.test(label) ? label : `d${label}`).slice(0, 12);
  if (!taken.has(base)) return base;
  for (let n = 2; ; n++) {
    const id = `${base.slice(0, 12 - String(n).length - 1)}_${n}`;
    if (!taken.has(id)) return id;
  }
}

function providerOf(draft: DdnsDraft): DdnsProvider {
  const username = draft.username.trim();
  switch (draft.provider) {
    case "cloudflare":
      return { kind: "cloudflare", zone: draft.zone.trim().toLowerCase() };
    case "no_ip":
    case "dynu":
      return { kind: draft.provider, username };
    case "custom":
      return { kind: "custom", url: draft.url.trim(), username: username === "" ? null : username };
    default:
      return { kind: draft.provider };
  }
}

/**
 * The entry a draft describes, with the secret its save carries, or what is wrong with it,
 * checked as the server checks it. `current` is the entry being edited; `others` the rest.
 */
export function ddnsOf(
  draft: DdnsDraft,
  current: Ddns | undefined,
  others: readonly Ddns[],
): { ddns?: Ddns; secrets?: Record<string, string>; errors: DdnsErrors } {
  const errors: DdnsErrors = {};
  const hostname = draft.hostname.trim().toLowerCase();
  const provider = providerOf(draft);
  const record = draft.ipv6 ? "AAAA" : "A";
  if (!isDnsName(hostname) || !hostname.includes(".")) errors.hostname = e.hostname;
  else if (provider.kind === "duckdns" && !/^[^.]+\.duckdns\.org$/.test(hostname)) {
    errors.hostname = e.duckdns;
  } else if (
    draft.enabled &&
    others.some((o) => o.enabled && o.ipv6 === draft.ipv6 && o.hostname.toLowerCase() === hostname)
  ) {
    errors.hostname = e.twice(record);
  }
  if (provider.kind === "cloudflare") {
    if (!isDnsName(provider.zone) || !provider.zone.includes(".")) errors.zone = e.zone;
    else if (
      !errors.hostname &&
      hostname !== provider.zone &&
      !hostname.endsWith(`.${provider.zone}`)
    ) {
      errors.zone = e.notInZone(hostname, provider.zone);
    }
  }
  const username = draft.username.trim();
  const required = provider.kind === "no_ip" || provider.kind === "dynu";
  if ((required || username !== "") && (username.length > 128 || !printable(username))) {
    errors.username = e.username;
  }
  if (provider.kind === "custom" && !updateUrlOk(provider.url)) errors.url = e.url;

  const secret = needsSecret(draft);
  // A stored token belongs to the provider it was typed for.
  const kept = current?.provider.kind === provider.kind ? (current.secret ?? null) : null;
  const label = strings.networks.ddns.editor.secret[draft.provider] ?? "";
  if (secret && draft.secret !== "") {
    if (draft.secret.length > 256 || !printable(draft.secret)) errors.secret = e.secret;
  } else if (secret && draft.enabled && kept === null) {
    errors.secret = current?.secret ? e.secretChanged(label) : e.secretNeeded(label);
  }
  if (Object.keys(errors).length > 0) return { errors };

  const id = current?.id ?? newId(hostname, new Set(others.map((o) => o.id)));
  const ddns: Ddns = {
    id,
    enabled: draft.enabled,
    hostname,
    provider,
    ipv6: draft.ipv6,
    source: draft.source,
    secret: secret ? kept : null,
  };
  const typed = secret && draft.secret !== "";
  return {
    ddns,
    ...(typed && { secrets: { [`services/ddns/${id}/secret`]: draft.secret } }),
    errors,
  };
}

/** The config with `edit` saved (in place, or appended) or an entry removed. */
export function withDdns(config: SiteConfig, edit: Ddns | { remove: string }): SiteConfig {
  const list = config.services.ddns;
  const ddns =
    "remove" in edit
      ? list.filter((d) => d.id !== edit.remove)
      : list.some((d) => d.id === edit.id)
        ? list.map((d) => (d.id === edit.id ? edit : d))
        : [...list, edit];
  return { ...config, services: { ...config.services, ddns } };
}

const FIELDS: Record<string, keyof DdnsErrors> = {
  hostname: "hostname",
  id: "hostname",
  secret: "secret",
  "provider/zone": "zone",
  "provider/username": "username",
  "provider/url": "url",
};

/** The server's issues on `services/ddns/<index>/…` as the editor's errors; the rest as text. */
export function ddnsErrorsOfIssues(
  issues: readonly { path: string; message: string }[],
  index: number,
): { errors: DdnsErrors; other: string[] } {
  const errors: DdnsErrors = {};
  const other: string[] = [];
  const prefix = `services/ddns/${index}/`;
  for (const issue of issues) {
    const field = issue.path.startsWith(prefix)
      ? FIELDS[issue.path.slice(prefix.length)]
      : undefined;
    if (field) errors[field] ??= issue.message;
    else other.push(issue.message);
  }
  return { errors, other };
}

/** An entry's state as the gateway reports it. */
export function ddnsStatus(
  ddns: Ddns,
  facts: readonly DdnsFacts[] | undefined,
): { tone: Tone; text: string; updated?: string } {
  if (!ddns.enabled) return { tone: "neutral", text: t.status.off };
  const found = facts?.find((f) => f.id === ddns.id);
  if (!found) return { tone: "neutral", text: t.status.unknown };
  const updated =
    found.updated_at === null ? undefined : t.updated(formatDateTime(found.updated_at));
  if (!found.running) return { tone: "warn", text: t.status.stopped, ...(updated && { updated }) };
  if (found.address === null) return { tone: "info", text: t.status.waiting };
  return { tone: "ok", text: t.status.registered(found.address), ...(updated && { updated }) };
}

/** The saved document's warnings on the entry at `index`. */
export function ddnsWarnings(
  issues: readonly { path: string; message: string }[],
  index: number,
): string[] {
  const prefix = `services/ddns/${index}/`;
  return issues.filter((i) => i.path.startsWith(prefix)).map((i) => i.message);
}
