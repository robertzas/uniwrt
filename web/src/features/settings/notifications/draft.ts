/**
 * The notifier editor's draft: from a stored notifier, and to what the API takes, checked as the
 * server checks it (`uniwrt_notify::validate`) so that its refusals are rare.
 */
import type { EventLevel } from "../../../api/gen/EventLevel";
import type { HeaderInput } from "../../../api/gen/HeaderInput";
import type { NodeId } from "../../../api/gen/NodeId";
import type { NotifierFilter } from "../../../api/gen/NotifierFilter";
import type { NotifierInput } from "../../../api/gen/NotifierInput";
import type { NotifierSpec } from "../../../api/gen/NotifierSpec";
import type { NotifierView } from "../../../api/gen/NotifierView";
import type { Placeholder } from "../../../api/gen/Placeholder";
import type { SecretsInput } from "../../../api/gen/SecretsInput";
import type { SmtpSecurity } from "../../../api/gen/SmtpSecurity";
import type { WebhookFormat } from "../../../api/gen/WebhookFormat";
import { strings } from "../../../lib/strings";
import { checkTemplate, STARTER } from "./template";

const t = strings.settings.notifications.check;

export type Kind = NotifierSpec["kind"];
export const KINDS: readonly Kind[] = ["ntfy", "webhook", "email"];
/** The presets first, uniwrt's JSON (which Home Assistant reads too) and a custom template last. */
export const FORMATS: readonly WebhookFormat[] = [
  "discord",
  "slack",
  "teams",
  "home_assistant",
  "generic",
  "custom",
];
/** Reminder intervals offered, in seconds; 0 is off (D-063). */
export const REMIND_CHOICES: readonly number[] = [0, 3600, 14_400, 43_200, 86_400];
export const DEFAULT_REMIND_S = 14_400;
export const MAX_NOTIFIERS = 20;
export const MAX_NAME = 60;
export const MAX_NODES = 32;
export const MAX_HEADERS = 10;
export const MAX_RECIPIENTS = 10;
const MAX_FIELD = 256;
const MAX_URL = 2048;
/** Headers the request sets itself. */
const RESERVED = [
  "host",
  "content-length",
  "content-type",
  "transfer-encoding",
  "connection",
  "user-agent",
];
const SMTP_PORTS: Record<SmtpSecurity, number> = { starttls: 587, tls: 465 };

/** A secret in the editor: as stored (only when one is), replaced by what was typed, or removed. */
export type Secret = { mode: "keep" } | { mode: "set"; value: string } | { mode: "remove" };

/** A webhook header: a stored one keeps its value (`null`) unless replaced, and its name. */
export interface HeaderDraft {
  key: number;
  name: string;
  stored: boolean;
  value: string | null;
}

export interface Draft {
  kind: Kind;
  name: string;
  enabled: boolean;
  min: EventLevel;
  someNodes: boolean;
  nodes: NodeId[];
  resolved: boolean;
  /** Seconds; 0 is off. */
  remind: number;
  server: string;
  topic: string;
  ntfyAuth: "none" | "token" | "password";
  token: Secret;
  /** ntfy's or the SMTP server's. */
  username: string;
  password: Secret;
  format: WebhookFormat;
  url: Secret;
  headers: HeaderDraft[];
  template: string;
  host: string;
  port: string;
  security: SmtpSecurity;
  emailAuth: boolean;
  from: string;
  /** Recipients separated by commas, spaces or lines. */
  to: string;
}

export type DraftErrors = Partial<Record<keyof Draft, string>>;

const fresh = (): Secret => ({ mode: "set", value: "" });
const stored = (set: boolean): Secret => (set ? { mode: "keep" } : fresh());
let headerKeys = 0;

/** A new header row. */
export function newHeader(): HeaderDraft {
  headerKeys += 1;
  return { key: headerKeys, name: "", stored: false, value: "" };
}

/** A topic hard to guess, since anyone who knows a topic on a public server can read it. */
export function randomTopic(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(8));
  const alphabet = "abcdefghijkmnpqrstuvwxyz23456789";
  return `uniwrt-${Array.from(bytes, (b) => alphabet[b % alphabet.length]).join("")}`;
}

