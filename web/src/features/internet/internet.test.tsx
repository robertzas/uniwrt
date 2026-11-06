import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Job } from "../../api/gen/Job";
import type { SiteSave } from "../../api/gen/SiteSave";
import type { StepName } from "../../api/gen/StepName";
import type { WanTestBody } from "../../api/gen/WanTestBody";
import { createAppRouter } from "../../app/routes";
import { Shell } from "../../app/Shell";
import { summary } from "../../test/fixtures";
import { LUMEN, PPPOE } from "../../test/presets";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { revisionMeta, siteView } from "../../test/site";
import type { FakeWebSocket } from "../../test/socket";

let calls: { method: string; url: string; body: unknown }[] = [];
let head = 2;

const STEPS: StepName[] = [
  "connect",
  "plan",
  "stage",
  "apply",
  "link",
  "address",
  "gateway",
  "dns",
  "https",
  "ipv6",
  "six_rd",
  "rollback",
];

function wanJob(
  state: Job["state"],
  states: Partial<Record<StepName, Job["steps"][0]["state"]>>,
): Job {
  return {
    id: 21,
    kind: "wan_test",
    target: "gw",
    node: 1,
    revision: null,
    state,
    steps: STEPS.map((name) => ({
      name,
      state: states[name] ?? (state === "running" ? "pending" : "done"),
      detail: name === "address" ? "Lease 203.0.113.57/24" : null,
      started_at: null,
      finished_at: null,
    })),
    nodes: [],
    error: null,
    created_by: 1,
    started_at: 1,
    finished_at: state === "running" ? null : 2,
  };
}

beforeEach(() => {
  calls = [];
  head = 2;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site" && method === "GET") return Response.json(siteView(head));
      if (url === "/api/v1/site" && method === "PUT") {
        head += 1;
        return Response.json(siteView(head, (body as SiteSave).config));
      }
      if (url === "/api/v1/isp/presets") return Response.json([PPPOE, LUMEN]);
      if (url === "/api/v1/wan/test") {
        return Response.json(wanJob("running", { connect: "running" }), { status: 202 });
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderInternet(mode: "managed" | "observe" = "managed"): Promise<FakeWebSocket> {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/internet"));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "gw", { role: "gateway", mode })]);
  ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
  await screen.findByRole("heading", { name: "Internet", level: 1 });
  return ws;
}

describe("Internet", () => {
  it("shows the connection in words", async () => {
    await renderInternet();
    const card = await screen.findByLabelText("Connection");
    expect(await within(card).findByText("Custom settings")).toBeInTheDocument();
    expect(within(card).getByText("DHCPv6 with prefix delegation")).toBeInTheDocument();
    // The fixture's gateway manages no package: applying would leave its WAN alone.
    expect(screen.getByText(/doesn't manage the network package on gw/)).toBeInTheDocument();
  });

  it("picks Lumen, tests it with the 6rd fallback, and saves 6rd when native IPv6 failed", async () => {
    const user = userEvent.setup();
    const ws = await renderInternet();
    await user.click(await screen.findByRole("button", { name: "Set up" }));
    const dialog = await screen.findByRole("dialog", { name: "Internet connection" });
    // The site's country (US) lists its ISPs before the generic presets.
    await user.click(within(dialog).getByRole("button", { name: /Lumen/ }));
    expect(within(dialog).getByText(/tries native IPv6 first/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Test the connection" }));
    await waitFor(() => expect(calls.some((c) => c.url === "/api/v1/wan/test")).toBe(true));
    const sent = calls.find((c) => c.url === "/api/v1/wan/test")?.body as WanTestBody;
    expect(sent.wan).toMatchObject({
      vlan: 201,
      preset: "us-lumen-ipoe",
      ipv6: { mode: "dhcpv6" },
    });
    expect(sent.fallback).toEqual({
      mode: "6rd",
      prefix: "2602::/24",
      border_relay: "205.171.2.64",
      ipv4_mask_len: 0,
    });
    expect(await within(dialog).findByText("Log in to the gateway")).toBeInTheDocument();

    ws.receive({
      type: "jobs_delta",
      topic: "jobs",
      jobs: [wanJob("succeeded", { ipv6: "failed" })],
    });
    expect(await within(dialog).findByText(/but 6rd did/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByText("Saved as revision 3")).toBeInTheDocument();
    const put = calls.find((c) => c.method === "PUT" && c.url === "/api/v1/site")?.body as SiteSave;
    expect(put.base).toBe(2);
    expect(put.message).toBe("Internet: Lumen / Quantum Fiber / CenturyLink Fiber · IPoE");
    expect(put.config.wan).toMatchObject({ vlan: 201, ipv6: { mode: "6rd", prefix: "2602::/24" } });
    expect(within(dialog).getByRole("button", { name: "Review and apply" })).toBeEnabled();
  });

  it("asks for a preset's fields and checks them before testing", async () => {
    const user = userEvent.setup();
    await renderInternet("observe");
    expect(await screen.findByText(/gw is in observe mode/)).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Set up" }));
    const dialog = await screen.findByRole("dialog", { name: "Internet connection" });
    await user.click(within(dialog).getByRole("button", { name: /DSL and PPPoE fiber/ }));
    // The gateway is observed: no test, but saving works.
    expect(within(dialog).getByRole("button", { name: "Test the connection" })).toBeDisabled();
    await user.click(within(dialog).getByRole("button", { name: "Save without testing" }));
    expect(within(dialog).getByText("PPPoE needs the ISP's username")).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText("Username"), "me@isp");
    await user.type(within(dialog).getByLabelText("Password"), "secret");
    await user.click(within(dialog).getByRole("button", { name: "Save without testing" }));
    expect(await within(dialog).findByText("Saved as revision 3")).toBeInTheDocument();
    const put = calls.find((c) => c.method === "PUT" && c.url === "/api/v1/site")?.body as SiteSave;
    expect(put.config.wan.proto).toEqual({
      kind: "pppoe",
      username: "me@isp",
      password: null,
      service: null,
    });
    expect(put.secrets).toEqual({ "wan/password": "secret" });
    expect(put.config.wan.mtu).toBe(1492);
  });
});
