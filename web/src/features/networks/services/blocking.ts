import type { Blocking } from "../../../api/gen/Blocking";
import type { BlocklistsView } from "../../../api/gen/BlocklistsView";
import type { CatalogList } from "../../../api/gen/CatalogList";
import type { ListStatus } from "../../../api/gen/ListStatus";
import { formatCount, formatDateTime } from "../../../lib/format";
import { isDnsName } from "../../../lib/names";
import { strings } from "../../../lib/strings";
import type { Tone } from "../../../ui/Badge";

const t = strings.networks.blocking;
const e = t.editor.errors;

/** The model's limits (`validate/blocking.rs`). */
export const MAX_CUSTOM = 16;
export const MAX_DOMAINS = 1_000;
export const MAX_BYPASS = 256;
/** The lists turning blocking on starts from (`DEFAULT_LISTS`). */
export const DEFAULT_LISTS: readonly string[] = ["hagezi_normal", "urlhaus"];

/** The settings as the editor holds them: allow and deny as typed, one domain per line. */
export interface BlockingDraft {
  enabled: boolean;
  lists: string[];
  custom: { name: string; url: string }[];
  allow: string;
  deny: string;
  bypass: string[];
  stats: boolean;
}

export interface BlockingErrors {
  custom: { name?: string; url?: string }[];
  customCount?: string;
  allow?: string;
  deny?: string;
  bypass?: string;
}

export const NO_ERRORS: BlockingErrors = { custom: [] };

export function blockingDraft(blocking: Blocking): BlockingDraft {
  return {
    enabled: blocking.enabled,
    lists: [...blocking.lists],
    custom: blocking.custom.map((list) => ({ ...list })),
    allow: blocking.allow.join("\n"),
    deny: blocking.deny.join("\n"),
    bypass: [...blocking.bypass],
    stats: blocking.stats,
  };
}

/**
 * The draft with blocking turned on or off; turning it on with nothing to block starts from the
 * default lists, as the model's default does.
 */
export function withEnabled(draft: BlockingDraft, enabled: boolean): BlockingDraft {
  const empty =
    draft.lists.length === 0 && draft.custom.length === 0 && parseDomains(draft.deny).length === 0;
  return { ...draft, enabled, ...(enabled && empty && { lists: [...DEFAULT_LISTS] }) };
}

/** The catalog's lists of `category`, in the catalog's order. */
export function listsOf(catalog: readonly CatalogList[], category: CatalogList["category"]) {
  return catalog.filter((list) => list.category === category);
}

/** About how many domains the chosen catalog lists hold together, duplicates included. */
export function estimate(catalog: readonly CatalogList[], ids: readonly string[]): number {
  return catalog.filter((list) => ids.includes(list.id)).reduce((sum, l) => sum + l.domains, 0);
}

/** The words of typed text (split on lines, spaces and commas), each lowercased; `*.` dropped. */
export function parseDomains(text: string): string[] {
  return text
    .split(/[\s,]+/)
    .map((word) => word.toLowerCase().replace(/^\*\./, "").replace(/\.$/, ""))
    .filter((word) => word !== "");
}

