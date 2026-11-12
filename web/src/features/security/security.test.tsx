import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Network } from "../../api/gen/Network";
import type { SiteConfig } from "../../api/gen/SiteConfig";
import type { SiteSave } from "../../api/gen/SiteSave";
import { createAppRouter } from "../../app/routes";
import { Shell } from "../../app/Shell";
import { client } from "../../test/clients";
import { detail, facts, summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { revisionMeta, siteConfig, siteView } from "../../test/site";

let calls: { method: string; url: string; body: unknown }[] = [];
let head = 2;
let config: SiteConfig;
/** A refusal the next save answers with, once. */
let refusal: { path: string; message: string } | undefined;

const IOT: Network = {
  ...(siteConfig().networks[0] as Network),
  id: "iot",
  name: "IoT",
  purpose: "iot",
  vlan: 30,
  ipv4: { address: "10.0.30.1/24", dhcp: null },
};

beforeEach(() => {
  calls = [];
  head = 2;
  refusal = undefined;
  const base = siteConfig();
  config = {
    ...base,
    networks: [...base.networks, IOT],
    firewall: {
      ...base.firewall,
      forwards: [
        {
          name: "NAS",
          enabled: true,
          proto: "tcp",
          ports: { first: 8443, last: 8443 },
          to: "192.168.1.100",
          to_port: 443,
          source: null,
          reflection: true,
        },
      ],
    },
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site" && method === "GET") return Response.json(siteView(head, config));
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
        config = (body as SiteSave).config;
        return Response.json(siteView(head, config));
      }
      if (url === "/api/v1/nodes/1") {
        const gateway = detail(facts(1));
        if (gateway.facts) {
          gateway.facts.port_mappings = [
            {
              proto: "udp",
              external_port: 3074,
              ip: "192.168.1.100",
              internal_port: 3074,
              description: "Xbox",
              expires_in_s: 3600,
            },
          ];
        }
        return Response.json(gateway);
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderSecurity(role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/security"));
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
    clients: [client("02:00:00:00:00:01", { ips: ["192.168.1.100"], hostname: "nas" })],
  });
  await screen.findByRole("heading", { name: "Security", level: 1 });
  return ws;
}

const put = () =>
  calls.filter((c) => c.method === "PUT" && c.url === "/api/v1/site").at(-1)?.body as SiteSave;

describe("Security", () => {
  it("shows the firewall read-only to viewers", async () => {
    await renderSecurity("viewer");
    const matrix = await screen.findByRole("table", { name: "Traffic between networks" });
    expect(
      within(matrix).getByRole("cell", { name: "IoT to Default: Replies only" }),
    ).toBeVisible();
    expect(within(matrix).getByRole("cell", { name: "Default to IoT: Allow" })).toBeVisible();
    expect(within(matrix).getByRole("cell", { name: "IoT to Internet: Allow" })).toBeVisible();
    const forwards = screen.getByLabelText("Port forwards");
    expect(within(forwards).getByText("192.168.1.100 port 443")).toBeVisible();
    expect(within(forwards).getByText("· nas")).toBeVisible();
    const mappings = await screen.findByRole("table", { name: "Ports opened through UPnP" });
    expect(await within(mappings).findByText("Xbox")).toBeVisible();
    expect(within(mappings).getByText("UDP")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Edit" })).toBeNull();
    expect(screen.queryByRole("button", { name: "New forward" })).toBeNull();
    expect(screen.getByText("Only admins can change the firewall.")).toBeVisible();
  });

  it("toggles cells of the matrix and saves them as one revision", async () => {
    const user = userEvent.setup();
    await renderSecurity();
    const card = await screen.findByLabelText("Traffic between networks", { selector: "section" });
    await user.click(within(card).getByRole("button", { name: "Edit" }));
    await user.click(within(card).getByRole("button", { name: "IoT to Default: Replies only" }));
    await user.click(within(card).getByRole("button", { name: "Default to Internet: Allow" }));
    expect(
      within(card).getByRole("button", {
        name: "IoT to Default: Allow. Changed from the purpose's default",
      }),
    ).toHaveAttribute("aria-pressed", "true");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText(/Saved as revision 3/)).toBeVisible();
    expect(put().message).toBe("Traffic between networks");
    expect(put().config.firewall.policies).toEqual([
      { from: "lan", to: "wan", action: "block" },
      { from: "iot", to: "lan", action: "allow" },
    ]);
  });

  it("adds a port forward: the form's checks first, then the server's", async () => {
    const user = userEvent.setup();
    await renderSecurity();
    await user.click(await screen.findByRole("button", { name: "New forward" }));
    const dialog = await screen.findByRole("dialog", { name: "New port forward" });
    await user.type(within(dialog).getByLabelText("Name"), "Game");
    await user.type(within(dialog).getByLabelText("Internet ports"), "8443");
    await user.type(within(dialog).getByLabelText("Host"), "10.0.30.20");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(within(dialog).getByText("NAS already forwards these ports")).toBeVisible();
    expect(calls.some((c) => c.method === "PUT")).toBe(false);

    await user.clear(within(dialog).getByLabelText("Internet ports"));
    await user.type(within(dialog).getByLabelText("Internet ports"), "27015-27030");
    await user.selectOptions(within(dialog).getByLabelText("Protocol"), "udp");
    refusal = { path: "firewall/forwards/1/to", message: "10.0.30.20 is on none of the networks" };
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByText("10.0.30.20 is on none of the networks")).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByText(/Saved as revision 3/)).toBeVisible();
    expect(put().message).toBe("New port forward Game");
    expect(put().config.firewall.forwards[1]).toEqual({
      name: "Game",
      enabled: true,
      proto: "udp",
      ports: { first: 27015, last: 27030 },
      to: "10.0.30.20",
      to_port: null,
      source: null,
      reflection: true,
    });
  });

  it("deletes a forward and sets the DMZ host", async () => {
    const user = userEvent.setup();
    await renderSecurity();
    await user.click(await screen.findByRole("button", { name: "Edit NAS" }));
    const dialog = await screen.findByRole("dialog", { name: "Port forward NAS" });
    expect(within(dialog).getByLabelText("Host port")).toHaveValue("443");
    await user.click(within(dialog).getByRole("button", { name: "Delete forward" }));
    await user.click(
      within(within(dialog).getByRole("alert")).getByRole("button", { name: "Delete forward" }),
    );
    expect(await within(dialog).findByText(/Saved as revision 3/)).toBeVisible();
    expect(put().config.firewall.forwards).toEqual([]);
    await user.click(
      within(dialog).getAllByRole("button", { name: "Close" }).at(-1) as HTMLElement,
    );

    const card = screen.getByLabelText("Port forwards");
    // The dialog is gone before the typing starts: closing, it gives the focus back to its
    // trigger, which on a busy runner can happen mid-word and take the rest of it.
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    await user.click(within(card).getAllByRole("button", { name: "Edit" })[0] as HTMLElement);
    const host = within(card).getByLabelText("Host address");
    // The closed dialog's focus return can land after the click that focuses the field and take
    // every keystroke on a busy runner (CI, three times): type again until the field has them.
    await waitFor(async () => {
      await user.clear(host);
      await user.type(host, "192.168.1.1");
      expect(host).toHaveValue("192.168.1.1");
    });
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText("The gateway's own address")).toBeVisible();
    await user.clear(host);
    await user.type(host, "192.168.1.100");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText(/Saved as revision 4/)).toBeVisible();
    expect(put().message).toBe("DMZ host 192.168.1.100");
    expect(put().config.firewall.dmz).toBe("192.168.1.100");
  });

  it("chooses UPnP's networks", async () => {
    const user = userEvent.setup();
    await renderSecurity();
    const card = await screen.findByLabelText("UPnP and NAT-PMP", { selector: "section" });
    expect(within(card).getByLabelText("Let clients open ports")).toBeChecked();
    expect(within(card).getByLabelText("Default")).toBeDisabled();
    await user.click(within(card).getByRole("button", { name: "Edit" }));
    await user.click(within(card).getByLabelText("Default"));
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(within(card).getByText("Choose at least one network")).toBeVisible();
    await user.click(within(card).getByLabelText("IoT"));
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText(/Saved as revision 3/)).toBeVisible();
    expect(put().message).toBe("UPnP networks");
    expect(put().config.firewall.upnp).toEqual({ enabled: true, networks: ["iot"] });
  });
});
