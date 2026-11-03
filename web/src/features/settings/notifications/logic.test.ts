import { describe, expect, it } from "vitest";
import type { NotifierView } from "../../../api/gen/NotifierView";
import { PLACEHOLDERS } from "../../../test/fixtures";
import {
  checkDraft,
  type Draft,
  draftOf,
  inputOf,
  recipients,
  withFormat,
  withSecurity,
} from "./draft";
import {
  checkTemplate,
  insertAt,
  renderTemplate,
  STARTER,
  sampleAlert,
  sampleTest,
} from "./template";
import { ago, deliveryOf, filterOf, remindName, targetOf } from "./words";

const view = (extra: Partial<NotifierView>): NotifierView => ({
  id: 1,
  name: "Phone",
  enabled: true,
  spec: { kind: "ntfy", server: "https://ntfy.sh", topic: "uniwrt-abc", username: null },
  filter: { min: "warning", nodes: [], resolved: true, remind_s: 14_400 },
  secrets: { url: null, token: false, password: false, headers: [] },
  status: { last_ok_at: null, last_error_at: null, last_error: null },
  created_at: 0,
  updated_at: 0,
  ...extra,
});

const check = (draft: Draft) => checkDraft(draft, PLACEHOLDERS);
const errorsOf = (draft: Draft) => {
  const result = check(draft);
  return "errors" in result ? result.errors : {};
};
const inputOfDraft = (draft: Draft) => {
  const result = check(draft);
  if (!("input" in result)) throw new Error(JSON.stringify(result.errors));
  return result.input;
};

