import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Ddns } from "../../../api/gen/Ddns";
import type { ServiceFacts } from "../../../api/gen/ServiceFacts";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import type { SiteSave } from "../../../api/gen/SiteSave";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { blocklistsView } from "../../../test/blocklists";
import { detail, facts, summary } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import { revisionMeta, siteConfig, siteView } from "../../../test/site";

let calls: { method: string; url: string; body: unknown }[] = [];
let head = 2;
let config: SiteConfig = siteConfig();
const NO_BLOCKLIST = { sha256: null, domains: null, conf: false };
let services: ServiceFacts = {
  ddns: [],
  doh_instances: null,
  dot_instances: null,
  blocklist: NO_BLOCKLIST,
  wireguard: null,
  tailscale: null,
  ssh: null,
};
let issues: { path: string; severity: "warning"; message: string }[] = [];
/** A refusal the next save answers with, once. */
let refusal: { path: string; message: string } | undefined;

const CF: Ddns = {
  id: "home",
  enabled: true,
  hostname: "home.example.com",
  provider: { kind: "cloudflare", zone: "example.com" },
  ipv6: false,
  source: "wan",
  secret: "config/00000000000000000000000000000001",
};

beforeEach(() => {
  calls = [];
  head = 2;
  config = siteConfig();
  services = {
    ddns: [],
    doh_instances: null,
    dot_instances: null,
    blocklist: NO_BLOCKLIST,
    wireguard: null,
    tailscale: null,
    ssh: null,
  };
  issues = [];
  refusal = undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site" && method === "GET") {
        return Response.json({ ...siteView(head, config), issues });
      }
      if (url === "/api/v1/site" && method === "PUT") {
        if (refusal) {
          const found = [{ ...refusal, severity: "error" }];
          refusal = undefined;
          return Response.json(
            { title: "Unprocessable", detail: "The document has errors", issues: found },
            { status: 422, headers: { "content-type": "application/problem+json" } },
          );
        }
        head += 1;
        config = (body as SiteSave).config;
        return Response.json(siteView(head, config));
      }
      if (url === "/api/v1/services/blocklists") return Response.json(blocklistsView());
      if (url === "/api/v1/nodes/1") {
        const gateway = detail(facts(1));
        if (gateway.facts) gateway.facts.services = services;
        return Response.json(gateway);
      }
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
  await screen.findByRole("heading", { name: "Networks", level: 1 });
}

const put = () =>
  calls.filter((c) => c.method === "PUT" && c.url === "/api/v1/site").at(-1)?.body as SiteSave;

