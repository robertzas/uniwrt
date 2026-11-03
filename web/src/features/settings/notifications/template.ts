/**
 * Custom webhook templates, rendered as the server renders them (`uniwrt_notify::render`):
 * JSON with `{{placeholders}}`, text ones JSON-escaped without quotes, number ones digits or
 * `null`. A template must make JSON for an alert and for a test, which names no alert.
 */
import type { Placeholder } from "../../../api/gen/Placeholder";
import { strings } from "../../../lib/strings";

const t = strings.settings.notifications.template;

/** The longest template, in bytes. */
export const MAX_TEMPLATE = 8 * 1024;

/** Where a custom template starts: every field a receiver usually wants. */
export const STARTER = `{
  "title": "{{title}}",
  "text": "{{message}}",
  "level": "{{level}}",
  "event": "{{event}}",
  "url": "{{url}}",
  "at": {{at}},
  "node": {{node}}
}`;

export type Values = Readonly<Record<string, string | number | null>>;

/** What an alert's message fills in: the server's own example (CPU high on a device). */
export function sampleAlert(link: string | null, at: number): Values {
  const cause = "CPU 93 %, above 90 % · worst 97 %";
  return {
    title: "CPU high · ap-garden",
    message: `${cause}\nWarning · firing for 0s since ${new Date(at).toISOString().slice(11, 16)} UTC`,
    level: "Warning",
    event: "opened",
    url: link === null ? "" : `${link}/insights/alerts?alert=1`,
    at,
    at_iso: new Date(at).toISOString().replace(/\.\d{3}Z$/, "Z"),
    alerts: 1,
    alert_id: 1,
    name: "CPU high",
    subject: "ap-garden",
    node: 1,
    cause,
  };
}

/** What a test fills in: no alert, so its names are empty and its numbers `null`. */
export function sampleTest(link: string | null, at: number): Values {
  return {
    title: "uniwrt test notification",
    message: "Alerts from uniwrt reach you here.",
    level: "Info",
    event: "test",
    url: link === null ? "" : `${link}/insights/alerts`,
    at,
    at_iso: new Date(at).toISOString().replace(/\.\d{3}Z$/, "Z"),
    alerts: 0,
    alert_id: null,
    name: "",
    subject: "",
    node: null,
    cause: "",
  };
}

/** The template filled from `values`, or why it cannot be (`"does not make JSON: …"`). */
export function renderTemplate(
  template: string,
  placeholders: readonly Placeholder[],
  values: Values,
): { text: string } | { error: string } {
  const numbers = new Map(placeholders.map((p) => [p.name, p.number]));
  let out = "";
  let rest = template;
  for (let start = rest.indexOf("{{"); start >= 0; start = rest.indexOf("{{")) {
    out += rest.slice(0, start);
    const after = rest.slice(start + 2);
    const end = after.indexOf("}}");
    if (end < 0) return { error: t.unclosed };
    const name = after.slice(0, end).trim();
    const number = numbers.get(name);
    if (number === undefined) return { error: t.unknown(name) };
    const value = values[name] ?? null;
    out += number
      ? value === null
        ? "null"
        : String(value)
      : JSON.stringify(String(value ?? "")).slice(1, -1);
    rest = after.slice(end + 2);
  }
  out += rest;
  try {
    JSON.parse(out);
  } catch (error) {
    return { error: t.notJson(error instanceof Error ? error.message : String(error)) };
  }
  return { text: out };
}

/** Why the template cannot be stored: empty, too long, or not JSON for an alert or a test. */
export function checkTemplate(
  template: string,
  placeholders: readonly Placeholder[],
): string | undefined {
  if (template.trim() === "") return t.empty;
  const bytes = new TextEncoder().encode(template).length;
  if (bytes > MAX_TEMPLATE) return t.tooLong(MAX_TEMPLATE);
  const at = Date.now();
  for (const values of [sampleAlert(null, at), sampleTest(null, at)]) {
    const rendered = renderTemplate(template, placeholders, values);
    if ("error" in rendered) {
      return values.event === "test" ? t.forTest(rendered.error) : rendered.error;
    }
  }
  return undefined;
}

/** The template with `text` put in place of the selection `[from, to)`, and where the caret goes. */
export function insertAt(
  template: string,
  from: number,
  to: number,
  text: string,
): { template: string; caret: number } {
  return {
    template: template.slice(0, from) + text + template.slice(to),
    caret: from + text.length,
  };
}