describe("a notifier's draft", () => {
  it("starts a new one with D-063's defaults and a topic hard to guess", () => {
    const draft = draftOf(undefined, "ntfy");
    expect(draft).toMatchObject({ min: "warning", resolved: true, remind: 14_400, enabled: true });
    expect(draft.topic).toMatch(/^uniwrt-[a-z2-9]{8}$/);
    expect(draftOf(undefined).topic).not.toBe(draft.topic);
  });

  it("makes an ntfy notifier with a token, and removes the token when signing in with none", () => {
    const draft = { ...draftOf(undefined, "ntfy"), name: " Phone ", topic: "alerts_1" };
    expect(inputOfDraft({ ...draft, server: "https://ntfy.example.com/" })).toEqual({
      name: "Phone",
      enabled: true,
      spec: { kind: "ntfy", server: "https://ntfy.example.com", topic: "alerts_1", username: null },
      filter: { min: "warning", nodes: [], resolved: true, remind_s: 14_400 },
      secrets: { token: null },
    });
    const token = { ...draft, ntfyAuth: "token" as const };
    expect(errorsOf(token).token).toBe("Enter the access token");
    expect(inputOfDraft({ ...token, token: { mode: "set", value: "tk_1" } }).secrets).toEqual({
      token: "tk_1",
    });
  });

  it("keeps stored secrets unless replaced or removed", () => {
    const stored = view({
      spec: { kind: "ntfy", server: "https://ntfy.sh", topic: "t", username: "me" },
      secrets: { url: null, token: false, password: true, headers: [] },
    });
    const draft = draftOf(stored);
    expect(draft).toMatchObject({
      ntfyAuth: "password",
      username: "me",
      password: { mode: "keep" },
    });
    expect(inputOfDraft(draft).secrets).toEqual({});
    expect(inputOfDraft({ ...draft, password: { mode: "set", value: "new" } }).secrets).toEqual({
      password: "new",
    });
    // Replacing with nothing typed is not a removal.
    expect(errorsOf({ ...draft, password: { mode: "set", value: "" } }).password).toBe(
      "Enter the password",
    );
    expect(errorsOf({ ...draftOf(view({})), ntfyAuth: "token" }).token).toBeDefined();
    expect(
      draftOf(view({ secrets: { url: null, token: true, password: false, headers: [] } })),
    ).toMatchObject({ ntfyAuth: "token", token: { mode: "keep" } });
  });

  it("checks ntfy's server and topic as the server does", () => {
    const draft = { ...draftOf(undefined, "ntfy"), name: "x" };
    expect(errorsOf({ ...draft, server: "ftp://ntfy.sh" }).server).toBeDefined();
    expect(errorsOf({ ...draft, server: "https://" }).server).toBeDefined();
    expect(errorsOf({ ...draft, topic: "has space" }).topic).toBeDefined();
    expect(errorsOf({ ...draft, topic: "x".repeat(65) }).topic).toBeDefined();
    expect(errorsOf({ ...draft, name: "" }).name).toBeDefined();
    expect(errorsOf({ ...draft, name: "x".repeat(61) }).name).toBeDefined();
    expect(errorsOf({ ...draft, someNodes: true, nodes: [] }).nodes).toBeDefined();
    expect(inputOfDraft({ ...draft, someNodes: true, nodes: [3, 1], remind: 0 }).filter).toEqual({
      min: "warning",
      nodes: [1, 3],
      resolved: true,
      remind_s: null,
    });
  });

  it("makes a webhook: URL, headers (stored ones kept by name) and a custom template", () => {
    const stored = view({
      spec: { kind: "webhook", format: "discord", template: null },
      secrets: { url: "https://discord.com", token: false, password: false, headers: ["X-Key"] },
    });
    const draft = draftOf(stored);
    expect(draft.url).toEqual({ mode: "keep" });
    expect(inputOfDraft(draft)).toMatchObject({
      spec: { kind: "webhook", format: "discord", template: null },
      secrets: { headers: [{ name: "X-Key", value: null }] },
    });
    const fresh = { ...draftOf(undefined, "webhook"), name: "Hook" };
    expect(errorsOf(fresh).url).toBe("Enter the webhook's URL");
    const withUrl = { ...fresh, url: { mode: "set" as const, value: " https://example.com/h " } };
    expect(inputOfDraft(withUrl).secrets).toEqual({ url: "https://example.com/h", headers: [] });
    const header = (name: string, value: string | null = "v") => ({
      key: name.length,
      name,
      stored: false,
      value,
    });
    expect(errorsOf({ ...withUrl, headers: [header("Content-Type")] }).headers).toBe(
      "uniwrt sets Content-Type itself",
    );
    expect(errorsOf({ ...withUrl, headers: [header("Bad Name")] }).headers).toBeDefined();
    expect(
      errorsOf({ ...withUrl, headers: [header("X-A"), { ...header("x-a"), key: 9 }] }).headers,
    ).toBe("x-a is there twice");
    expect(errorsOf({ ...withUrl, headers: [header("X-A", "")] }).headers).toBeDefined();
    const custom = { ...withUrl, ...withFormat(withUrl, "custom") };
    expect(custom.template).toBe(STARTER);
    expect(inputOfDraft(custom).spec).toEqual({
      kind: "webhook",
      format: "custom",
      template: STARTER,
    });
    expect(errorsOf({ ...custom, template: '{"n": "{{node}}" ' }).template).toMatch(/JSON/);
    // A preset drops a template written before.
    expect(inputOfDraft({ ...custom, format: "slack" }).spec).toMatchObject({ template: null });
  });

  it("makes an email notifier: security moves the usual port, recipients once each", () => {
    const draft: Draft = {
      ...draftOf(undefined, "email"),
      name: "Mail",
      host: "smtp.example.com",
      username: "me",
      password: { mode: "set", value: "pw" },
      from: "uniwrt <u@example.com>",
      to: "a@example.com, B@example.com; a@example.com\nb@example.com",
    };
    expect(inputOfDraft(draft)).toMatchObject({
      spec: {
        kind: "email",
        host: "smtp.example.com",
        port: 587,
        security: "starttls",
        username: "me",
        from: "uniwrt <u@example.com>",
        to: ["a@example.com", "B@example.com"],
      },
      secrets: { password: "pw" },
    });
    expect(withSecurity(draft, "tls")).toEqual({ security: "tls", port: "465" });
    expect(withSecurity({ ...draft, port: "2525" }, "tls")).toEqual({ security: "tls" });
    expect(errorsOf({ ...draft, port: "0" }).port).toBeDefined();
    expect(errorsOf({ ...draft, host: "smtp example" }).host).toBeDefined();
    expect(errorsOf({ ...draft, from: "nobody" }).from).toBeDefined();
    expect(errorsOf({ ...draft, to: "" }).to).toBe("1 to 10 addresses");
    expect(errorsOf({ ...draft, to: "a@example.com nope" }).to).toBe(
      "nope is not an email address",
    );
    expect(errorsOf({ ...draft, emailAuth: true, username: "" }).username).toBeDefined();
    const open = inputOfDraft({ ...draft, emailAuth: false });
    expect(open.spec).toMatchObject({ username: null });
    expect(open.secrets).toEqual({});
    expect(recipients(" a@x.org,,b@x.org ")).toEqual(["a@x.org", "b@x.org"]);
  });

  it("turns a stored one on or off with its secrets kept", () => {
    const stored = view({});
    expect(inputOf(stored, false)).toEqual({
      name: "Phone",
      enabled: false,
      spec: stored.spec,
      filter: stored.filter,
      secrets: {},
    });
  });
});

