import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import type { SiteSave } from "../../../api/gen/SiteSave";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { detail, facts } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders } from "../../../test/providers";
import { revisionMeta, siteConfig, siteView } from "../../../test/site";

let calls: { method: string; url: string; body: unknown }[] = [];
let head = 2;
/** The config the last save stored, which later reads return. */
let stored: SiteConfig | undefined;

/** The demo's site with an IoT network on VLAN 30, the node managing `network`. */
function config(): SiteConfig {
  const base = siteConfig();
  const lan = base.networks[0];
  if (!lan) throw new Error("no LAN");
  return {
    ...base,
    networks: [...base.networks, { ...lan, id: "iot", name: "IoT", purpose: "iot", vlan: 30 }],
    nodes: [{ node: 1, hostname: null, managed: ["network"], radios: [], ports: [], raw: [] }],
  };
}

beforeEach(() => {
  calls = [];
  head = 2;
  stored = undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site" && method === "GET")
        return Response.json(siteView(head, stored ?? config()));
      if (url === "/api/v1/site" && method === "PUT") {
        head += 1;
        stored = (body as SiteSave).config;
        return Response.json(siteView(head, stored));
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

describe("Device › Ports", () => {
  it("shows each LAN port as a trunk, then saves an access port", async () => {
    const user = userEvent.setup();
    const { socket, ws } = openTestSocket();
    const Router = createAppRouter(memoryHistory("/devices/1?tab=ports"));
    render(() => (
      <QueryClientProvider client={new QueryClient()}>
        <SignedInProviders socket={socket}>
          <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
        </SignedInProviders>
      </QueryClientProvider>
    ));
    await waitFor(() =>
      expect(ws.sent).toContainEqual({ sub: expect.arrayContaining(["node:1"]) }),
    );
    ws.receive({ type: "node_snapshot", topic: "node:1", node: detail(facts(1)) });
    ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
    expect(await screen.findByRole("tab", { name: "Ports", selected: true })).toBeInTheDocument();
    const card = await screen.findByLabelText("Ethernet ports");
    const lan2 = await within(card).findByRole("row", { name: "lan2" });
    expect(within(lan2).getByText("Trunk")).toBeInTheDocument();

    await user.click(within(card).getByRole("button", { name: "Edit" }));
    await user.selectOptions(within(card).getByLabelText("Untagged lan2"), "iot");
    await user.click(within(lan2).getByRole("button", { name: "None" }));
    expect(within(lan2).getByLabelText("Mesh backhaul on lan2")).toBeChecked();
    const lan1 = within(card).getByRole("row", { name: "lan1" });
    await user.click(within(lan1).getByLabelText("Mesh backhaul on lan1"));
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText(/Saved as revision 3/)).toBeInTheDocument();
    const put = calls.find((c) => c.method === "PUT")?.body as SiteSave;
    expect(put.message).toBe("Ports of gw");
    expect(put.config.nodes[0]?.ports).toEqual([
      { port: "lan1", native: "lan", tagged: { kind: "all" }, backhaul: false },
      { port: "lan2", native: "iot", tagged: { kind: "none" }, backhaul: true },
    ]);
    // The saved profile shows the opt-out.
    ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
    const reads = () => calls.filter((c) => c.method === "GET" && c.url === "/api/v1/site");
    await waitFor(() => expect(reads().length).toBeGreaterThan(2));
    const saved = await within(card).findByRole("row", { name: "lan1" });
    expect(await within(saved).findByText("Opted out")).toBeInTheDocument();
    expect(within(card).getByText(/batman-adv may take it over/)).toBeInTheDocument();
  });
});