/** The draft of a stored notifier, or a new one of `kind`. */
export function draftOf(view: NotifierView | undefined, kind: Kind = "ntfy"): Draft {
  const draft: Draft = {
    kind: view?.spec.kind ?? kind,
    name: view?.name ?? "",
    enabled: view?.enabled ?? true,
    min: view?.filter.min ?? "warning",
    someNodes: (view?.filter.nodes.length ?? 0) > 0,
    nodes: [...(view?.filter.nodes ?? [])],
    resolved: view?.filter.resolved ?? true,
    remind: view === undefined ? DEFAULT_REMIND_S : (view.filter.remind_s ?? 0),
    server: "https://ntfy.sh",
    topic: randomTopic(),
    ntfyAuth: "none",
    token: stored(view?.secrets.token ?? false),
    username: "",
    password: stored(view?.secrets.password ?? false),
    format: "discord",
    url: stored(view?.secrets.url != null),
    headers: (view?.secrets.headers ?? []).map((name) => ({
      ...newHeader(),
      name,
      stored: true,
      value: null,
    })),
    template: "",
    host: "",
    port: String(SMTP_PORTS.starttls),
    security: "starttls",
    emailAuth: true,
    from: "",
    to: "",
  };
  const spec = view?.spec;
  if (spec?.kind === "ntfy") {
    draft.server = spec.server;
    draft.topic = spec.topic;
    draft.username = spec.username ?? "";
    draft.ntfyAuth = spec.username !== null ? "password" : view?.secrets.token ? "token" : "none";
  } else if (spec?.kind === "webhook") {
    draft.format = spec.format;
    draft.template = spec.template ?? "";
  } else if (spec?.kind === "email") {
    draft.host = spec.host;
    draft.port = String(spec.port);
    draft.security = spec.security;
    draft.username = spec.username ?? "";
    draft.emailAuth = spec.username !== null;
    draft.from = spec.from;
    draft.to = spec.to.join(", ");
  }
  return draft;
}

/** The changes that go with another SMTP security: its usual port, unless one was chosen. */
export function withSecurity(draft: Draft, security: SmtpSecurity): Partial<Draft> {
  const usual = Object.values(SMTP_PORTS).map(String);
  return usual.includes(draft.port.trim())
    ? { security, port: String(SMTP_PORTS[security]) }
    : { security };
}

/** The changes that go with another webhook format: a custom one starts from an example. */
export function withFormat(draft: Draft, format: WebhookFormat): Partial<Draft> {
  return format === "custom" && draft.template.trim() === ""
    ? { format, template: STARTER }
    : { format };
}

