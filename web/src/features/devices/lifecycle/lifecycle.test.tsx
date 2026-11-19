import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Job } from "../../../api/gen/Job";
import type { NodeDetail } from "../../../api/gen/NodeDetail";
import type { ZtpApproval } from "../../../api/gen/ZtpApproval";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { detail, facts, summary } from "../../../test/fixtures";
import { NO_PACKAGES } from "../../../test/packages";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import { revisionMeta, siteConfig, siteView } from "../../../test/site";
import { awaitingOf } from "./AwaitingCard";
import { removalOf } from "./Removed";

let calls: { method: string; url: string; body?: unknown }[] = [];
let approvals: ZtpApproval[] = [];
let refuse: string | undefined;

const steps: Record<
  "decommission" | "forget" | "replace" | "reset",
  Job["steps"][number]["name"][]
> = {
  decommission: ["connect", "remove_tool", "wipe", "record"],
  forget: ["record"],
  reset: ["connect", "remove_tool", "wipe", "record"],
  replace: ["connect", "check", "install_key", "verify_key", "inventory", "record", "watch"],
};

function job(kind: keyof typeof steps, state: Job["state"], extra: Partial<Job> = {}): Job {
  return {
    id: 30,
    kind,
    target: "gw",
    node: 1,
    revision: null,
    state,
    steps: steps[kind].map((name, i) => ({
      name,
      state: state === "running" ? (i === 0 ? "running" : "pending") : "done",
      detail:
        state === "succeeded" && name === "record" && kind === "replace" ? "No match: lan4" : null,
      started_at: null,
      finished_at: null,
    })),
    nodes: [],
    error: null,
    created_by: 1,
    started_at: 1,
    finished_at: state === "running" ? null : 2,
    ...extra,
  };
}

function approval(node: number | null): ZtpApproval {
  return {
    mac: "02:00:00:00:00:01",
    name: null,
    role: null,
    created_at: 1,
    expires_at: 253_402_300_799_000,
    created_by: "admin",
    node,
    board_name: "test,router",
  };
}

beforeEach(() => {
  calls = [];
  approvals = [];
  refuse = undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site") {
        const config = siteConfig();
        config.adoption_port = { port: "lan4", address: "192.168.1.250" };
        return Response.json(siteView(2, config));
      }
      if (url === "/api/v1/ztp/pending") return Response.json({ pending: [], approvals });
      if (url.startsWith("/api/v1/ztp/approvals/") && method === "DELETE") {
        approvals = [];
        return new Response(null, { status: 204 });
      }
      if (url === "/api/v1/nodes/1/root-password/reveal") return Response.json({ value: "s3cret" });
      if (refuse && method !== "GET") {
        return Response.json(
          { title: "Conflict", detail: refuse },
          { status: 409, headers: { "content-type": "application/problem+json" } },
        );
      }
      if (url === "/api/v1/adopt/probe") {
        return Response.json({
          host: "192.168.1.9",
          port: 22,
          host_key: "ssh-ed25519 AAAA",
          fingerprint: "SHA256:new",
          algorithm: "ED25519",
        });
      }
      if (url === "/api/v1/nodes/1/decommission")
        return Response.json(job("decommission", "running"), { status: 202 });
      if (url === "/api/v1/nodes/1" && method === "DELETE")
        return Response.json(job("forget", "running"), { status: 202 });
      if (url === "/api/v1/nodes/1/reset")
        return Response.json(job("reset", "running"), { status: 202 });
      if (url === "/api/v1/nodes/1/replace")
        return Response.json(job("replace", "running"), { status: 202 });
      if (url === "/api/v1/firmware") return Response.json({ nodes: [], error: null });
      if (url === "/api/v1/nodes/1/packages") return Response.json(NO_PACKAGES);
      if (url === "/api/v1/nodes/1/backups") return Response.json([]);
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderDevice(
  options: {
    role?: "admin" | "viewer";
    others?: boolean;
    tab?: string;
    node?: (d: NodeDetail) => void;
  } = {},
) {
  const role = options.role ?? "admin";
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory(`/devices/1${options.tab ?? "?tab=config"}`));
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
    summary(1, "gw", { role: "gateway" }),
    ...(options.others ? [summary(2, "ap-1", { role: "ap" })] : []),
  ]);
  const node = detail(facts(1));
  options.node?.(node);
  ws.receive({ type: "node_snapshot", topic: "node:1", node });
  ws.receive({ type: "config", topic: "config", head: revisionMeta(2), applied: [] });
  await screen.findByRole("heading", { name: "gw" });
  return { ws };
}

const card = () => screen.findByRole("region", { name: "Replace or remove" });

describe("lifecycle", () => {
  it("finds a node's removal and the approval its record awaits", () => {
    const old = job("decommission", "failed", { id: 3 });
    const latest = job("forget", "succeeded", { id: 7 });
    const other = job("decommission", "succeeded", { id: 9, node: 2 });
    const backup = { ...job("forget", "running", { id: 11 }), kind: "backup" as const };
    expect(removalOf([old, latest, other, backup], 1)).toBe(latest);
    expect(removalOf([other], 1)).toBeUndefined();
    const node = detail(null).node;
    expect(awaitingOf([approval(null), approval(1)], node)?.node).toBe(1);
    expect(awaitingOf([approval(null)], node)).toBeUndefined();
  });
});

