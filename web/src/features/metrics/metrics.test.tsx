import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SeriesInfo } from "../../api/gen/SeriesInfo";
import { createAppRouter } from "../../app/routes";
import { client } from "../../test/clients";
import { summary } from "../../test/fixtures";
import { metricsAnswer } from "../../test/metrics";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";

const MAC = "f0:18:98:00:00:01";
let listing: SeriesInfo[] = [];
let calls: string[] = [];

beforeEach(() => {
  calls = [];
  listing = [
    { entity: "site", metrics: ["down_bps", "up_bps", "clients", "nodes_online"] },
    { entity: "node:1", metrics: ["cpu", "memory", "load"] },
    { entity: "node:1/radio:radio0", metrics: ["busy", "noise", "stations"] },
    { entity: `client:${MAC}`, metrics: ["signal", "down_bps", "up_bps"] },
  ];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      calls.push(url);
      if (url === "/api/v1/metrics/series") return Response.json(listing);
      if (url.startsWith("/api/v1/metrics?")) return Response.json(metricsAnswer(url));
      if (url === "/api/v1/clients") return Response.json([client(MAC)]);
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

function renderAt(path: string) {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory(path);
  const Router = createAppRouter(history);
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket}>
        <Router />
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "Gateway", { role: "gateway" })]);
  return history;
}

const query = (url: string) => new URL(url, "http://test").searchParams;

describe("the Metrics explorer", () => {
  it("charts an entity's metrics, one chart per unit, as the URL says", async () => {
    const user = userEvent.setup();
    const history = renderAt("/insights/metrics?entity=node%3A1&range=7d");
    expect(await screen.findByRole("heading", { name: "Gateway", level: 2 })).toBeVisible();
    const list = screen.getByRole("navigation", { name: "Series" });
    expect(within(list).getByRole("button", { name: /^Gateway/ })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(within(list).getByText("Whole network")).toBeVisible();
    expect(within(list).getByText("radio0")).toBeVisible();

    expect(await screen.findByRole("img", { name: "CPU · Memory" })).toBeVisible();
    expect(screen.getByRole("img", { name: "Load" })).toBeVisible();
    const read = calls.find((url) => url.startsWith("/api/v1/metrics?")) ?? "";
    expect(query(read).get("entity")).toBe("node:1");
    expect(query(read).get("metrics")).toBe("cpu,memory,load");
    expect(Number(query(read).get("to")) - Number(query(read).get("from"))).toBe(7 * 86_400_000);
    // The legend shows the latest values.
    expect(await screen.findAllByText("42 %")).toHaveLength(2);

    await user.click(screen.getByRole("button", { name: "Load" }));
    await waitFor(() => expect(screen.queryByRole("img", { name: "Load" })).toBeNull());
    await waitFor(() => expect(query(history.get()).get("metrics")).toBe("cpu,memory"));
    // The last metric stays.
    await user.click(screen.getByRole("button", { name: "CPU" }));
    await user.click(screen.getByRole("button", { name: "Memory" }));
    expect(screen.getByRole("button", { name: "Memory" })).toHaveAttribute("aria-pressed", "true");

    await user.click(screen.getByRole("button", { name: "Last hour" }));
    await waitFor(() => expect(history.get()).toContain("range=1h"));
  });

  it("names clients and filters the list", async () => {
    const user = userEvent.setup();
    const history = renderAt("/insights/metrics");
    // The site by default.
    expect(await screen.findByRole("heading", { name: "Whole network", level: 2 })).toBeVisible();
    const list = screen.getByRole("navigation", { name: "Series" });
    await user.type(screen.getByLabelText("Filter"), "iphone");
    const row = await within(list).findByRole("button", { name: /Annas-iPhone/ });
    expect(within(list).queryByText("Whole network")).toBeNull();
    await user.click(row);
    expect(await screen.findByRole("heading", { name: "Annas-iPhone", level: 2 })).toBeVisible();
    expect(history.get()).toContain(`entity=client%3A${encodeURIComponent(MAC)}`);
    expect(screen.getByRole("link", { name: /Open its page/ })).toHaveAttribute(
      "href",
      `/clients/${MAC}`,
    );
    expect(await screen.findByRole("img", { name: "Signal" })).toBeVisible();
    expect(screen.getByRole("img", { name: "Download · Upload" })).toBeVisible();
  });

  it("says when nothing is kept yet", async () => {
    listing = [];
    renderAt("/insights/metrics");
    expect(await screen.findByText("No metrics yet")).toBeVisible();
  });
});
