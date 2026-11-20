import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DiagRun } from "../../../api/gen/DiagRun";
import type { OptionalToolStatus } from "../../../api/gen/OptionalToolStatus";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { detail, facts, summary } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import { revisionMeta, siteView } from "../../../test/site";

let calls: { method: string; url: string; body: unknown }[] = [];
let runs: DiagRun[] = [];
let tools: OptionalToolStatus[] = [];

function run(id: number, extra: Partial<DiagRun> = {}): DiagRun {
  return {
    id,
    node: 1,
    tool: { tool: "ping", target: "openwrt.org", family: null, count: 4 },
    state: "running",
    by: "admin",
    started_at: 1_800_000_000_000,
    ended_at: null,
    exit_code: null,
    error: null,
    lines: [],
    dropped: 0,
    ...extra,
  };
}

const status = (tool: OptionalToolStatus["tool"], installed: boolean): OptionalToolStatus => ({
  tool,
  installed,
  package: tool === "tcpdump" ? "tcpdump-mini" : tool === "ethtool" ? "ethtool-full" : "iperf3",
  replaces: tool === "ethtool" ? "ethtool" : null,
});

beforeEach(() => {
  calls = [];
  runs = [];
  tools = [status("tcpdump", true), status("iperf3", false), status("ethtool", false)];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site") return Response.json(siteView(2));
      if (url === "/api/v1/diag/runs" && method === "GET") return Response.json(runs);
      if (url === "/api/v1/diag/runs") {
        return Response.json(run(9, { tool: body.tool, lines: [] }), { status: 202 });
      }
      if (url.startsWith("/api/v1/diag/runs/") && method === "DELETE") {
        return Response.json(run(9));
      }
      if (url === "/api/v1/nodes/1/diag/tools") return Response.json(tools);
      if (url === "/api/v1/nodes/1/packages" && method === "POST") {
        return Response.json(
          {
            id: 30,
            kind: "packages",
            target: "gw",
            node: 1,
            revision: null,
            state: "running",
            steps: [],
            nodes: [],
            error: null,
            created_by: 1,
            started_at: 1,
            finished_at: null,
          },
          { status: 202 },
        );
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderTab(options: { role?: "admin" | "viewer"; managed?: boolean } = {}) {
  const role = options.role ?? "admin";
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/devices/1?tab=tools"));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  await waitFor(() => {
    const sent = ws.sent.at(-1) as { sub?: string[] } | undefined;
    expect(sent?.sub).toContain("node:1");
  });
  sendFleet(ws, [
    summary(1, "gw", options.managed ? { mode: "managed" } : {}),
    summary(2, "ap-garden", { role: "ap" }),
  ]);
  const node = detail(facts(1));
  if (options.managed) node.node.mode = "managed";
  ws.receive({ type: "node_snapshot", topic: "node:1", node });
  ws.receive({ type: "config", topic: "config", head: revisionMeta(2), applied: [] });
  await screen.findByRole("heading", { name: "gw" });
  const form = await screen.findByRole("region", { name: "Run a test" });
  return { ws, form };
}

const subscribed = (ws: { sent: unknown[] }) =>
  (ws.sent.at(-1) as { sub?: string[] } | undefined)?.sub ?? [];

describe("the Tools tab", () => {
  it("runs a ping and follows its output live, then stops it", async () => {
    const user = userEvent.setup();
    const { ws, form } = await renderTab();
    const host = within(form).getByLabelText("Host");
    await user.type(host, "-c");
    await user.tab();
    expect(within(form).getByText("Enter a host name or an address.")).toBeTruthy();
    await user.clear(host);
    await user.type(host, "openwrt.org");
    const count = within(form).getByLabelText("Requests");
    await user.clear(count);
    await user.type(count, "4");
    await user.click(within(form).getByRole("button", { name: "Run" }));
    await waitFor(() =>
      expect(calls.find((call) => call.method === "POST")?.body).toEqual({
        node: 1,
        tool: { tool: "ping", target: "openwrt.org", family: null, count: 4 },
      }),
    );
    await waitFor(() => expect(subscribed(ws)).toContain("diag:9"));
    ws.receive({ type: "diag_snapshot", topic: "diag:9", run: run(9) });
    const output = await screen.findByRole("region", { name: "Ping openwrt.org" });
    expect(within(output).getByText("Waiting for output…")).toBeTruthy();
    ws.receive({
      type: "diag_delta",
      topic: "diag:9",
      lines: [{ at: 1, err: false, text: "PING openwrt.org (139.59.209.225): 56 data bytes" }],
      dropped: 0,
      end: null,
    });
    await within(output).findByText(/56 data bytes/);
    await user.click(within(output).getByRole("button", { name: "Stop" }));
    await waitFor(() =>
      expect(
        calls.some((call) => call.method === "DELETE" && call.url === "/api/v1/diag/runs/9"),
      ).toBe(true),
    );
    ws.receive({
      type: "diag_delta",
      topic: "diag:9",
      lines: [],
      dropped: 0,
      end: {
        state: "stopped",
        ended_at: 1_800_000_004_000,
        exit_code: null,
        error: "Stopped by admin",
      },
    });
    await within(output).findByText("Stopped by admin");
    expect(within(output).queryByRole("button", { name: "Stop" })).toBeNull();
  });

  it("shows a throughput test's rate each way, and its far end by name", async () => {
    runs = [
      run(4, {
        tool: { tool: "iperf3", peer: { node: 2 }, direction: "send", seconds: 1, streams: 1 },
        state: "succeeded",
        ended_at: 1_800_000_012_000,
        exit_code: 0,
      }),
    ];
    const { ws } = await renderTab();
    const list = await screen.findByRole("list", { name: "Recent runs on this device" });
    expect(within(list).getByText("Throughput to ap-garden")).toBeTruthy();
    await waitFor(() => expect(subscribed(ws)).toContain("diag:4"));
    const lines = [
      "[  5]   0.00-1.00   sec   112 MBytes   940 Mbits/sec    0   2.25 MBytes       ",
      "[  5]   0.00-1.00   sec   112 MBytes   940 Mbits/sec    2             sender",
      "[  5]   0.00-1.00   sec   111 MBytes   931 Mbits/sec                  receiver",
    ].map((text) => ({ at: 1, err: false, text }));
    ws.receive({ type: "diag_snapshot", topic: "diag:4", run: { ...runs[0], lines } as DiagRun });
    const output = await screen.findByRole("region", { name: "Throughput to ap-garden" });
    expect(within(output).getByText("gw → ap-garden")).toBeTruthy();
    expect(within(output).getByText("931 Mb/s")).toBeTruthy();
    expect(within(output).getByText("Sent 940 Mb/s · 117.4 MB · 2 retransmits")).toBeTruthy();
  });

  it("warns of a missing tool and installs it through the package job", async () => {
    const user = userEvent.setup();
    const { form } = await renderTab({ managed: true });
    await user.click(within(form).getByRole("button", { name: "Cable test" }));
    expect(within(form).getByLabelText("Port")).toBeTruthy();
    await within(form).findByText(/needs the ethtool-full package/);
    const card = screen.getByRole("region", { name: "Optional tools" });
    const row = within(card).getByText("Cable tests").closest("li") as HTMLElement;
    await user.click(within(row).getByRole("button", { name: "Install" }));
    await waitFor(() =>
      expect(calls.find((call) => call.url === "/api/v1/nodes/1/packages")?.body).toEqual({
        install: ["ethtool-full"],
        remove: ["ethtool"],
        upgrade: [],
      }),
    );
    await within(row).findByRole("button", { name: "Installing…" });
  });

  it("is read-only for viewers and in observe mode", async () => {
    const { form } = await renderTab({ role: "viewer" });
    expect(within(form).getByText("Only admins run diagnostics on devices.")).toBeTruthy();
    expect((within(form).getByRole("button", { name: "Run" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    const card = screen.getByRole("region", { name: "Optional tools" });
    await within(card).findByText(
      "Read-only in observe mode: turn management on to install tools.",
    );
    expect(within(card).queryByRole("button", { name: "Install" })).toBeNull();
  });
});
