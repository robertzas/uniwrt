import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Network } from "../../api/gen/Network";
import type { SiteSave } from "../../api/gen/SiteSave";
import { createAppRouter } from "../../app/routes";
import { Shell } from "../../app/Shell";
import { client } from "../../test/clients";
import { detail, facts, summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { revisionMeta, siteView } from "../../test/site";

let calls: { method: string; url: string; body: unknown }[] = [];
let head = 2;
/** A refusal the next save answers with, once. */
let refusal: { path: string; message: string } | undefined;

const GUEST: Network = {
  id: "guest",
  name: "Guest",
  purpose: "guest",
  vlan: 40,
  ipv4: {
    address: "192.168.40.1/24",
    dhcp: {
      start: "192.168.40.100",
      end: "192.168.40.249",
      lease_time_s: 43200,
      reservations: [],
      options: { dns: [], ntp: [], domain: null, boot: null, custom: [] },
    },
  },
  ipv6: "delegated",
  ipv6_clients: "slaac_stateless",
  isolation: true,
  mdns: false,
  igmp_snooping: false,
};

beforeEach(() => {
  calls = [];
  head = 2;
  refusal = undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site" && method === "GET") return Response.json(siteView(head));
      if (url === "/api/v1/site" && method === "PUT") {
        if (refusal) {
          const issues = [{ ...refusal, severity: "error" }];
          refusal = undefined;
          return Response.json(
            { title: "Unprocessable", detail: "The document has errors", issues },
            { status: 422, headers: { "content-type": "application/problem+json" } },
          );
        }
        head += 1;
        return Response.json(siteView(head, (body as SiteSave).config));
      }
      if (url === "/api/v1/nodes/1") {
        const gateway = detail(facts(1));
        if (gateway.facts) {
          gateway.facts.leases = ["192.168.1.100", "192.168.1.101", "192.168.1.20"].map((ip) => ({
            ip,
            mac: null,
            hostname: null,
            client_id: null,
            device: null,
            expires_in_s: 600,
          }));
        }
        return Response.json(gateway);
      }
      if (url === "/api/v1/ipam/network?purpose=guest") return Response.json(GUEST);
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderNetworks(role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/networks"));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "gw", { role: "gateway" })]);
  ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
  await waitFor(() => expect(ws.sent).toContainEqual({ sub: expect.arrayContaining(["clients"]) }));
  ws.receive({
    type: "clients_snapshot",
    topic: "clients",
    clients: [
      client("02:00:00:00:00:01", { ips: ["192.168.1.100"] }),
      client("02:00:00:00:00:02", { ips: ["192.168.1.150"], online: false }),
    ],
  });
  await screen.findByRole("heading", { name: "Networks", level: 1 });
  return ws;
}

const put = () =>
  calls.filter((c) => c.method === "PUT" && c.url === "/api/v1/site").at(-1)?.body as SiteSave;

describe("Networks", () => {
  it("lists the networks with their subnet, pool use and clients", async () => {
    await renderNetworks("viewer");
    const row = await screen.findByRole("listitem", { name: "Default" });
    expect(within(row).getByText("192.168.1.0/24")).toBeInTheDocument();
    expect(within(row).getByText("Gateway 192.168.1.1")).toBeInTheDocument();
    expect(within(row).getByText("Untagged")).toBeInTheDocument();
    // Two of the gateway's leases sit in the pool; one client is online in the subnet.
    expect(await within(row).findByText("2 of 150 leased")).toBeInTheDocument();
    expect(within(row).getByText("1 client online")).toBeInTheDocument();
    expect(within(row).queryByRole("button", { name: /Edit/ })).not.toBeInTheDocument();
    expect(screen.getByText("Only admins can change the networks.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New network" })).not.toBeInTheDocument();
  });

  it("adds a guest network from the address plan's suggestion", async () => {
    const user = userEvent.setup();
    await renderNetworks();
    await user.click(await screen.findByRole("button", { name: "New network" }));
    const chooser = await screen.findByRole("dialog", { name: "New network" });
    await user.click(within(chooser).getByRole("button", { name: /Guest/ }));
    await user.click(within(chooser).getByRole("button", { name: "Continue" }));
    const editor = await screen.findByRole("dialog", { name: "New network" });
    await waitFor(() => expect(within(editor).getByLabelText("Name")).toHaveValue("Guest"));
    expect(within(editor).getByLabelText("VLAN ID")).toHaveValue("40");
    expect(within(editor).getByLabelText("Client isolation")).toBeChecked();
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    expect(await within(editor).findByText(/Saved as revision 3/)).toBeInTheDocument();
    expect(put().message).toBe("New network Guest");
    expect(put().config.networks.map((n) => n.id)).toEqual(["lan", "guest"]);
    expect(put().config.networks[1]).toEqual(GUEST);
  });

  it("edits a network: client checks first, then the server's issues on their fields", async () => {
    const user = userEvent.setup();
    await renderNetworks();
    await user.click(await screen.findByRole("button", { name: "Edit Default" }));
    const editor = await screen.findByRole("dialog", { name: "Network Default" });
    const start = within(editor).getByLabelText("Pool start");
    await user.clear(start);
    await user.type(start, "192.168.2.10");
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    expect(within(editor).getByText("Not a host of the subnet")).toBeInTheDocument();
    expect(calls.some((c) => c.method === "PUT")).toBe(false);

    await user.clear(start);
    await user.type(start, "192.168.1.50");
    await user.click(within(editor).getByLabelText("Network boot (PXE)"));
    await user.type(within(editor).getByLabelText("Boot file"), "netboot.xyz.efi");
    await user.click(within(editor).getByRole("button", { name: "Add a reservation" }));
    await user.type(within(editor).getByLabelText("MAC address"), "02-00-00-00-00-AA");
    await user.type(within(editor).getByLabelText("Address"), "192.168.1.40");
    refusal = {
      path: "networks/lan/ipv4/dhcp/reservations/02:00:00:00:00:aa/ip",
      message: "192.168.1.40 is reserved twice",
    };
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    expect(await within(editor).findByText("192.168.1.40 is reserved twice")).toBeInTheDocument();
    await user.click(within(editor).getByRole("button", { name: "Save" }));
    expect(await within(editor).findByText(/Saved as revision 3/)).toBeInTheDocument();
    const lan = put().config.networks[0];
    expect(put().message).toBe("Network Default");
    expect(lan?.ipv4?.dhcp?.start).toBe("192.168.1.50");
    expect(lan?.ipv4?.dhcp?.options.boot).toEqual({
      filename: "netboot.xyz.efi",
      server_name: null,
      server: null,
    });
    expect(lan?.ipv4?.dhcp?.reservations).toEqual([
      { mac: "02:00:00:00:00:aa", ip: "192.168.1.40", name: null },
    ]);
    // The default network can't be deleted.
    expect(within(editor).queryByRole("button", { name: "Delete network" })).toBeNull();
  });

  it("edits the local DNS records", async () => {
    const user = userEvent.setup();
    await renderNetworks();
    const card = await screen.findByLabelText("Local DNS");
    expect(within(card).getByText("No custom records.")).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Edit" }));
    await user.click(within(card).getByRole("button", { name: "Add a record" }));
    await user.type(within(card).getByLabelText("Name"), "uniwrt");
    await user.type(within(card).getByLabelText("Address"), "192.168.1.5");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(
      within(card).getByText("uniwrt.lan always points at the controller"),
    ).toBeInTheDocument();
    await user.clear(within(card).getByLabelText("Name"));
    await user.type(within(card).getByLabelText("Name"), "nas");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText(/Saved as revision 3/)).toBeInTheDocument();
    expect(put().config.services.dns_records).toEqual([
      { kind: "host", name: "nas", ip: "192.168.1.5" },
    ]);
  });
});