describe("custom templates", () => {
  it("renders text JSON-escaped and numbers bare or null, as the server does", () => {
    const values = sampleAlert("https://uniwrt.lan", 1_000);
    const out = renderTemplate(
      '{"t": "{{ title }}", "m": "{{message}}", "n": {{node}}}',
      PLACEHOLDERS,
      values,
    );
    expect(out).toEqual({
      text: `{"t": "CPU high · ap-garden", "m": "CPU 93 %, above 90 % · worst 97 %\\nWarning · firing for 0s since 00:00 UTC", "n": 1}`,
    });
    const test = renderTemplate(
      '{"n": {{node}}, "u": "{{url}}"}',
      PLACEHOLDERS,
      sampleTest(null, 0),
    );
    expect(test).toEqual({ text: '{"n": null, "u": ""}' });
  });

  it("says why a template cannot be stored", () => {
    expect(checkTemplate(STARTER, PLACEHOLDERS)).toBeUndefined();
    expect(checkTemplate("  ", PLACEHOLDERS)).toBe("The template is empty");
    expect(checkTemplate(`"${"x".repeat(9000)}"`, PLACEHOLDERS)).toMatch(/longer than 8,192/);
    expect(checkTemplate('{"a": "{{title}"}', PLACEHOLDERS)).toBe("has a {{ without its }}");
    expect(checkTemplate('{"a": "{{nope}}"}', PLACEHOLDERS)).toBe("has no placeholder {{nope}}");
    expect(checkTemplate('{"a": {{title}}}', PLACEHOLDERS)).toMatch(/^does not make JSON/);
    // JSON for an alert, but not for a test, whose alert id is null.
    expect(checkTemplate('{"a": "x{{alert_id}}"}', PLACEHOLDERS)).toBeUndefined();
    expect(checkTemplate('{"a": {{alert_id}}1}', PLACEHOLDERS)).toMatch(/^For a test/);
  });

  it("puts a placeholder in at the caret", () => {
    expect(insertAt('{"a": ""}', 7, 7, "{{title}}")).toEqual({
      template: '{"a": "{{title}}"}',
      caret: 16,
    });
    expect(insertAt("abc", 1, 2, "X")).toEqual({ template: "aXc", caret: 2 });
  });
});

describe("notifiers in words", () => {
  const names = (id: number) => ({ 1: "Gateway", 2: "Attic AP", 3: "Garden" })[id];

  it("says where each kind sends", () => {
    expect(targetOf(view({}))).toBe("ntfy.sh/uniwrt-abc");
    expect(
      targetOf(
        view({
          spec: { kind: "webhook", format: "home_assistant", template: null },
          secrets: { url: "http://ha.lan:8123", token: false, password: false, headers: [] },
        }),
      ),
    ).toBe("Home Assistant · http://ha.lan:8123");
    const email = view({
      spec: {
        kind: "email",
        host: "smtp",
        port: 587,
        security: "starttls",
        username: null,
        from: "u@x.org",
        to: ["a@x.org", "b@x.org", "c@x.org"],
      },
    });
    expect(targetOf(email)).toBe("a@x.org +2");
  });

  it("says which alerts a notifier hears of", () => {
    expect(filterOf(view({}).filter, names)).toBe(
      "Warning and above · Every device · Resolutions · Reminders every 4 hours",
    );
    expect(
      filterOf({ min: "critical", nodes: [1, 2, 3, 9], resolved: false, remind_s: null }, names),
    ).toBe("Critical only · Gateway, Attic AP +2 · No resolutions · No reminders");
    expect(filterOf({ min: "info", nodes: [2], resolved: true, remind_s: 1800 }, names)).toBe(
      "Every level · Attic AP · Resolutions · Reminders every 30m",
    );
    expect(remindName(0)).toBe("Off");
    expect(remindName(86_400)).toBe("Every day");
  });

  it("tells the latest delivery", () => {
    expect(deliveryOf({ last_ok_at: null, last_error_at: null, last_error: null })).toEqual({
      state: "none",
    });
    expect(deliveryOf({ last_ok_at: 5, last_error_at: 3, last_error: "x" })).toEqual({
      state: "ok",
      at: 5,
    });
    expect(deliveryOf({ last_ok_at: 5, last_error_at: 9, last_error: "HTTP 404" })).toEqual({
      state: "failed",
      at: 9,
      why: "HTTP 404",
    });
    expect(ago(0, 30_000)).toBe("just now");
    expect(ago(0, 150_000)).toBe("2m ago");
    expect(ago(0, 7_380_000)).toBe("2h 3m ago");
  });
});
