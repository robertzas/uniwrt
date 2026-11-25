import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NodeUci } from "../../../api/gen/NodeUci";
import type { RawSection } from "../../../api/gen/RawSection";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { detail, facts, summary } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import { revisionMeta, siteConfig, siteView } from "../../../test/site";

const MASK = "••••••••";
let calls: { method: string; url: string; body: unknown }[] = [];
let uci: () => Response;
let raw: RawSection[] = [];

const pending: RawSection = {
  package: "network",
  type: "interface",
  name: "iot",
  options: { proto: "static" },
  merge: false,
  unset: [],
  remove: false,
  matching: {},
};

function read(revision = 2): NodeUci {
  return {
    node: 1,
    read_at: 1_800_000_000_000,
    revision,
    packages: [
      {
        name: "network",
        owner: "rendered",
        error: null,
        sections: [
          {
            type: "interface",
            name: "lan",
            options: [{ name: "proto", value: "static", secret: false }],
            overrides: [],
            key: null,
          },
          {
            type: "route",
            name: null,
            options: [
              { name: "target", value: "10.9.0.0/16", secret: false },
              { name: "gateway", value: "192.168.1.2", secret: false },
            ],
            overrides: [],
            key: null,
          },
        ],
      },
      {
        name: "wireless",
        owner: "node",
        error: null,
        sections: [
          {
            type: "wifi-iface",
            name: "default_radio0",
            options: [
              { name: "ssid", value: "home", secret: false },
              { name: "key", value: MASK, secret: true },
            ],
            overrides: [],
            key: null,
          },
        ],
      },
    ],
    unmatched: raw.length > 0 ? [0] : [],
  };
}

function config(): SiteConfig {
  return {
    ...siteConfig(),
    nodes: [
      {
        node: 1,
        hostname: null,
        managed: [],
        radios: [],
        ports: [],
        raw,
        reboot: { kind: "site" },
        led_schedule: true,
      },
    ],
  };
}

beforeEach(() => {
  calls = [];
  raw = [];
  uci = () => Response.json(read());
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site" && method === "PUT")
        return Response.json(siteView(3, body.config));
      if (url === "/api/v1/site") return Response.json(siteView(2, config()));
      if (url === "/api/v1/nodes/1/uci") return uci();
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderTab(options: { role?: "admin" | "viewer"; managed?: boolean } = {}) {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/devices/1?tab=uci"));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={options.role ?? "admin"}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  await waitFor(() => {
    const sent = ws.sent.at(-1) as { sub?: string[] } | undefined;
    expect(sent?.sub).toContain("node:1");
  });
  const mode = options.managed === false ? "observe" : "managed";
  sendFleet(ws, [summary(1, "gw", { mode })]);
  const node = detail(facts(1));
  node.node.mode = mode;
  ws.receive({ type: "node_snapshot", topic: "node:1", node });
  ws.receive({ type: "config", topic: "config", head: revisionMeta(2), applied: [] });
  return screen.findByRole("region", { name: "UCI explorer of gw" });
}

const lastSave = () =>
  calls.filter((c) => c.method === "PUT").at(-1)?.body as
    | { config: SiteConfig; secrets?: Record<string, string> }
    | undefined;

