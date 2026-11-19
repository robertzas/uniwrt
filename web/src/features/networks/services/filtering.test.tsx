import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BlocklistsView } from "../../../api/gen/BlocklistsView";
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
let lists: BlocklistsView = blocklistsView();
/** A refusal the next save answers with, once. */
let refusal: { path: string; message: string } | undefined;

const SYNCED: Partial<BlocklistsView> = {
  enabled: true,
  lists: [
    {
      id: "hagezi_normal",
      name: "HaGeZi Multi Normal",
      url: "https://example.com/hagezi-normal.txt",
      checked_at: 0,
      fetched_at: 0,
      domains: 301_234,
      error: null,
    },
    {
      id: null,
      name: "Mine",
      url: "https://example.com/mine.txt",
      checked_at: null,
      fetched_at: null,
      domains: null,
      error: "HTTP 404",
    },
  ],
  compiled: { sha256: "ab", domains: 250_000, cut: 51_234, capacity: 250_000, at: 0 },
  sync: { state: "synced", sha256: "ab", domains: 250_000, pushed_at: 0, error: null },
  refreshed_at: 0,
};

beforeEach(() => {
  calls = [];
  head = 2;
  config = siteConfig();
  lists = blocklistsView();
  refusal = undefined;
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
      if (url === "/api/v1/services/blocklists") return Response.json(lists);
      if (url === "/api/v1/services/blocklists/refresh") {
        lists = { ...lists, refreshing: true };
        return Response.json(lists, { status: 202 });
      }
      if (url === "/api/v1/nodes/1") return Response.json(detail(facts(1)));
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderFiltering(role: "admin" | "viewer" = "admin") {
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
  return screen.findByRole("region", { name: "DNS filtering" });
}

const put = () =>
  calls.filter((c) => c.method === "PUT" && c.url === "/api/v1/site").at(-1)?.body as SiteSave;

describe("DNS filtering", () => {
  it("shows viewers the gateway's copy, the lists and what was left out", async () => {
    config.services.blocking = { ...config.services.blocking, enabled: true, allow: ["a.example"] };
    lists = blocklistsView(SYNCED);
    const card = await renderFiltering("viewer");
    expect(await within(card).findByText("The gateway blocks 250,000 domains")).toBeInTheDocument();
    expect(
      within(card).getByText(/51,234 domains left out: the gateway holds up to 250,000/),
    ).toBeInTheDocument();
    const rows = within(within(card).getByRole("list", { name: "Lists" })).getAllByRole("listitem");
    expect(within(rows[0] as HTMLElement).getByText(/^301,234 domains · updated/)).toBeVisible();
    expect(within(rows[1] as HTMLElement).getByText("Custom")).toBeInTheDocument();
    expect(within(rows[1] as HTMLElement).getByText("Download failed: HTTP 404")).toBeVisible();
    expect(within(card).queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Refresh lists" })).not.toBeInTheDocument();
  });

  it("says when the saved settings still wait for an apply", async () => {
    config.services.blocking = { ...config.services.blocking, enabled: true };
    const card = await renderFiltering();
    expect(await within(card).findByText(/Saved, not applied yet/)).toBeInTheDocument();
  });

  it("turns filtering on with another list, a custom one, a block and a bypass", async () => {
    const user = userEvent.setup();
    const card = await renderFiltering();
    expect(await within(card).findByText(/^Off: every name resolves unfiltered/)).toBeVisible();
    await user.click(await within(card).findByRole("button", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: "DNS filtering" });
    await user.click(within(dialog).getByRole("checkbox", { name: "Filter DNS on every network" }));
    await user.click(within(dialog).getByRole("checkbox", { name: "OISD Big" }));
    expect(within(dialog).getByText(/About 553,000 domains chosen/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Add a list" }));
    await user.type(within(dialog).getByLabelText("Name of list 1"), "Mine");
    await user.type(within(dialog).getByLabelText("URL of list 1"), "https://example.com/mine.txt");
    await user.type(within(dialog).getByLabelText("Blocked domains"), "Ads.Example.com");
    const add = within(dialog).getByRole("combobox", { name: "Add a client" });
    await user.type(add, "not a mac{Enter}");
    expect(within(dialog).getByText(/Pick a client, or type its MAC/)).toBeInTheDocument();
    await user.clear(add);
    await user.type(add, "3C-22-FB-12-34-56{Enter}");
    expect(within(dialog).getByText("3c:22:fb:12:34:56")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    await within(dialog).findByText(/Saved as revision 3/);
    expect(put().message).toBe("Update DNS filtering");
    expect(put().config.services.blocking).toEqual({
      enabled: true,
      lists: ["hagezi_normal", "urlhaus", "oisd_big"],
      custom: [{ name: "Mine", url: "https://example.com/mine.txt" }],
      allow: [],
      deny: ["ads.example.com"],
      bypass: ["3c:22:fb:12:34:56"],
      stats: true,
    });
  });

  it("checks fields before saving and shows the server's issues on them", async () => {
    const user = userEvent.setup();
    const card = await renderFiltering();
    await user.click(await within(card).findByRole("button", { name: "Edit" }));
    const dialog = await screen.findByRole("dialog", { name: "DNS filtering" });
    await user.type(within(dialog).getByLabelText("Allowed domains"), "x.example.com");
    await user.type(within(dialog).getByLabelText("Blocked domains"), "x.example.com");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(within(dialog).getByText("x.example.com is allowed too")).toBeInTheDocument();
    expect(put()).toBeUndefined();

    await user.clear(within(dialog).getByLabelText("Blocked domains"));
    refusal = { path: "services/blocking/allow/0", message: "Allowed by the server? No." };
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByText("Allowed by the server? No.")).toBeInTheDocument();
  });

  it("downloads the lists again on demand", async () => {
    const user = userEvent.setup();
    config.services.blocking = { ...config.services.blocking, enabled: true };
    lists = blocklistsView(SYNCED);
    const card = await renderFiltering();
    await user.click(await within(card).findByRole("button", { name: "Refresh lists" }));
    await waitFor(() =>
      expect(calls.some((c) => c.url === "/api/v1/services/blocklists/refresh")).toBe(true),
    );
    expect(await within(card).findByText("Downloading the lists…")).toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Refresh lists" })).toBeDisabled();
  });
});
