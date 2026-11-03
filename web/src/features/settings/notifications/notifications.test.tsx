import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NotificationSettings } from "../../../api/gen/NotificationSettings";
import type { NotifierView } from "../../../api/gen/NotifierView";
import { createAppRouter } from "../../../app/routes";
import { PLACEHOLDERS, summary } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";

const NOW = Date.now();
let calls: { url: string; method: string; body: unknown }[] = [];
let notifiers: NotifierView[] = [];
let settings: NotificationSettings = { link_url: null };
let testAnswer: Response | undefined;

const phone: NotifierView = {
  id: 1,
  name: "Phone",
  enabled: true,
  spec: { kind: "ntfy", server: "https://ntfy.sh", topic: "uniwrt-abc", username: null },
  filter: { min: "warning", nodes: [], resolved: true, remind_s: 14_400 },
  secrets: { url: null, token: true, password: false, headers: [] },
  status: { last_ok_at: NOW - 300_000, last_error_at: null, last_error: null },
  created_at: NOW,
  updated_at: NOW,
};

const discord: NotifierView = {
  ...phone,
  id: 2,
  name: "Discord #net",
  spec: { kind: "webhook", format: "discord", template: null },
  filter: { min: "critical", nodes: [2], resolved: false, remind_s: null },
  secrets: { url: "https://discord.com", token: false, password: false, headers: ["X-Key"] },
  status: { last_ok_at: NOW - 600_000, last_error_at: NOW - 120_000, last_error: "HTTP 404" },
};

beforeEach(() => {
  calls = [];
  notifiers = [phone, discord];
  settings = { link_url: null };
  testAnswer = undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ url, method, body });
      if (url === "/api/v1/notifiers" && method === "GET") {
        return Response.json({ notifiers, settings, placeholders: PLACEHOLDERS });
      }
      if (url === "/api/v1/notifiers" && method === "POST") {
        const { secrets: _, ...rest } = body;
        return Response.json(
          {
            ...rest,
            id: 3,
            secrets: { url: null, token: false, password: false, headers: [] },
            status: { last_ok_at: null, last_error_at: null, last_error: null },
            created_at: NOW,
            updated_at: NOW,
          },
          { status: 201 },
        );
      }
      if (url.startsWith("/api/v1/notifiers/") && method === "PUT") {
        const id = Number(url.split("/").pop());
        const { secrets: _, ...rest } = body;
        return Response.json({ ...notifiers.find((n) => n.id === id), ...rest });
      }
      if (url === "/api/v1/notifiers/test") {
        return testAnswer ?? new Response(null, { status: 204 });
      }
      if (url === "/api/v1/notifications/settings") return Response.json(body);
      if (method === "DELETE") return new Response(null, { status: 204 });
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderAt(path: string, role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory(path));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router />
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "Gateway", { role: "gateway" }), summary(2, "Attic AP")]);
  await screen.findByRole("heading", { name: "Settings", level: 1 });
  return screen.findByRole("region", { name: "Notifiers" });
}

const sent = (path: string, method?: string) =>
  calls.filter((c) => c.method !== "GET" && c.url === path && (!method || c.method === method));

