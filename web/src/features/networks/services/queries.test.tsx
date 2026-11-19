import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DnsQueryPage } from "../../../api/gen/DnsQueryPage";
import type { DnsStats } from "../../../api/gen/DnsStats";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import type { SiteSave } from "../../../api/gen/SiteSave";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { blocklistsView, dnsQuery, dnsStats } from "../../../test/blocklists";
import { detail, facts, summary } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import { revisionMeta, siteConfig, siteView } from "../../../test/site";

let calls: { method: string; url: string; body: unknown }[] = [];
let head = 2;
let config: SiteConfig = siteConfig();
let stats: DnsStats = dnsStats();
/** The query pages by their `before` cursor ("" for the first). */
let pages: Record<string, DnsQueryPage> = {};

const BUSY: Partial<DnsStats> = {
  queries: 1_200,
  blocked: 150,
  hours: [{ hour: Date.now() - 3_600_000, queries: 1_200, blocked: 150 }],
  clients: [
    {
      client: "aa:00:00:00:00:01",
      mac: "aa:00:00:00:00:01",
      name: "laptop",
      queries: 900,
      blocked: 100,
    },
    { client: "fd00::99", mac: null, name: null, queries: 300, blocked: 50 },
  ],
  top_domains: [{ domain: "example.com", queries: 400 }],
  top_blocked: [{ domain: "ads.tracker.example", queries: 120 }],
};

beforeEach(() => {
  calls = [];
  head = 2;
  config = siteConfig();
  config.services.blocking = { ...config.services.blocking, enabled: true };
  stats = dnsStats(BUSY);
  pages = {
    "": {
      queries: [dnsQuery({ domain: "ads.tracker.example", outcome: "blocked" })],
      next: "1700:7",
    },
    "1700:7": { queries: [dnsQuery({ domain: "old.example", at: 1_700 })], next: null },
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site" && method === "GET") return Response.json(siteView(head, config));
      if (url === "/api/v1/site" && method === "PUT") {
        head += 1;
        config = (body as SiteSave).config;
        return Response.json(siteView(head, config));
      }
      if (url === "/api/v1/services/blocklists") {
        return Response.json(blocklistsView({ enabled: true }));
      }
      if (url.startsWith("/api/v1/services/dns/stats")) return Response.json(stats);
      if (url.startsWith("/api/v1/services/dns/queries")) {
        const before = new URL(url, "http://x").searchParams.get("before") ?? "";
        return Response.json(pages[before] ?? { queries: [], next: null });
      }
      if (url === "/api/v1/nodes/1") return Response.json(detail(facts(1)));
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

/** The test socket of the last mount: the controller's side of the WebSocket. */
let server: ReturnType<typeof openTestSocket>["ws"];

function mount(role: "admin" | "viewer") {
  const { socket, ws } = openTestSocket();
  server = ws;
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
}

async function renderQueries(role: "admin" | "viewer" = "admin") {
  mount(role);
  return screen.findByRole("region", { name: "DNS queries" });
}

const queryCalls = () => calls.filter((c) => c.url.startsWith("/api/v1/services/dns/queries"));

describe("DNS query statistics", () => {
  it("shows viewers the totals, the chart, the clients and the top domains", async () => {
    const card = await renderQueries("viewer");
    expect(await within(card).findByText("1,200")).toBeInTheDocument();
    expect(within(card).getByText(`${(12.5).toLocaleString(undefined)} %`)).toBeInTheDocument();
    expect(
      within(card).getByRole("img", { name: /Queries over the last 24 hours: 1,200/ }),
    ).toBeInTheDocument();
    expect(within(card).getByRole("link", { name: "laptop" })).toHaveAttribute(
      "href",
      "/clients/aa%3A00%3A00%3A00%3A00%3A01",
    );
    expect(within(card).getByText("fd00::99")).toBeInTheDocument();
    const blocked = within(card).getByRole("region", { name: "Top blocked" });
    expect(within(blocked).getByText("ads.tracker.example")).toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: /^Allow / })).not.toBeInTheDocument();
  });

  it("reads another range", async () => {
    const card = await renderQueries();
    await within(card).findByText("1,200");
    await userEvent.click(within(card).getByRole("button", { name: "7 days" }));
    await waitFor(() =>
      expect(calls.some((c) => c.url === "/api/v1/services/dns/stats?range=7d")).toBe(true),
    );
  });

  it("allows a blocked domain as the next revision", async () => {
    config.services.blocking.deny = ["other.example"];
    const card = await renderQueries();
    await userEvent.click(
      await within(card).findByRole("button", { name: "Allow ads.tracker.example" }),
    );
    await within(card).findByText(/Saved as revision 3/);
    const save = calls.find((c) => c.method === "PUT")?.body as SiteSave;
    expect(save.message).toBe("Allow ads.tracker.example in DNS filtering");
    expect(save.config.services.blocking.allow).toEqual(["ads.tracker.example"]);
    expect(save.config.services.blocking.deny).toEqual(["other.example"]);
    // The new head reaches the page, which reads the revision that allows it.
    server.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
    expect(await within(card).findAllByText("Allowed")).not.toHaveLength(0);
  });

  it("opens the query log by client, filters it and reads older pages", async () => {
    const card = await renderQueries();
    await userEvent.click(
      await within(card).findByRole("button", { name: "Show the queries of laptop" }),
    );
    const log = await screen.findByRole("dialog", { name: "DNS query log" });
    const table = await within(log).findByRole("table", { name: "DNS queries" });
    expect(within(table).getByText("ads.tracker.example")).toBeInTheDocument();
    expect(within(table).getByText("laptop")).toBeInTheDocument();
    expect(queryCalls()[0]?.url).toBe(
      "/api/v1/services/dns/queries?client=aa%3A00%3A00%3A00%3A00%3A01&limit=200",
    );

    await userEvent.click(within(log).getByRole("button", { name: "Load older" }));
    expect(await within(table).findByText("old.example")).toBeInTheDocument();
    expect(queryCalls().at(-1)?.url).toContain("before=1700%3A7");
    expect(within(log).getByText("No older queries.")).toBeInTheDocument();

    await userEvent.selectOptions(within(log).getByLabelText("Outcome"), "blocked");
    await waitFor(() => expect(queryCalls().at(-1)?.url).toContain("outcome=blocked"));
    await userEvent.type(within(log).getByLabelText("Domain contains"), "ads");
    await waitFor(() => expect(queryCalls().at(-1)?.url).toContain("domain=ads"));
  });

  it("isn't shown while statistics are off", async () => {
    config.services.blocking.stats = false;
    mount("admin");
    await screen.findByRole("region", { name: "DNS filtering" });
    expect(screen.queryByRole("region", { name: "DNS queries" })).not.toBeInTheDocument();
    expect(calls.some((c) => c.url.startsWith("/api/v1/services/dns/stats"))).toBe(false);
  });
});