/** As the model's `is_list_url`: an http(s) URL with a host, of characters a URL carries plainly. */
export function isListUrl(url: string): boolean {
  const rest = url.startsWith("https://")
    ? url.slice(8)
    : url.startsWith("http://")
      ? url.slice(7)
      : undefined;
  if (rest === undefined) return false;
  const host = rest.split(/[/?#]/)[0] ?? "";
  return url.length <= 512 && host !== "" && /^[!-~]*$/.test(url) && !/['"\\`<>]/.test(url);
}

/** The domains of `text` once each, or the first error, checked as the model checks them. */
function domainsOf(text: string): { domains: string[]; error?: string } {
  const domains = [...new Set(parseDomains(text))];
  const bad = domains.find((domain) => !isDnsName(domain));
  if (bad !== undefined) return { domains, error: e.domain(bad) };
  if (domains.length > MAX_DOMAINS) return { domains, error: e.tooManyDomains };
  return { domains };
}

/** The settings a draft describes, or what is wrong with it, checked as the server checks them. */
export function blockingOf(draft: BlockingDraft): { blocking?: Blocking; errors: BlockingErrors } {
  const errors: BlockingErrors = { custom: [] };
  let failed = false;
  const urls = new Set<string>();
  const custom = draft.custom.map((list, i) => {
    const name = list.name.trim();
    const url = list.url.trim();
    const found: { name?: string; url?: string } = {};
    // biome-ignore lint/suspicious/noControlCharactersInRegex: the model refuses control characters.
    if (name === "" || [...name].length > 64 || /[\u0000-\u001f\u007f]/.test(name)) {
      found.name = e.name;
    }
    if (!isListUrl(url)) found.url = e.url;
    else if (urls.has(url)) found.url = e.twice;
    urls.add(url);
    errors.custom[i] = found;
    if (found.name || found.url) failed = true;
    return { name, url };
  });
  if (custom.length > MAX_CUSTOM) {
    errors.customCount = e.tooManyCustom;
    failed = true;
  }
  const allow = domainsOf(draft.allow);
  const deny = domainsOf(draft.deny);
  const both = deny.domains.find((domain) => allow.domains.includes(domain));
  errors.allow = allow.error;
  errors.deny = deny.error ?? (both === undefined ? undefined : e.allowed(both));
  if (draft.bypass.length > MAX_BYPASS) errors.bypass = e.tooManyBypass;
  if (failed || errors.allow || errors.deny || errors.bypass) return { errors };
  return {
    blocking: {
      enabled: draft.enabled,
      lists: [...new Set(draft.lists)],
      custom,
      allow: allow.domains,
      deny: deny.domains,
      bypass: [...new Set(draft.bypass)],
      stats: draft.stats,
    },
    errors,
  };
}

/** The server's errors on the settings' fields; the others are shown above the buttons. */
export function blockingErrorsOfIssues(issues: readonly { path: string; message: string }[]): {
  errors: BlockingErrors;
  other: string[];
} {
  const errors: BlockingErrors = { custom: [] };
  const other: string[] = [];
  const prefix = "services/blocking/";
  for (const issue of issues) {
    const [field, index, part] = issue.path.startsWith(prefix)
      ? issue.path.slice(prefix.length).split("/")
      : [];
    if (field === "custom" && index !== undefined && (part === "name" || part === "url")) {
      const at = Number(index);
      errors.custom[at] = {
        ...errors.custom[at],
        [part]: errors.custom[at]?.[part] ?? issue.message,
      };
    } else if (field === "custom" && index === undefined) {
      errors.customCount ??= issue.message;
    } else if (field === "allow" || field === "deny" || field === "bypass") {
      errors[field] ??= issue.message;
    } else {
      other.push(issue.message);
    }
  }
  return { errors, other };
}

/** Where the gateway's copy stands, in words. */
export function syncStatus(view: BlocklistsView): { tone: Tone; text: string; pulse?: boolean } {
  const sync = view.sync;
  switch (sync.state) {
    case "synced":
      return { tone: "ok", text: t.sync.synced(formatCount(sync.domains ?? 0)) };
    case "waiting":
    case "pushing":
      return { tone: "info", text: t.sync[sync.state], pulse: true };
    case "failed":
      return { tone: "crit", text: sync.error ? `${t.sync.failed}: ${sync.error}` : t.sync.failed };
    case "offline":
    case "busy":
    case "unmanaged":
      return { tone: "warn", text: t.sync[sync.state] };
    default:
      return { tone: "neutral", text: t.sync[sync.state] };
  }
}

/** A list's download state, in words. */
export function listState(list: ListStatus): { tone: Tone; text: string } {
  if (list.error !== null) {
    const failed = t.list.failed(list.error);
    return { tone: "warn", text: list.domains === null ? failed : `${failed}; ${t.list.kept}` };
  }
  if (list.domains === null) return { tone: "neutral", text: t.list.never };
  const domains = t.list.domains(formatCount(list.domains));
  const changed = list.fetched_at ?? list.checked_at;
  return {
    tone: "ok",
    text: changed === null ? domains : `${domains} · ${t.list.updated(formatDateTime(changed))}`,
  };
}

/** Whether the lists' state moves on its own soon: worth reading again often. */
export function isBusy(view: BlocklistsView | undefined): boolean {
  return (
    view !== undefined &&
    (view.refreshing || view.sync.state === "waiting" || view.sync.state === "pushing")
  );
}