describe("Network services", () => {
  it("shows the saved services to viewers, with what the gateway reports", async () => {
    config.services.upstream = {
      encrypted: [
        {
          protocol: "doh",
          url: "https://dns.quad9.net/dns-query",
          bootstrap: ["9.9.9.9", "149.112.112.112"],
        },
        { protocol: "dot", name: "dns.google", addresses: ["8.8.8.8"], port: 853, spki: [] },
      ],
      servers: ["192.0.2.53"],
      isp: false,
      strict: true,
    };
    config.services.ddns = [CF, { ...CF, id: "v6", ipv6: true, source: "web" }];
    services = {
      ddns: [{ id: "home", address: "203.0.113.7", updated_at: 0, running: true }],
      doh_instances: 1,
      dot_instances: 1,
      wireguard: null,
      tailscale: null,
      ssh: null,
      blocklist: NO_BLOCKLIST,
    };
    issues = [
      { path: "services/ddns/1/ipv6", severity: "warning", message: "The WAN has IPv6 off" },
    ];
    await renderNetworks("viewer");
    const upstream = await screen.findByRole("region", { name: "Upstream DNS" });
    expect(within(upstream).getByText("Quad9")).toBeInTheDocument();
    expect(
      await within(upstream).findByText("1 DoH proxy running on the gateway"),
    ).toBeInTheDocument();
    expect(within(upstream).getByText("Google")).toBeInTheDocument();
    expect(within(upstream).getByText("dns.google · 8.8.8.8")).toBeInTheDocument();
    expect(within(upstream).getByText("Stubby (DoT) running on the gateway")).toBeInTheDocument();
    expect(within(upstream).getByText("Encrypted only")).toBeInTheDocument();
    expect(within(upstream).getByText("Not asked: encrypted only")).toBeInTheDocument();
    expect(within(upstream).queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();

    const ddns = screen.getByRole("region", { name: "Dynamic DNS" });
    const rows = within(ddns).getAllByRole("listitem");
    expect(
      within(rows[0] as HTMLElement).getByText("Cloudflare · A · WAN address"),
    ).toBeInTheDocument();
    expect(
      await within(rows[0] as HTMLElement).findByText("Registered 203.0.113.7"),
    ).toBeInTheDocument();
    expect(
      within(rows[1] as HTMLElement).getByText("Cloudflare · AAAA · Web echo"),
    ).toBeInTheDocument();
    expect(
      within(rows[1] as HTMLElement).getByText("No report from the gateway yet"),
    ).toBeInTheDocument();
    expect(within(rows[1] as HTMLElement).getByText("The WAN has IPv6 off")).toBeInTheDocument();
    expect(within(ddns).queryByRole("button", { name: "Add a name" })).not.toBeInTheDocument();

    expect(screen.getByRole("checkbox", { name: "Serve time to the networks" })).toBeDisabled();
  });

  it("layers DoT and DoH resolvers before the ISP's, in the order chosen", async () => {
    const user = userEvent.setup();
    await renderNetworks();
    const card = await screen.findByRole("region", { name: "Upstream DNS" });
    expect(
      within(card).getByText("The ISP's servers, from the WAN connection"),
    ).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Edit" }));
    expect(within(card).getByRole("checkbox", { name: "Encrypted only" })).toBeDisabled();
    // DNS over TLS is the protocol offered first.
    await user.click(within(card).getByRole("button", { name: "Quad9" }));
    expect(within(card).getByRole("button", { name: "Quad9" })).toBeDisabled();
    await user.click(within(card).getByRole("button", { name: "DNS over HTTPS" }));
    expect(within(card).getByRole("button", { name: "Quad9" })).toBeEnabled();
    await user.click(within(card).getByRole("button", { name: "NextDNS" }));
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(
      within(card).getByText("Enter your NextDNS profile ID: letters and digits"),
    ).toBeInTheDocument();
    expect(put()).toBeUndefined();

    const nextdns = within(card).getByRole("group", { name: "NextDNS (DoH)" });
    await user.type(within(nextdns).getByLabelText("NextDNS profile ID"), "abc123");
    await user.click(within(card).getByRole("button", { name: "Move NextDNS (DoH) up" }));
    expect(within(card).getByRole("button", { name: "Move NextDNS (DoH) up" })).toBeDisabled();
    await user.click(within(card).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(put()).toBeDefined());
    expect(put().message).toBe("Upstream DNS: NextDNS (DoH), Quad9 (DoT) → the ISP's");
    expect(put().config.services.upstream).toEqual({
      encrypted: [
        {
          protocol: "doh",
          url: "https://dns.nextdns.io/abc123",
          bootstrap: ["45.90.28.0", "45.90.30.0"],
        },
        {
          protocol: "dot",
          name: "dns.quad9.net",
          addresses: ["9.9.9.9", "149.112.112.112"],
          port: 853,
          spki: [],
        },
      ],
      servers: [],
      isp: true,
      strict: false,
    });
    expect(await within(card).findByText(/Saved as revision 3/)).toBeInTheDocument();
  });

  it("keeps queries encrypted only, with the plain layers kept", async () => {
    const user = userEvent.setup();
    await renderNetworks();
    const card = await screen.findByRole("region", { name: "Upstream DNS" });
    await user.click(within(card).getByRole("button", { name: "Edit" }));
    await user.click(within(card).getByRole("button", { name: "Add a server" }));
    await user.type(within(card).getByLabelText("Server 1"), "9.9.9.9");
    await user.click(within(card).getByRole("button", { name: "Custom" }));
    const row = within(card).getByRole("group", { name: "Resolver 1 (DoT)" });
    await user.type(within(row).getByLabelText("Certificate name"), "dot.example");
    await user.type(within(row).getByLabelText("Addresses"), "192.0.2.53");
    await user.clear(within(row).getByLabelText("Port"));
    await user.type(within(row).getByLabelText("Port"), "8853");
    await user.click(within(card).getByRole("checkbox", { name: "Encrypted only" }));
    expect(within(card).getAllByText("Not asked: encrypted only")).toHaveLength(2);
    await user.click(within(card).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(put()).toBeDefined());
    expect(put().message).toBe("Upstream DNS: dot.example (DoT) only");
    expect(put().config.services.upstream).toEqual({
      encrypted: [
        { protocol: "dot", name: "dot.example", addresses: ["192.0.2.53"], port: 8853, spki: [] },
      ],
      servers: ["9.9.9.9"],
      isp: true,
      strict: true,
    });
  });

  it("shows the server's refusal on the custom server it concerns", async () => {
    const user = userEvent.setup();
    await renderNetworks();
    const card = await screen.findByRole("region", { name: "Upstream DNS" });
    await user.click(within(card).getByRole("button", { name: "Edit" }));
    await user.click(within(card).getByRole("checkbox", { name: "Then the ISP's servers" }));
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(
      within(card).getByText(
        "Choose where queries go: encrypted resolvers, DNS servers or the ISP's",
      ),
    ).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Add a server" }));
    await user.type(within(card).getByLabelText("Server 1"), "nope");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(within(card).getByText("Not an IP address")).toBeInTheDocument();

    await user.clear(within(card).getByLabelText("Server 1"));
    await user.type(within(card).getByLabelText("Server 1"), "127.0.0.1");
    refusal = { path: "services/upstream/servers/0", message: "127.0.0.1 is not a DNS server" };
    await user.click(within(card).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText("127.0.0.1 is not a DNS server")).toBeInTheDocument();
  });

  it("adds a Cloudflare name with its token as a secret", async () => {
    const user = userEvent.setup();
    await renderNetworks();
    const card = await screen.findByRole("region", { name: "Dynamic DNS" });
    expect(within(card).getByText("No DDNS names.")).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Add a name" }));
    const dialog = await screen.findByRole("dialog", { name: "Add a DDNS name" });
    await user.type(within(dialog).getByLabelText("Hostname"), "home.example.com");
    await user.selectOptions(within(dialog).getByLabelText("Provider"), "duckdns");
    await user.selectOptions(within(dialog).getByLabelText("Provider"), "cloudflare");
    expect(within(dialog).getByLabelText("Zone")).toHaveValue("example.com");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(within(dialog).getByText("Enter the API token")).toBeInTheDocument();

    await user.type(within(dialog).getByLabelText("API token"), "cf-token");
    await user.click(within(dialog).getByRole("button", { name: "IPv6 (AAAA)" }));
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(put()).toBeDefined());
    expect(put().message).toBe("Add DDNS name home.example.com");
    expect(put().secrets).toEqual({ "services/ddns/home/secret": "cf-token" });
    expect(put().config.services.ddns).toEqual([{ ...CF, ipv6: true, secret: null }]);
    expect(await within(dialog).findByText(/Saved as revision 3/)).toBeInTheDocument();
  });

  it("deletes a name", async () => {
    const user = userEvent.setup();
    config.services.ddns = [CF];
    await renderNetworks();
    const card = await screen.findByRole("region", { name: "Dynamic DNS" });
    await user.click(within(card).getByRole("button", { name: "Edit home.example.com" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit home.example.com" });
    expect(
      within(dialog).getByText("Stored in the vault. Leave empty to keep it."),
    ).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await user.click(
      within(within(dialog).getByRole("alert")).getByRole("button", { name: "Delete" }),
    );
    await waitFor(() => expect(put()).toBeDefined());
    expect(put().message).toBe("Delete DDNS name home.example.com");
    expect(put().config.services.ddns).toEqual([]);
  });

  it("turns the NTP server off", async () => {
    const user = userEvent.setup();
    await renderNetworks();
    const card = await screen.findByRole("region", { name: "Time server" });
    await user.click(within(card).getByRole("checkbox", { name: "Serve time to the networks" }));
    await user.click(within(card).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(put()).toBeDefined());
    expect(put().message).toBe("Turn the NTP server off");
    expect(put().config.services.ntp_server).toBe(false);
  });
});
