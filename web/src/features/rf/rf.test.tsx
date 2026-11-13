import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { MetricsResponse } from "../../api/gen/MetricsResponse";
import type { Plan } from "../../api/gen/Plan";
import type { RfView } from "../../api/gen/RfView";
import type { SiteView } from "../../api/gen/SiteView";
import { createAppRouter } from "../../app/routes";
import { summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { channel, rfPlan, rfView } from "../../test/rf";
import { siteView } from "../../test/site";
import { clockText, nightlyTimes } from "./Nightly";
import { withBandPlan } from "./planned";

let calls: { method: string; url: string; body: unknown }[] = [];
let plan: Plan;

beforeEach(() => {
  calls = [];
  plan = rfPlan();
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
      if (url === "/api/v1/rf/plan") return Response.json(plan);
      if (url === "/api/v1/site" && method === "PUT") {
        const save = body as { config: SiteView["config"] };
        return Response.json(siteView(8, save.config));
      }
      if (url === "/api/v1/site") return Response.json(siteView(7));
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
        body: { scanning: true, interval_h: 8, dfs: "all", nightly: true, nightly_at_min: 210 },
      }),
    );
    await user.click(screen.getByRole("button", { name: "Off" }));
    await waitFor(() =>
      expect(calls.filter((c) => c.method === "PUT").at(-1)?.body).toEqual({
        scanning: false,
        interval_h: 4,
        dfs: "all",
        nightly: true,
        nightly_at_min: 210,
      }),
    );
  });

  it("previews the channel plan on demand, and lets admins set the DFS policy", async () => {
    const user = userEvent.setup();
    await renderAt("/insights/rf");
    const card = await region("Channel plan");
    // Planned only when asked.
    expect(calls.some((c) => c.url === "/api/v1/rf/plan")).toBe(false);
    await user.click(within(card).getByRole("button", { name: "Preview plan" }));
    const table = await within(card).findByRole("table", { name: "Planned settings" });
    expect(within(card).getByText("Cost 1.60 → 0.50")).toBeVisible();
    expect(within(card).getByText("69 % better")).toBeVisible();
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows[0]).toHaveTextContent(/Gateway.*36 · 80 MHz · 23 dBm.*149 · 80 MHz · 20 dBm/);
    expect(rows[0]).toHaveTextContent("1.40 → 0.30");
    expect(rows[0]).toHaveTextContent("neighbors 0.20 · move 0.10");
    expect(rows[0]).toHaveTextContent("Quieter: the nearest device hears it at -58 dBm");
    expect(rows[1]).toHaveTextContent(/Attic AP.*Unchanged.*Observe mode: kept as it runs/);
    expect(within(card).getByText(/Left out .*: Attic AP radio2/)).toBeVisible();

    await user.click(within(card).getByRole("button", { name: "No weather radar" }));
    await waitFor(() =>
      expect(calls.filter((c) => c.method === "PUT").at(-1)?.body).toEqual({
        scanning: true,
        interval_h: 4,
        dfs: "non_weather",
        nightly: true,
        nightly_at_min: 210,
      }),
    );
    // Planned again under the new policy.
    await waitFor(() => expect(calls.filter((c) => c.url === "/api/v1/rf/plan")).toHaveLength(2));
    expect(within(card).queryByText(/Every radio here is in observe mode/)).toBeNull();
  });

  it("lets admins save the band's plan as the next revision, to review and apply", async () => {
    const user = userEvent.setup();
    await renderAt("/insights/rf");
    const card = await region("Channel plan");
    await user.click(within(card).getByRole("button", { name: "Preview plan" }));
    await user.click(await within(card).findByRole("button", { name: "Use this plan" }));
    expect(await within(card).findByText(/Saved as revision 8/)).toBeVisible();
    expect(within(card).getByRole("link", { name: /Review/ })).toHaveAttribute("href", "/changes");
    const put = calls.find((c) => c.method === "PUT" && c.url === "/api/v1/site");
    expect(put?.body).toMatchObject({
      base: 7,
      message: "RF plan for 5 GHz: 1 radio changed",
      config: {
        radio: {
          plan: {
            // The observed radio isn't stored: it keeps what it runs.
            radios: [
              {
                node: 1,
                radio: "radio1",
                band: "5g",
                channel: 149,
                width: 80,
                tx_power: { dbm: 20 },
              },
            ],
            mesh: null,
          },
        },
      },
    });
  });

  it("says why nothing moves when every radio is in observe mode", async () => {
    for (const radio of plan.bands[1]?.radios ?? []) radio.reasons = [{ kind: "observed" }];
    const user = userEvent.setup();
    await renderAt("/insights/rf");
    const card = await region("Channel plan");
    await user.click(within(card).getByRole("button", { name: "Preview plan" }));
    expect(await within(card).findByText(/Every radio here is in observe mode/)).toBeVisible();
    expect(within(card).queryByRole("button", { name: "Use this plan" })).toBeNull();
  });

  it("shows viewers the schedule without the controls", async () => {
    await renderAt("/insights/rf", rfView(), "viewer");
    expect(await screen.findByText("Background scans: Every 4 h")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Scan now" })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Scan .* now$/ })).toBeNull();
    expect(screen.getByText("DFS channels: All")).toBeVisible();
    expect(screen.queryByRole("button", { name: "No weather radar" })).toBeNull();
    expect(screen.getByText("Optimizes nightly at 03:30")).toBeVisible();
    expect(screen.queryByRole("checkbox", { name: "Optimize nightly" })).toBeNull();
  });

  it("lets admins turn the nightly optimization off or start it at another time", async () => {
    const user = userEvent.setup();
    await renderAt("/insights/rf");
    const card = await region("Channel plan");
    const nightly = within(card).getByRole("checkbox", { name: "Optimize nightly" });
    expect(nightly).toBeChecked();
    const time = within(card).getByRole("combobox", { name: "Starts at" });
    expect(time).toHaveValue("210");
    await user.selectOptions(time, "120");
    await waitFor(() =>
      expect(calls.filter((c) => c.method === "PUT").at(-1)?.body).toMatchObject({
        nightly: true,
        nightly_at_min: 120,
      }),
    );
    await user.click(nightly);
    await waitFor(() =>
      expect(calls.filter((c) => c.method === "PUT").at(-1)?.body).toMatchObject({
        nightly: false,
        nightly_at_min: 210,
      }),
    );
  });

  it("says so when no radio reports yet", async () => {
    const view = rfView();
    view.report.bands = [];
    await renderAt("/insights/rf", view);
    expect(await screen.findByText("No radios yet.")).toBeVisible();
  });
});

