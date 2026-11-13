import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { alertRecord, summary } from "../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../test/providers";
import { createAppRouter } from "./routes";
import { Shell } from "./Shell";

let posts: string[] = [];

beforeEach(() => {
  posts = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (init?.method === "POST") {
        posts.push(url);
        return Response.json(alertRecord(2, { acknowledged: { by: 1, username: "admin", at: 1 } }));
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

function renderShell(role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/updates"));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "Gateway", { role: "gateway" }), summary(3, "Garden AP")]);
  return ws;
}

describe("the alerts bell", () => {
  it("counts the alerts wanting attention and lists those firing", async () => {
    const user = userEvent.setup();
    const ws = renderShell();
    expect(
      await screen.findByRole("button", { name: "Alerts, none needing attention" }),
    ).toBeVisible();

    ws.receive({
      type: "alerts_snapshot",
      topic: "alerts",
      alerts: [
        alertRecord(1, { silenced: true, name: "Device running hot" }),
        alertRecord(2, {
          level: "critical",
          name: "Device offline",
          subject: "node:3/offline",
          node: 3,
          cause: {
            type: "event",
            event: 4,
            kind: "node_offline",
            matches: 1,
            condition: "node:3/offline",
          },
        }),
        alertRecord(3),
      ],
    });
    const bell = await screen.findByRole("button", { name: "Alerts, 2 needing attention" });
    expect(bell).toHaveTextContent("2");

    await user.click(bell);
    const popover = await screen.findByRole("dialog");
    expect(within(popover).getByText("3 firing")).toBeVisible();
    const items = within(popover)
      .getAllByRole("listitem")
      .map((item) => item.textContent ?? "");
    // The critical one first; the silenced one last.
    expect(items[0]).toContain("Device offline");
    expect(items[0]).toContain("Garden AP");
    expect(items[1]).toContain("CPU 93 %, above 90 %");
    expect(items[2]).toContain("Silenced");
    expect(within(popover).getAllByRole("link")[0]).toHaveAttribute(
      "href",
      "/insights/alerts?alert=2",
    );

    await user.click(
      within(popover).getByRole("button", { name: "Acknowledge: Device offline, Garden AP" }),
    );
    await waitFor(() => expect(posts).toEqual(["/api/v1/alerts/2/acknowledge"]));
    ws.receive({
      type: "alerts_delta",
      topic: "alerts",
      alerts: [
        alertRecord(2, {
          level: "critical",
          name: "Device offline",
          node: 3,
          acknowledged: { by: 1, username: "admin", at: 1 },
        }),
      ],
    });
    expect(
      await screen.findByRole("button", { name: "Alerts, 1 needing attention" }),
    ).toBeVisible();

    // Resolved alerts leave.
    ws.receive({
      type: "alerts_delta",
      topic: "alerts",
      alerts: [alertRecord(3, { resolved_at: Date.now(), resolution: "cleared" })],
    });
    expect(
      await screen.findByRole("button", { name: "Alerts, none needing attention" }),
    ).toBeVisible();
  });

  it("is quiet when nothing fires, and offers viewers no acknowledging", async () => {
    const user = userEvent.setup();
    const ws = renderShell("viewer");
    ws.receive({ type: "alerts_snapshot", topic: "alerts", alerts: [] });
    await user.click(await screen.findByRole("button", { name: /^Alerts/ }));
    expect(await screen.findByText("All quiet")).toBeVisible();

    ws.receive({ type: "alerts_delta", topic: "alerts", alerts: [alertRecord(5)] });
    const popover = await screen.findByRole("dialog");
    await within(popover).findByText("Device CPU busy");
    expect(within(popover).queryByRole("button", { name: /Acknowledge/ })).not.toBeInTheDocument();
  });
});