describe("Settings › Notifications", () => {
  it("lists the notifiers with where they send, their filter and their last delivery", async () => {
    const list = await renderAt("/settings");
    const rows = await within(list).findAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(within(list).getByText("2 on")).toBeVisible();
    expect(rows[0]).toHaveTextContent("ntfy.sh/uniwrt-abc");
    expect(rows[0]).toHaveTextContent(
      "Warning and above · Every device · Resolutions · Reminders every 4 hours",
    );
    expect(rows[0]).toHaveTextContent("Delivered 5m ago");
    expect(rows[1]).toHaveTextContent("Discord · https://discord.com");
    expect(rows[1]).toHaveTextContent("Critical only · Attic AP · No resolutions · No reminders");
    expect(rows[1]).toHaveTextContent("Failed 2m ago: HTTP 404");
  });

  it("turns one off with its secrets kept, and removes one after asking", async () => {
    const user = userEvent.setup();
    const list = await renderAt("/settings?tab=notifications");
    await user.click(await within(list).findByRole("checkbox", { name: "Turn off Phone" }));
    await waitFor(() => expect(sent("/api/v1/notifiers/1", "PUT")).toHaveLength(1));
    expect(sent("/api/v1/notifiers/1")[0]?.body).toEqual({
      name: "Phone",
      enabled: false,
      spec: phone.spec,
      filter: phone.filter,
      secrets: {},
    });
    await user.click(within(list).getByRole("button", { name: "Remove: Discord #net" }));
    const dialog = await screen.findByRole("dialog", { name: "Remove Discord #net?" });
    await user.click(within(dialog).getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(sent("/api/v1/notifiers/2", "DELETE")).toHaveLength(1));
    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(1));
  });

  it("adds an ntfy notifier, checked before it is sent", async () => {
    const user = userEvent.setup();
    const list = await renderAt("/settings");
    await user.click(within(list).getByRole("button", { name: "Add notifier" }));
    const dialog = await screen.findByRole("dialog", { name: "Add a notifier" });
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByText(/A name has 1 to 60 characters/)).toBeVisible();
    expect(sent("/api/v1/notifiers")).toHaveLength(0);

    await user.type(within(dialog).getByLabelText("Name"), "My phone");
    const topic = within(dialog).getByLabelText("Topic");
    await user.clear(topic);
    await user.type(topic, "home-alerts");
    await user.click(within(dialog).getByRole("button", { name: "Critical" }));
    await user.click(within(dialog).getByRole("button", { name: "Some devices" }));
    await user.click(within(dialog).getByRole("checkbox", { name: "Gateway" }));
    await user.selectOptions(within(dialog).getByLabelText("Reminders"), "3600");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(sent("/api/v1/notifiers")).toHaveLength(1));
    expect(sent("/api/v1/notifiers")[0]?.body).toEqual({
      name: "My phone",
      enabled: true,
      spec: { kind: "ntfy", server: "https://ntfy.sh", topic: "home-alerts", username: null },
      filter: { min: "critical", nodes: [1], resolved: true, remind_s: 3600 },
      secrets: { token: null },
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(await within(list).findByText("My phone")).toBeVisible();
  });

  it("edits a webhook: stored secrets shown as set, a test with them, the server's why", async () => {
    const user = userEvent.setup();
    const list = await renderAt("/settings");
    await user.click(await within(list).findByRole("button", { name: "Edit: Discord #net" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit Discord #net" });
    expect(within(dialog).getByText("https://discord.com/…")).toBeVisible();
    expect(within(dialog).queryByRole("button", { name: "Email" })).not.toBeInTheDocument();

    testAnswer = new Response(
      JSON.stringify({ title: "Bad Gateway", detail: "The test failed: HTTP 401 Unauthorized" }),
      { status: 502, headers: { "content-type": "application/problem+json" } },
    );
    await user.click(within(dialog).getByRole("button", { name: "Send a test" }));
    expect(await within(dialog).findByText("The test failed: HTTP 401 Unauthorized")).toBeVisible();
    expect(sent("/api/v1/notifiers/test")[0]?.body).toMatchObject({
      id: 2,
      spec: { kind: "webhook", format: "discord", template: null },
      secrets: { headers: [{ name: "X-Key", value: null }] },
    });

    // A new URL replaces the stored one; the stored header goes.
    await user.click(within(dialog).getByRole("button", { name: "Replace: Webhook URL" }));
    await user.type(
      within(dialog).getByLabelText("Webhook URL"),
      "https://discord.com/api/webhooks/2/x",
    );
    await user.click(within(dialog).getByRole("button", { name: "Remove the header X-Key" }));
    testAnswer = undefined;
    await user.click(within(dialog).getByRole("button", { name: "Send a test" }));
    expect(await within(dialog).findByText(/Test sent/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(sent("/api/v1/notifiers/2", "PUT")).toHaveLength(1));
    expect(sent("/api/v1/notifiers/2")[0]?.body).toMatchObject({
      secrets: { url: "https://discord.com/api/webhooks/2/x", headers: [] },
    });
  });

  it("writes a custom template with placeholders and a preview", async () => {
    const user = userEvent.setup();
    const list = await renderAt("/settings");
    await user.click(await within(list).findByRole("button", { name: "Edit: Discord #net" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit Discord #net" });
    await user.selectOptions(within(dialog).getByLabelText("Format"), "custom");
    const area = within(dialog).getByLabelText("Body template") as HTMLTextAreaElement;
    expect(area.value).toContain('"title": "{{title}}"');
    expect(within(dialog).getByText(/"title": "CPU high · ap-garden"/)).toBeVisible();

    await user.clear(area);
    await user.type(area, '{{"n": ');
    await user.click(within(dialog).getByRole("button", { name: "{{node}}" }));
    expect(area.value).toBe('{"n": {{node}}');
    expect(await within(dialog).findByText(/The template does not make JSON/)).toBeVisible();
    await user.type(area, "}");
    expect(area.value).toBe('{"n": {{node}}}');
    await user.click(within(dialog).getByRole("button", { name: "A test" }));
    expect(within(dialog).getByText(/"n": null/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(sent("/api/v1/notifiers/2", "PUT")).toHaveLength(1));
    expect(sent("/api/v1/notifiers/2")[0]?.body).toMatchObject({
      spec: { kind: "webhook", format: "custom", template: '{"n": {{node}}}' },
    });
  });

  it("saves the link address, prefilled with this page's while unset", async () => {
    const user = userEvent.setup();
    await renderAt("/settings");
    const card = await screen.findByRole("region", { name: "Links in messages" });
    expect(within(card).getByText("Not set")).toBeVisible();
    const input = within(card).getByLabelText("This controller's address") as HTMLInputElement;
    expect(input.value).toBe(location.origin);
    await user.clear(input);
    await user.type(input, "uniwrt.lan");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText("Not an http:// or https:// URL")).toBeVisible();
    await user.clear(input);
    await user.type(input, "https://uniwrt.lan:8443/");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(sent("/api/v1/notifications/settings")[0]?.body).toEqual({
        link_url: "https://uniwrt.lan:8443",
      }),
    );
    await waitFor(() => expect(within(card).queryByText("Not set")).not.toBeInTheDocument());
  });

  it("shows viewers everything and lets them change nothing", async () => {
    const list = await renderAt("/settings", "viewer");
    await within(list).findAllByRole("listitem");
    expect(within(list).getByText("Only admins change notifications")).toBeVisible();
    expect(within(list).queryByRole("button", { name: "Add notifier" })).not.toBeInTheDocument();
    expect(within(list).queryByRole("checkbox")).not.toBeInTheDocument();
    expect(within(list).queryByRole("button", { name: /Edit/ })).not.toBeInTheDocument();
    const card = screen.getByRole("region", { name: "Links in messages" });
    expect(within(card).getByLabelText("This controller's address")).toHaveAttribute("readonly");
    expect(within(card).queryByRole("button")).not.toBeInTheDocument();
  });
});
