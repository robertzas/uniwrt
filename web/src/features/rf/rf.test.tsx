import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MetricsResponse } from "../../api/gen/MetricsResponse";
import type { RfView } from "../../api/gen/RfView";
import { createAppRouter } from "../../app/routes";
import { summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { channel, rfView } from "../../test/rf";

let calls: { method: string; url: string; body: unknown }[] = [];

beforeEach(() => {
  calls = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
      calls.push({ method, url, body });
      if (url.startsWith("/api/v1/metrics?")) {
        const empty: MetricsResponse = {
          entity: "site",
          from: 0,
          to: 1,
          resolution: "raw",
          series: [],
        };
        return Response.json(empty);
      }
      if (method === "POST" && url === "/api/v1/rf/scans") {
        return Response.json({ queued: [rfView().radios[1]] }, { status: 202 });
      }
      if (method === "PUT" && url === "/api/v1/rf/settings") return Response.json(body);
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderAt(path: string, view: RfView = rfView(), role?: "viewer") {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory(path);
  const Router = createAppRouter(history);
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router />
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "Gateway", { role: "gateway" }), summary(2, "Attic AP")]);
  await waitFor(() => expect(ws.sent).toContainEqual({ sub: expect.arrayContaining(["rf"]) }));
  ws.receive({ type: "rf", topic: "rf", view });
  return { ws, history };
}

const region = (name: string) => screen.findByRole("region", { name });

describe("the RF page", () => {
  it("shows 5 GHz first: the spectrum, the radios, the channels, the matrix and the networks", async () => {
    const view = rfView();
    view.report.bands[1]?.channels.push(channel(165));
    await renderAt("/insights/rf", view);
    const spectrum = await region("Spectrum");
    expect(within(spectrum).getByRole("img", { name: /Networks heard on 5 GHz/ })).toBeVisible();
    expect(
      within(spectrum).getByRole("button", { name: "Neighbor-5G · 36 · -58 dBm" }),
    ).toBeVisible();

    const radios = await region("Your radios");
    const cards = within(radios)
      .getAllByRole("listitem")
      .filter((li) => li.querySelector("a"));
    expect(cards).toHaveLength(2);
    expect(cards[0]).toHaveTextContent(/Gateway.*Channel 36 · 80 MHz/);
    expect(cards[0]).toHaveTextContent("Crowded");
    expect(cards[0]).toHaveTextContent(/Neighbor-5G · same channel/);
    expect(cards[0]).toHaveTextContent(/Home · overlaps 25%/);
    expect(cards[0]).toHaveTextContent(/Scanned 2h.* ago · Next in 2h/);
    // Not scanned yet: no crowding verdict, and its state says why it waits.
    expect(cards[1]).not.toHaveTextContent("Clean channel");
    expect(cards[1]).toHaveTextContent("Not scanned yet · Waiting for a quiet moment");

    const channels = await region("Channels");
    // The radar column, and channel 52's DFS state.
    expect(within(channels).getAllByText("Radar seen")).toHaveLength(2);
    expect(within(channels).getByText(/2 radar hits, last 3h.* ago/)).toBeVisible();
    // Channels in use only, until all are asked for.
    expect(within(channels).queryByText("165")).toBeNull();
    await userEvent.setup().click(within(channels).getByRole("button", { name: "All" }));
    expect(await within(channels).findByText("165")).toBeVisible();

    const matrix = await region("How your devices hear each other");
    const cells = within(matrix).getAllByRole("cell");
    expect(cells.some((c) => c.textContent === "-61")).toBe(true);
    expect(within(matrix).getByRole("img", { name: "Live, over the mesh" })).toBeVisible();

    const networks = await region("Networks around");
    expect(within(networks).getByText("Netgear")).toBeVisible();
    expect(within(networks).getByText("Looks like yours")).toBeVisible();
    expect(within(networks).getByText("Attic AP")).toBeVisible();
    await userEvent.setup().click(within(networks).getByRole("button", { name: "Neighbors" }));
    await waitFor(() => expect(within(networks).queryByText("Attic AP")).toBeNull());
  });

  it("switches bands through the URL", async () => {
    const user = userEvent.setup();
    const { history } = await renderAt("/insights/rf");
    await region("Spectrum");
    await user.click(screen.getByRole("button", { name: "2.4 GHz" }));
    await waitFor(() => expect(history.get()).toBe("/insights/rf?band=2g"));
    expect(await screen.findByRole("button", { name: "Cafe · 6 · -72 dBm" })).toBeInTheDocument();
    expect(screen.queryByText("Neighbor-5G")).toBeNull();
  });

  it("lets admins scan a radio or all, and set the background schedule", async () => {
    const user = userEvent.setup();
    await renderAt("/insights/rf");
    await user.click(await screen.findByRole("button", { name: "Scan Gateway radio1 now" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "POST",
        url: "/api/v1/rf/scans",
        body: { node: 1, radio: "radio1" },
      }),
    );

    await user.click(screen.getByRole("button", { name: "Scan now" }));
    await waitFor(() =>
      expect(calls.filter((c) => c.url === "/api/v1/rf/scans").at(-1)?.body).toEqual({}),
    );

    await user.selectOptions(screen.getByRole("combobox", { name: "Interval" }), "8");
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "PUT",
        url: "/api/v1/rf/settings",
        body: { scanning: true, interval_h: 8 },
      }),
    );
    await user.click(screen.getByRole("button", { name: "Off" }));
    await waitFor(() =>
      expect(calls.filter((c) => c.method === "PUT").at(-1)?.body).toEqual({
        scanning: false,
        interval_h: 4,
      }),
    );
  });

  it("shows viewers the schedule without the controls", async () => {
    await renderAt("/insights/rf", rfView(), "viewer");
    expect(await screen.findByText("Background scans: Every 4 h")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Scan now" })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Scan .* now$/ })).toBeNull();
  });

  it("says so when no radio reports yet", async () => {
    const view = rfView();
    view.report.bands = [];
    await renderAt("/insights/rf", view);
    expect(await screen.findByText("No radios yet.")).toBeVisible();
  });
});