describe("UciTab", () => {
  it("browses and searches the packages, secrets masked, read-only in observe mode", async () => {
    const card = await renderTab({ managed: false });
    expect(await within(card).findByText(/observe mode/)).toBeInTheDocument();
    const lan = await within(card).findByRole("article", { name: "network.lan" });
    expect(within(lan).getByText("static")).toBeInTheDocument();
    expect(within(card).getByRole("article", { name: "network.@route[0]" })).toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();

    await userEvent.type(within(card).getByRole("searchbox"), "home");
    const iface = await within(card).findByRole("article", { name: "wireless.default_radio0" });
    expect(within(iface).getByText(MASK)).toBeInTheDocument();
    expect(within(card).queryByRole("article", { name: "network.lan" })).not.toBeInTheDocument();
    await userEvent.type(within(card).getByRole("searchbox"), " nothing");
    expect(await within(card).findByText("Nothing matches the search.")).toBeInTheDocument();
  });

  it("is read-only for viewers", async () => {
    const card = await renderTab({ role: "viewer" });
    expect(
      await within(card).findByText("Only admins can edit the configuration."),
    ).toBeInTheDocument();
    await within(card).findByRole("article", { name: "network.lan" });
    expect(within(card).queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
  });

  it("saves an edit as a merge, a typed secret sealed, into the site's next revision", async () => {
    const card = await renderTab();
    await userEvent.click(within(card).getByRole("button", { name: /wireless/ }));
    const iface = await within(card).findByRole("article", { name: "wireless.default_radio0" });
    await userEvent.click(within(iface).getByRole("button", { name: "Edit" }));
    const form = within(iface).getByRole("form", { name: "Edit wireless.default_radio0" });
    const ssid = within(form).getByRole("textbox", { name: "Value of ssid" });
    await userEvent.clear(ssid);
    await userEvent.type(ssid, "lab");
    const secret = within(form).getByPlaceholderText("Leave empty to keep it");
    await userEvent.type(secret, "new passphrase");
    await userEvent.click(within(form).getByRole("button", { name: "Save to the site" }));

    await waitFor(() => expect(lastSave()).toBeDefined());
    const save = lastSave();
    expect(save?.config.nodes[0]?.raw).toEqual([
      {
        package: "wireless",
        type: "wifi-iface",
        name: "default_radio0",
        options: { ssid: "lab" },
        merge: true,
        unset: [],
        remove: false,
        matching: {},
      },
    ]);
    expect(save?.secrets).toEqual({ "nodes/1/raw/0/key": "new passphrase" });
    expect(await within(card).findByText(/Saved as revision 3/)).toBeInTheDocument();
  });

  it("deletes an anonymous section by what it holds, and adds a section", async () => {
    const card = await renderTab();
    const route = await within(card).findByRole("article", { name: "network.@route[0]" });
    await userEvent.click(within(route).getByRole("button", { name: "Delete section" }));
    await userEvent.click(within(route).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(lastSave()).toBeDefined());
    expect(lastSave()?.config.nodes[0]?.raw).toEqual([
      expect.objectContaining({
        type: "route",
        remove: true,
        matching: { target: "10.9.0.0/16", gateway: "192.168.1.2" },
      }),
    ]);

    calls = [];
    await userEvent.click(within(card).getByRole("button", { name: "Add section" }));
    const form = within(card).getByRole("form", { name: "Add a section to network" });
    await userEvent.type(within(form).getByLabelText("Type"), "interface");
    await userEvent.type(within(form).getByLabelText("Name"), "lan");
    await userEvent.click(within(form).getByRole("button", { name: "Save to the site" }));
    expect(
      await within(form).findByText("The package has a section named lan: edit that one"),
    ).toBeInTheDocument();
    await userEvent.clear(within(form).getByLabelText("Name"));
    await userEvent.type(within(form).getByLabelText("Name"), "guest");
    await userEvent.type(within(form).getByRole("textbox", { name: "Option" }), "proto");
    await userEvent.type(within(form).getByRole("textbox", { name: "Value of proto" }), "dhcp");
    await userEvent.click(within(form).getByRole("button", { name: "Save to the site" }));
    await waitFor(() => expect(lastSave()).toBeDefined());
    expect(lastSave()?.config.nodes[0]?.raw).toEqual([
      expect.objectContaining({ type: "interface", name: "guest", options: { proto: "dhcp" } }),
    ]);
  });

  it("lists the overrides the device doesn't hold, which an admin can drop", async () => {
    raw = [pending];
    const card = await renderTab();
    const list = await within(card).findByRole("region", { name: "Overrides not on the device" });
    expect(within(list).getByText("network.iot")).toBeInTheDocument();
    await userEvent.click(within(list).getByRole("button", { name: "Drop" }));
    await waitFor(() => expect(lastSave()?.config.nodes[0]?.raw).toEqual([]));
  });

  it("waits for a read of the latest revision before editing", async () => {
    uci = () => Response.json(read(1));
    const card = await renderTab();
    expect(await within(card).findByText(/The site changed since this read/)).toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
  });

  it("says why the read failed", async () => {
    uci = () =>
      new Response(JSON.stringify({ title: "Conflict", detail: "The node is offline" }), {
        status: 409,
        headers: { "content-type": "application/problem+json" },
      });
    const card = await renderTab();
    expect(await within(card).findByRole("alert")).toHaveTextContent("The node is offline");
    uci = () => Response.json(read());
    await userEvent.click(within(card).getByRole("button", { name: "Try again" }));
    expect(await within(card).findByRole("article", { name: "network.lan" })).toBeInTheDocument();
  });
});
