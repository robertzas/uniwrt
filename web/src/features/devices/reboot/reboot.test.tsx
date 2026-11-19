import { toaster } from "@kobalte/core/toast";
import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Job } from "../../../api/gen/Job";
import type { JobNode } from "../../../api/gen/JobNode";
import { createAppRouter } from "../../../app/routes";
import { detail, facts, summary } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import type { FakeWebSocket } from "../../../test/socket";
import { Toaster } from "../../../ui/Toast";
import { defaultPick, rebootOrder, runningReboot } from "./reboot";

const FLEET = [
  summary(1, "Gateway", { role: "gateway", mode: "managed" }),
  summary(2, "Living room", { mode: "managed" }),
  summary(3, "Garage", { mode: "managed", status: { state: "offline", since: 0, reasons: [] } }),
  summary(4, "Attic"),
];

function rebootJob(state: Job["state"], nodes: JobNode[], extra: Partial<Job> = {}): Job {
  return {
    id: 7,
    kind: "reboot",
    target: nodes.map((node) => node.name).join(", "),
    node: null,
    revision: null,
    state,
    steps: [
      {
        name: "reboot",
        state: state === "running" ? "running" : state === "succeeded" ? "done" : "failed",
        detail: null,
        started_at: 1,
        finished_at: null,
      },
    ],
    nodes,
    error: null,
    created_by: 1,
    started_at: 1,
    finished_at: null,
    ...extra,
  };
}

const node = (id: number, name: string, state: JobNode["state"], detail: string | null = null) => ({
  node: id,
  name,
  state,
  detail,
});

let calls: { key: string; body?: unknown }[] = [];

function stubApi() {
  calls = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const key = `${init?.method ?? "GET"} ${url}`;
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ key, body });
      if (key === "POST /api/v1/reboots") {
        const names = new Map(FLEET.map((n) => [n.id, n.name]));
        return Response.json(
          rebootJob(
            "running",
            (body.nodes as number[]).map((id) => node(id, names.get(id) ?? "", "pending")),
          ),
          { status: 202 },
        );
      }
      if (key.startsWith("GET /api/v1/nodes/")) return Response.json(detail(facts(5)));
      return Response.json({ status: "ok", version: "1.0.0", commit: "x", built_at: "y" });
    }),
  );
}

/** Renders `path`; on a device's page, answers its topic with the device as the fleet has it. */
async function renderAt(path: string): Promise<FakeWebSocket> {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory(path));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket}>
        <Router />
        <Toaster />
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, FLEET);
  const id = Number(path.split("/devices/")[1]);
  const own = FLEET.find((n) => n.id === id);
  if (own) {
    const topic = `node:${id}`;
    await waitFor(() => expect(ws.sent).toContainEqual({ sub: expect.arrayContaining([topic]) }));
    const base = detail(facts(5));
    const node = { ...base.node, id, name: own.name, role: own.role, mode: own.mode };
    ws.receive({ type: "node_snapshot", topic, node: { ...base, node } });
  }
  await waitFor(() => expect(ws.sent).toContainEqual({ sub: expect.arrayContaining(["jobs"]) }));
  ws.receive({ type: "jobs_snapshot", topic: "jobs", jobs: [] });
  return ws;
}

describe("reboot", () => {
  it("lists the gateway last and picks the managed devices that answer", () => {
    expect(rebootOrder(FLEET).map((n) => n.name)).toEqual([
      "Attic",
      "Garage",
      "Living room",
      "Gateway",
    ]);
    expect(defaultPick(FLEET)).toEqual([1, 2]);
  });

  it("finds a restart of the devices, started anywhere, while it runs", () => {
    const job = rebootJob("running", [node(2, "Living room", "applied")]);
    const jobs = { byId: { 7: job }, ready: true };
    expect(runningReboot(jobs, [2])).toBe(job);
    expect(runningReboot(jobs, [1])).toBeUndefined();
    expect(runningReboot({ byId: { 7: { ...job, state: "succeeded" } }, ready: true }, [2])).toBe(
      undefined,
    );
  });
});

describe("Restarting devices", () => {
  beforeEach(stubApi);
  afterEach(() => toaster.clear());

  it("restarts the picked devices from the list and follows the job", async () => {
    const user = userEvent.setup();
    const ws = await renderAt("/devices");
    await user.click(await screen.findByRole("button", { name: "Restart…" }));
    const dialog = await screen.findByRole("dialog", { name: "Restart devices" });
    const box = (name: string) => within(dialog).getByRole("checkbox", { name });
    expect(box("Gateway")).toBeChecked();
    expect(box("Living room")).toBeChecked();
    expect(box("Garage")).not.toBeChecked();
    expect(box("Attic")).toBeDisabled();
    expect(dialog).toHaveTextContent("Restarts last. The network has no internet meanwhile.");
    expect(dialog).toHaveTextContent(
      "While the gateway restarts, the whole network has no internet.",
    );

    await user.click(box("Gateway"));
    expect(dialog).not.toHaveTextContent("While the gateway restarts");
    await user.click(within(dialog).getByRole("button", { name: "Restart" }));
    await waitFor(() =>
      expect(calls).toContainEqual({ key: "POST /api/v1/reboots", body: { nodes: [2] } }),
    );
    const list = await within(dialog).findByRole("list", { name: "Devices" });
    expect(list).toHaveTextContent("Living roomWaiting");

    ws.receive({
      type: "jobs_delta",
      topic: "jobs",
      jobs: [rebootJob("running", [node(2, "Living room", "applied", "Restarting")])],
    });
    await waitFor(() => expect(list).toHaveTextContent("Living roomRestarting"));
    ws.receive({
      type: "jobs_delta",
      topic: "jobs",
      jobs: [rebootJob("succeeded", [node(2, "Living room", "verified", "Back after 42 s")])],
    });
    await waitFor(() => expect(list).toHaveTextContent("Living roomBackBack after 42 s"));
    expect(await screen.findAllByText("The device restarted.")).not.toHaveLength(0);
  });

  it("refuses to restart a device in observe mode", async () => {
    await renderAt("/devices/4");
    const observed = await screen.findByRole("button", { name: "Restart" });
    expect(observed).toBeDisabled();
    expect(observed).toHaveAttribute("title", "Turn management on to restart it.");
  });

  it("restarts one managed device after asking, and says the job failed", async () => {
    const user = userEvent.setup();
    const ws = await renderAt("/devices/1");
    await user.click(await screen.findByRole("button", { name: "Restart" }));
    const dialog = await screen.findByRole("dialog", { name: "Restart Gateway?" });
    expect(within(dialog).queryByRole("checkbox")).toBeNull();
    expect(dialog).toHaveTextContent(
      "While the gateway restarts, the whole network has no internet.",
    );
    await user.click(within(dialog).getByRole("button", { name: "Restart" }));
    await waitFor(() =>
      expect(calls).toContainEqual({ key: "POST /api/v1/reboots", body: { nodes: [1] } }),
    );
    ws.receive({
      type: "jobs_delta",
      topic: "jobs",
      jobs: [
        rebootJob("failed", [node(1, "Gateway", "failed", "Not back within 10 min")], {
          error: { code: "not_back", message: "Gateway didn't come back" },
        }),
      ],
    });
    expect(await screen.findByText("The restarts stopped")).toBeVisible();
    expect(within(dialog).getByRole("list", { name: "Devices" })).toHaveTextContent(
      "GatewayFailedNot back within 10 min",
    );
  });
});