/** The recipients written in `to`, once each. */
export function recipients(to: string): string[] {
  const seen = new Set<string>();
  return to
    .split(/[\s,;]+/)
    .filter((address) => address !== "")
    .filter((address) => {
      const key = address.toLowerCase();
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
}

/** Why `text` is not an http(s) URL with a host. */
export function urlProblem(text: string): string | undefined {
  if (text.length > MAX_URL || /\s/.test(text)) return t.url;
  try {
    const url = new URL(text);
    return (url.protocol === "http:" || url.protocol === "https:") && url.hostname !== ""
      ? undefined
      : t.url;
  } catch {
    return t.url;
  }
}

const looksLikeEmail = (text: string) => /^[^\s@<>]+@[^\s@<>]+$/.test(text.trim());
/** `name <address>` or an address. */
const looksLikeMailbox = (text: string) => {
  const angled = /<([^<>]+)>\s*$/.exec(text);
  return looksLikeEmail(angled?.[1] ?? text);
};
const tooLong = (text: string) => text.length > MAX_FIELD || /\p{Cc}/u.test(text);
const HEADER_NAME = /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/;

/** A secret as the API takes it: left out (kept), `null` (removed) or the new value. */
function secretInput(secret: Secret, trim = false): string | null | undefined {
  if (secret.mode === "keep") return undefined;
  if (secret.mode === "remove") return null;
  return trim ? secret.value.trim() : secret.value;
}

/** Why a secret that must be there is not. */
function missing(secret: Secret, what: string): string | undefined {
  if (secret.mode === "keep") return undefined;
  if (secret.mode === "remove" || secret.value.trim() === "") return what;
  return undefined;
}

function checkKind(draft: Draft, placeholders: readonly Placeholder[], errors: DraftErrors) {
  if (draft.kind === "ntfy") {
    errors.server = urlProblem(draft.server.trim().replace(/\/+$/, ""));
    if (!/^[A-Za-z0-9_-]{1,64}$/.test(draft.topic.trim())) errors.topic = t.topic;
    if (draft.ntfyAuth === "token") errors.token = missing(draft.token, t.token);
    if (draft.ntfyAuth === "password") {
      const username = draft.username.trim();
      if (username === "" || tooLong(username)) errors.username = t.username;
      errors.password = missing(draft.password, t.password);
    }
  } else if (draft.kind === "webhook") {
    errors.url =
      missing(draft.url, t.webhookUrl) ??
      (draft.url.mode === "set" ? urlProblem(draft.url.value.trim()) : undefined);
    errors.headers = headersProblem(draft.headers);
    if (draft.format === "custom") errors.template = checkTemplate(draft.template, placeholders);
  } else {
    const host = draft.host.trim();
    if (host === "" || host.length > 253 || /[\s/]/.test(host)) errors.host = t.host;
    const port = Number(draft.port.trim());
    if (!Number.isInteger(port) || port < 1 || port > 65_535) errors.port = t.port;
    if (draft.emailAuth) {
      const username = draft.username.trim();
      if (username === "" || tooLong(username)) errors.username = t.username;
      errors.password = missing(draft.password, t.password);
    }
    if (!looksLikeMailbox(draft.from.trim())) errors.from = t.from;
    const to = recipients(draft.to);
    const bad = to.find((address) => !looksLikeEmail(address));
    if (to.length === 0 || to.length > MAX_RECIPIENTS) errors.to = t.recipients(MAX_RECIPIENTS);
    else if (bad !== undefined) errors.to = t.notAnAddress(bad);
  }
}

function headersProblem(headers: readonly HeaderDraft[]): string | undefined {
  if (headers.length > MAX_HEADERS) return t.headers(MAX_HEADERS);
  const names = new Set<string>();
  for (const header of headers) {
    const name = header.name.trim();
    const key = name.toLowerCase();
    if (!HEADER_NAME.test(name)) return name === "" ? t.headerName : t.notAHeader(name);
    if (RESERVED.includes(key)) return t.reserved(name);
    if (names.has(key)) return t.duplicateHeader(name);
    names.add(key);
    const value = header.value;
    if (value !== null && (value === "" || /[\p{Cc}]/u.test(value) || value.length > MAX_URL)) {
      return t.headerValue(name);
    }
  }
  return undefined;
}

/** The draft as the API takes it, or why it cannot be saved. */
export function checkDraft(
  draft: Draft,
  placeholders: readonly Placeholder[],
): { input: NotifierInput } | { errors: DraftErrors } {
  const errors: DraftErrors = {};
  const name = draft.name.trim();
  if (name === "" || [...name].length > MAX_NAME || /\p{Cc}/u.test(name)) {
    errors.name = t.name(MAX_NAME);
  }
  if (draft.someNodes && draft.nodes.length === 0) errors.nodes = t.nodes;
  if (draft.someNodes && draft.nodes.length > MAX_NODES) errors.nodes = t.tooManyNodes(MAX_NODES);
  checkKind(draft, placeholders, errors);
  for (const key of Object.keys(errors) as (keyof Draft)[]) {
    if (errors[key] === undefined) delete errors[key];
  }
  if (Object.keys(errors).length > 0) return { errors };

  const filter: NotifierFilter = {
    min: draft.min,
    nodes: draft.someNodes ? [...draft.nodes].sort((a, b) => a - b) : [],
    resolved: draft.resolved,
    remind_s: draft.remind > 0 ? draft.remind : null,
  };
  const secrets: SecretsInput = {};
  const put = (field: "url" | "token" | "password", value: string | null | undefined) => {
    if (value !== undefined) secrets[field] = value;
  };
  let spec: NotifierSpec;
  if (draft.kind === "ntfy") {
    const password = draft.ntfyAuth === "password";
    spec = {
      kind: "ntfy",
      server: draft.server.trim().replace(/\/+$/, ""),
      topic: draft.topic.trim(),
      username: password ? draft.username.trim() : null,
    };
    if (draft.ntfyAuth === "none") secrets.token = null;
    if (draft.ntfyAuth === "token") put("token", secretInput(draft.token));
    if (password) put("password", secretInput(draft.password));
  } else if (draft.kind === "webhook") {
    spec = {
      kind: "webhook",
      format: draft.format,
      template: draft.format === "custom" ? draft.template : null,
    };
    put("url", secretInput(draft.url, true));
    secrets.headers = draft.headers.map(
      (header): HeaderInput => ({ name: header.name.trim(), value: header.value }),
    );
  } else {
    spec = {
      kind: "email",
      host: draft.host.trim(),
      port: Number(draft.port.trim()),
      security: draft.security,
      username: draft.emailAuth ? draft.username.trim() : null,
      from: draft.from.trim(),
      to: recipients(draft.to),
    };
    if (draft.emailAuth) put("password", secretInput(draft.password));
  }
  return { input: { name, enabled: draft.enabled, spec, filter, secrets } };
}

/** A stored notifier as the API takes it back, its secrets kept: to turn it on or off. */
export function inputOf(view: NotifierView, enabled = view.enabled): NotifierInput {
  return { name: view.name, enabled, spec: view.spec, filter: view.filter, secrets: {} };
}
