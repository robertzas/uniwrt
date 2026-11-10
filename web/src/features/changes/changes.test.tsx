import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ApplyPlan } from "../../api/gen/ApplyPlan";
import { createAppRouter } from "../../app/routes";
import { Shell } from "../../app/Shell";
import { drift } from "../../test/drift";
import { summary } from "../../test/fixtures";
import { applyJob, applyPlan, nodePlan } from "../../test/plan";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { revisionMeta, siteView } from "../../test/site";
import type { FakeWebSocket } from "../../test/socket";

let plan: ApplyPlan;
let posts: { url: string; body: unknown }[] = [];

beforeEach(() => {
  plan = applyPlan();
  posts = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (init?.method === "POST") {
        posts.push({ url, body: JSON.parse(String(init.body ?? "null")) });
        if (url === "/api/v1/apply") return Response.json(applyJob("running"), { status: 202 });
        if (url.endsWith("/restore")) return Response.json(siteView(6));
      }
      if (url === "/api/v1/apply/plan") return Response.json(plan);
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

function sendConfig(ws: FakeWebSocket, head = 5) {
  ws.receive({
    type: "config",
    topic: "config",
    head: revisionMeta(head, { message: "Guest Wi-Fi" }),
    applied: [
      { node: 1, revision: 4, job: 3, at: 0 },
      { node: 2, revision: 4, job: 3, at: 0 },
    ],
  });
}

async function renderAt(path: string, role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory(path);
  const Router = createAppRouter(history);
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [
    summary(1, "Gateway", { role: "gateway", mode: "managed" }),
    summary(2, "Attic AP", { mode: "managed" }),
    summary(4, "Shed AP"),
  ]);
  sendConfig(ws);
  ws.receive({ type: "jobs_snapshot", topic: "jobs", jobs: [] });
  return { ws, history };
}

describe("Review & apply", () => {
  it("counts the devices behind in the top bar and opens the review", async () => {
    const user = userEvent.setup();
    const { history } = await renderAt("/wifi");
    const tray = await screen.findByRole("link", { name: "Pending changes: 2 devices to update" });
    expect(tray).toHaveTextContent("2");
    await user.click(tray);
    await waitFor(() => expect(history.get()).toBe("/changes"));
    expect(await screen.findByRole("heading", { name: "Review changes", level: 1 })).toBeVisible();
    expect(document.querySelector("header")).toHaveTextContent("Review changes");
  });

  it("shows the impact, each node's diff with secrets masked, and the nodes left alone", async () => {
    const user = userEvent.setup();
    await renderAt("/changes");
    const summaryCard = await screen.findByRole("region", { name: "Review changes" });
    expect(summaryCard).toHaveTextContent("Revision 5 · Guest Wi-Fi");
    expect(summaryCard).toHaveTextContent("2 devices change");
    expect(summaryCard).toHaveTextContent("1 up to date");
    expect(summaryCard).toHaveTextContent("1 in observe mode");

    const impact = screen.getByRole("region", { name: "Impact" });
    expect(impact).toHaveTextContent("Wi-Fi restarts on Attic AP");
    expect(impact).toHaveTextContent("dnsmasq stops on Attic AP");
    expect(impact).toHaveTextContent("rolls back by itself");

    const order = screen.getByRole("region", { name: "In the order they apply" });
    const cards = within(order).getAllByRole("region");
    expect(cards.map((c) => c.getAttribute("aria-label"))).toEqual(["Attic AP", "Gateway"]);
    const attic = cards[0] as HTMLElement;
    expect(attic).toHaveTextContent("2 hops from the gateway");
    expect(attic).toHaveTextContent("/etc/config/wireless");
    expect(attic).toHaveTextContent("'uw_home_2g'");
    expect(attic).toHaveTextContent("was 'default_radio0'");
    expect(attic).toHaveTextContent("'OpenWrt'");
    expect(attic).toHaveTextContent("'Home'");
    expect(within(attic).getAllByText("secret")).toHaveLength(2);
    expect(attic).not.toHaveTextContent("••••");
    const gateway = cards[1] as HTMLElement;
    expect(gateway).toHaveTextContent("@device[1]");
    expect(gateway).toHaveTextContent("Reached at 10.0.0.1 once applied (now 192.168.1.1)");
    expect(gateway).toHaveTextContent(
      "it keeps 192.168.1.0/24 beside 10.0.0.0/24 until uniwrt's host has an address on 10.0.0.0/24",
    );
    expect(gateway).toHaveTextContent("The WAN changes last, on its own");
    expect(attic).not.toHaveTextContent("The WAN changes last");
    expect(gateway).toHaveTextContent("factory subnet");
    // Packages open and close.
    await user.click(within(gateway).getByRole("button", { name: "The network package" }));
    expect(gateway).not.toHaveTextContent("br-lan.20");
    // Its raw overrides, closed until opened, a sealed secret never shown.
    expect(gateway).not.toHaveTextContent("wireguard");
    await user.click(within(gateway).getByRole("button", { name: /Raw overrides/ }));
    expect(gateway).toHaveTextContent("2 sections");
    expect(gateway).toHaveTextContent("config interface 'wg0'");
    expect(gateway).toHaveTextContent("option proto 'wireguard'");
    expect(gateway).toHaveTextContent("list addresses '10.9.0.1/24'");
    expect(gateway).toHaveTextContent("option private_key secret");
    expect(gateway).not.toHaveTextContent("7777");
    expect(gateway).toHaveTextContent("Merged");
    expect(within(attic).queryByRole("button", { name: /Raw overrides/ })).toBeNull();

    const alone = screen.getByRole("region", { name: "Left alone" });
    expect(alone).toHaveTextContent("Hall AP");
    expect(alone).toHaveTextContent("Shed AP");
    expect(alone).toHaveTextContent("Observe mode");
  });

  it("marks the drift an apply overwrites, counted in the top bar", async () => {
    const user = userEvent.setup();
    const gatewayPlan = plan.nodes[1];
    if (!gatewayPlan) throw new Error("the plan's gateway");
    gatewayPlan.drift = ["network", "firewall"];
    const { ws, history } = await renderAt("/wifi");
    ws.receive({
      type: "config",
      topic: "config",
      head: revisionMeta(5, { message: "Guest Wi-Fi" }),
      applied: [
        { node: 1, revision: 4, job: 3, at: 0 },
        { node: 2, revision: 4, job: 3, at: 0 },
      ],
      drift: {
        drifts: [
          drift(1, "network"),
          drift(1, "firewall", { removed: true, diff: { package: "firewall", sections: [] } }),
          drift(2, "system", { acknowledged: { at: 0, by: "ada" } }),
        ],
        muted: [],
      },
    });
    const chip = await screen.findByRole("button", {
      name: "Drift: 2 packages changed on the devices",
    });
    await user.click(chip);
    const popover = await screen.findByRole("dialog", { name: "Drift" });
    const rows = within(popover).getAllByRole("link");
    expect(rows.map((row) => row.getAttribute("href"))).toEqual([
      "/devices/1?tab=config",
      "/devices/1?tab=config",
      "/changes",
    ]);
    expect(rows[0]).toHaveTextContent("Gateway · network");
    expect(rows[1]).toHaveTextContent("removed");
    await user.click(within(popover).getByRole("link", { name: "Review & overwrite" }));
    await waitFor(() => expect(history.get()).toBe("/changes"));

    const gateway = await screen.findByRole("region", { name: "Gateway" });
    expect(gateway).toHaveTextContent(
      "Overwrites the drift of network: changed on the device after uniwrt confirmed it.",
    );
    const network = within(gateway).getByRole("button", { name: "The network package" });
    expect(network).toHaveTextContent("Drift");
    const dhcp = within(gateway).getByRole("button", { name: "The dhcp package" });
    expect(dhcp).not.toHaveTextContent("Drift");
    expect(screen.getByRole("region", { name: "Impact" })).toHaveTextContent(
      "Overwrites a change made by hand on Gateway.",
    );
  });

  it("applies the reviewed revision and follows each node live", async () => {
    const user = userEvent.setup();
    const { ws } = await renderAt("/changes");
    await user.click(await screen.findByRole("button", { name: "Apply revision 5" }));
    await waitFor(() => expect(posts).toEqual([{ url: "/api/v1/apply", body: { revision: 5 } }]));
    const progress = await screen.findByRole("region", { name: "Applying revision 5" });
    expect(progress).toHaveTextContent("You can leave this page");
    ws.receive({
      type: "jobs_delta",
      topic: "jobs",
      jobs: [applyJob("running", [{ name: "Gateway", state: "applied" }, { state: "verified" }])],
    });
    const devices = await within(progress).findByRole("list", { name: "Devices" });
    expect(devices).toHaveTextContent("Applied, checking");
    expect(devices).toHaveTextContent("Passed its checks");
    // The top bar says an apply runs.
    expect(screen.getByRole("link", { name: /Changes are being applied/ })).toBeVisible();
    ws.receive({
      type: "jobs_delta",
      topic: "jobs",
      jobs: [applyJob("succeeded", [{ name: "Gateway", state: "confirmed" }])],
    });
    expect(await within(progress).findByText("Revision 5 is applied and confirmed.")).toBeVisible();
    expect(within(progress).getByRole("list", { name: "Devices" })).toHaveTextContent("Confirmed");
    await user.click(within(progress).getByRole("button", { name: "Review again" }));
    expect(await screen.findByRole("region", { name: "Impact" })).toBeVisible();
  });

  it("explains a rolled-back apply", async () => {
    const { ws } = await renderAt("/changes");
    await screen.findByRole("region", { name: "Impact" });
    const failed = {
      ...applyJob("failed", [{ name: "Gateway", state: "rolled_back", detail: "WAN down" }]),
      error: { code: "rolled_back" as const, message: "Gateway failed its checks" },
    };
    // An apply started elsewhere shows once seen running, and stays once it ends.
    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [{ ...failed, state: "running" }] });
    const progress = await screen.findByRole("region", { name: "Applying revision 5" });
    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [failed] });
    expect(await within(progress).findByText("The changes were rolled back")).toBeVisible();
    expect(progress).toHaveTextContent("Gateway failed its checks");
    expect(progress).toHaveTextContent("WAN down");
    expect(within(progress).getByRole("link", { name: "Open the history" })).toBeVisible();
  });

  it("refuses to apply while a node can't, or once another revision is saved", async () => {
    plan.nodes[1] = nodePlan(1, "Gateway", {
      action: "refused",
      role: "gateway",
      issues: [{ path: "node", message: "no WAN port in board.json" }],
    });
    const { ws } = await renderAt("/changes");
    const apply = await screen.findByRole("button", { name: "Apply revision 5" });
    expect(apply).toBeDisabled();
    expect(screen.getByText("Some devices can't take these changes")).toBeVisible();
    expect(screen.getByRole("region", { name: "Gateway" })).toHaveTextContent(
      "no WAN port in board.json",
    );

    plan = applyPlan();
    await userEvent.setup().click(screen.getByRole("button", { name: "Review again" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Apply revision 5" })).toBeEnabled(),
    );
    sendConfig(ws, 6);
    expect(await screen.findByText(/Revision 6 was saved since this review/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Apply revision 5" })).toBeDisabled();
  });

  it("discards the changes back to the revision the devices hold", async () => {
    const user = userEvent.setup();
    await renderAt("/changes");
    await user.click(await screen.findByRole("button", { name: "Discard changes" }));
    const dialog = await screen.findByRole("dialog", { name: "Go back to revision 4?" });
    await user.click(within(dialog).getByRole("button", { name: "Discard changes" }));
    await waitFor(() =>
      expect(posts).toEqual([
        {
          url: "/api/v1/revisions/4/restore",
          body: { base: 5, message: "Discarded the changes after revision 4" },
        },
      ]),
    );
  });

  it("keeps viewers out of the review and the tray", async () => {
    await renderAt("/changes", "viewer");
    expect(await screen.findByText("Only admins apply changes")).toBeVisible();
    expect(screen.queryByRole("link", { name: /Pending changes/ })).toBeNull();
  });
});
