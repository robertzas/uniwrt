import { memoryHistory } from "@solidjs/router";
import { fireEvent, render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import type { SiteSave } from "../../../api/gen/SiteSave";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { summary } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import { revisionMeta, siteConfig, siteView } from "../../../test/site";

let calls: { method: string; url: string; body: unknown }[] = [];
let head = 2;
let config: SiteConfig = siteConfig();

beforeEach(() => {
  calls = [];
  head = 2;
  config = siteConfig();
  config.site.timezone = "Europe/Berlin";
  config.nodes = [
    {
      node: 2,
      hostname: null,
      managed: [],
      radios: [],
      ports: [],
      raw: [],
      reboot: { kind: "own", days: ["sat"], at: 180 },
      led_schedule: false,
    },
  ];
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
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderSchedules(role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/settings"));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  await waitFor(() => expect(ws.sent.length).toBeGreaterThan(0));
  sendFleet(ws, [
    summary(1, "gw", { role: "gateway", mode: "managed" }),
    summary(2, "Office AP", { mode: "managed" }),
    summary(3, "Garage", { mode: "observe" }),
  ]);
  ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
  return within(await screen.findByRole("region", { name: "Schedules" }, { timeout: 3000 }));
}

describe("Settings › Site › Schedules", () => {
  it("saves a restart time and an LEDs-off schedule, with nothing to apply", async () => {
    const user = userEvent.setup();
    const card = await renderSchedules();
    expect(card.getByText(/Europe\/Berlin/)).toBeInTheDocument();
    expect(card.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();

    await user.click(card.getByLabelText("Restart the devices on a schedule"));
    const restarts = within(card.getByRole("region", { name: "Scheduled restarts" }));
    expect(restarts.getByRole("button", { name: "Sunday" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await user.click(restarts.getByRole("button", { name: "Wednesday" }));
    fireEvent.input(restarts.getByLabelText("At"), { target: { value: "03:30" } });
    expect(restarts.getByText(/^Next restart: /)).toBeInTheDocument();
    // The observed device doesn't count; the one with its own time says so.
    expect(
      restarts.getByText("Followed by gw, Office AP (its own: Sat 03:00)"),
    ).toBeInTheDocument();

    await user.click(card.getByLabelText("Turn the LEDs off at certain times"));
    const leds = within(card.getByRole("region", { name: "LEDs off" }));
    expect(leds.getByText("until the next day")).toBeInTheDocument();
    expect(card.getByText("Followed by gw")).toBeInTheDocument();

    await user.click(card.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls.some((c) => c.method === "PUT")).toBe(true));
    const saved = calls.find((c) => c.method === "PUT")?.body as SiteSave;
    expect(saved.config.schedules).toEqual({
      reboot: { days: ["wed", "sun"], at: 210 },
      leds_off: {
        spans: [{ days: ["mon", "tue", "wed", "thu", "fri", "sat", "sun"], from: 1380, to: 420 }],
      },
    });
    expect(saved.message).toBe("Edit the site's schedules");
    expect(await card.findByText(/without an apply/)).toBeInTheDocument();
  });

  it("refuses a restart without a day, and turns a schedule off", async () => {
    const user = userEvent.setup();
    config.schedules = { reboot: { days: ["sun"], at: 240 }, leds_off: null };
    const card = await renderSchedules();
    const restarts = within(card.getByRole("region", { name: "Scheduled restarts" }));
    await user.click(restarts.getByRole("button", { name: "Sunday" }));
    await user.click(card.getByRole("button", { name: "Save" }));
    expect(await restarts.findByRole("alert")).toHaveTextContent("Pick one day at least.");
    expect(calls.some((c) => c.method === "PUT")).toBe(false);

    await user.click(card.getByRole("button", { name: "Cancel" }));
    expect(restarts.getByRole("button", { name: "Sunday" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await user.click(card.getByLabelText("Restart the devices on a schedule"));
    await user.click(card.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls.some((c) => c.method === "PUT")).toBe(true));
    const saved = calls.find((c) => c.method === "PUT")?.body as SiteSave;
    expect(saved.config.schedules).toEqual({ reboot: null, leds_off: null });
  });

  it("shows viewers the schedules without letting them change them", async () => {
    config.schedules = { reboot: { days: ["sun"], at: 240 }, leds_off: null };
    const card = await renderSchedules("viewer");
    expect(card.getByLabelText("Restart the devices on a schedule")).toBeDisabled();
    expect(card.getByRole("button", { name: "Sunday" })).toBeDisabled();
    expect(card.getByText("Only admins can change the schedules.")).toBeInTheDocument();
  });
});