describe("the nightly start times", () => {
  it("offer every half hour, with an odd time set elsewhere in order", () => {
    expect(clockText(210)).toBe("03:30");
    expect(clockText(0)).toBe("00:00");
    const times = nightlyTimes(215);
    expect(times).toHaveLength(49);
    expect(times.slice(6, 9)).toEqual([180, 210, 215]);
    expect(nightlyTimes(210)).toHaveLength(48);
  });
});

describe("withBandPlan", () => {
  it("replaces the band's radios, keeps the others' and takes the mesh's channel from its group", () => {
    const config = siteView().config;
    config.radio.plan = {
      radios: [
        { node: 1, radio: "radio0", band: "2g", channel: 6, width: 20, tx_power: "auto" },
        { node: 3, radio: "radio1", band: "5g", channel: 36, width: 80, tx_power: "auto" },
      ],
      mesh: { band: "5g", channel: 36, width: 80 },
    };
    const band = rfPlan().bands[1];
    if (!band) throw new Error("no 5 GHz plan");
    const kept = withBandPlan(config, band);
    // No mesh radio in this plan: the stored mesh stays.
    expect(kept.radio.plan.mesh).toEqual({ band: "5g", channel: 36, width: 80 });
    expect(kept.radio.plan.radios.map((r) => `${r.node}/${r.radio}/${r.channel}`)).toEqual([
      "1/radio0/6",
      "1/radio1/149",
    ]);
    const first = band.radios[0];
    if (!first) throw new Error("no radio");
    first.reasons = [{ kind: "mesh_group", radios: 2 }];
    first.planned.tx_power_dbm = null;
    const meshed = withBandPlan(config, band);
    expect(meshed.radio.plan.mesh).toEqual({ band: "5g", channel: 149, width: 80 });
    expect(meshed.radio.plan.radios[1]?.tx_power).toBe("auto");
  });
});