describe("Device › Config › Replace or remove", () => {
  it("is for admins only", async () => {
    await renderDevice({ role: "viewer" });
    await screen.findByRole("region", { name: "Backups" });
    expect(screen.queryByRole("region", { name: "Replace or remove" })).toBeNull();
  });

  it("only replaces the gateway while other devices depend on it", async () => {
    await renderDevice({ others: true });
    const region = await card();
    expect(within(region).getByRole("button", { name: "Replace…" })).toBeEnabled();
    for (const name of ["Reset…", "Decommission…", "Forget…"]) {
      expect(within(region).getByRole("button", { name })).toBeDisabled();
    }
    expect(region).toHaveTextContent("remove them first, or replace its device");
  });

  it("decommissions after showing the root password, then shows the outcome in the page's place", async () => {
    const user = userEvent.setup();
    const { ws } = await renderDevice();
    await user.click(within(await card()).getByRole("button", { name: "Decommission…" }));
    const dialog = await screen.findByRole("dialog", { name: "Decommission gw?" });
    expect(dialog).toHaveTextContent("leaves the mesh");
    await user.click(within(dialog).getByRole("button", { name: "Show root password" }));
    expect(await within(dialog).findByText("s3cret")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("checkbox", { name: /Also reset it/ }));
    expect(within(dialog).queryByText("s3cret")).toBeNull();
    await user.click(within(dialog).getByRole("button", { name: "Decommission" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "POST",
        url: "/api/v1/nodes/1/decommission",
        body: { firstboot: true },
      }),
    );
    expect(
      await within(dialog).findByRole("list", { name: "Decommissioning gw" }),
    ).toBeInTheDocument();

    // The record goes before the job's end reaches the page.
    ws.receive({ type: "node_delta", topic: "node:1", events: [], roams: [], removed: true });
    await waitFor(() => {
      ws.receive({ type: "jobs_snapshot", topic: "jobs", jobs: [job("decommission", "running")] });
      expect(screen.getByRole("heading", { name: "Decommissioning gw" })).toBeInTheDocument();
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [job("decommission", "succeeded")] });
    expect(
      await screen.findByRole("heading", { name: "gw is decommissioned" }),
    ).toBeInTheDocument();
  });

  it("words the server's refusal of a forget", async () => {
    const user = userEvent.setup();
    refuse = "gw is still to be upgraded by rollout 4: skip it there, or cancel the rollout";
    await renderDevice();
    await user.click(within(await card()).getByRole("button", { name: "Forget…" }));
    const dialog = await screen.findByRole("dialog", { name: "Forget gw?" });
    expect(dialog).toHaveTextContent("Nothing changes on the device");
    await user.click(within(dialog).getByRole("button", { name: "Forget" }));
    expect(await within(dialog).findByText(refuse)).toBeInTheDocument();
    expect(calls).toContainEqual({ method: "DELETE", url: "/api/v1/nodes/1", body: undefined });
  });

  it("replaces the device after its fingerprint is confirmed, then leads to Review", async () => {
    const user = userEvent.setup();
    const { ws } = await renderDevice({ others: true });
    await user.click(within(await card()).getByRole("button", { name: "Replace…" }));
    const dialog = await screen.findByRole("dialog", { name: "Replace gw's device" });
    await user.type(within(dialog).getByLabelText("Address"), "192.168.1.9");
    await user.click(within(dialog).getByRole("button", { name: "Continue" }));
    expect(await within(dialog).findByTestId("fingerprint")).toHaveTextContent("SHA256:new");
    await user.click(within(dialog).getByRole("button", { name: "It matches, replace" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "POST",
        url: "/api/v1/nodes/1/replace",
        body: { host: "192.168.1.9", host_key: "ssh-ed25519 AAAA" },
      }),
    );
    expect(await within(dialog).findByText("Replacing gw's device")).toBeInTheDocument();
    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [job("replace", "succeeded")] });
    expect(await within(dialog).findByText("No match: lan4")).toBeInTheDocument();
    expect(within(dialog).getByRole("link", { name: "Review changes" })).toHaveAttribute(
      "href",
      "/changes",
    );
  });

  it("resets the device, then says how it comes back and stops waiting on request", async () => {
    const user = userEvent.setup();
    const { ws } = await renderDevice();
    await user.click(within(await card()).getByRole("button", { name: "Reset…" }));
    const dialog = await screen.findByRole("dialog", { name: "Reset gw to its defaults?" });
    expect(dialog).toHaveTextContent("test,router");
    await user.click(within(dialog).getByRole("button", { name: "Reset" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "POST",
        url: "/api/v1/nodes/1/reset",
        body: undefined,
      }),
    );
    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [job("reset", "succeeded")] });
    expect(await within(dialog).findByText(/^Reset\. Its record now waits/)).toBeInTheDocument();
  });

  it("shows an offline record awaiting its device with the adoption port's path", async () => {
    const user = userEvent.setup();
    approvals = [approval(1)];
    await renderDevice({
      tab: "",
      node: (d) => {
        d.status = { state: "offline", since: 1, reasons: [] };
      },
    });
    const waiting = await screen.findByRole("region", { name: "Waiting for its device" });
    expect(waiting).toHaveTextContent("MAC 02:00:00:00:00:01 (test,router)");
    expect(waiting).toHaveTextContent("calls home by itself");
    expect(await within(waiting).findByText(/adoption port \(lan4\)/)).toBeInTheDocument();
    await user.click(within(waiting).getByRole("button", { name: "Stop waiting" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "DELETE",
        url: "/api/v1/ztp/approvals/02%3A00%3A00%3A00%3A00%3A01",
        body: undefined,
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("region", { name: "Waiting for its device" })).toBeNull(),
    );
  });
});
