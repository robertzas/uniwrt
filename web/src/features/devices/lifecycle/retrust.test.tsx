import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Job } from "../../../api/gen/Job";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { detail, facts, summary } from "../../../test/fixtures";
import { NO_PACKAGES } from "../../../test/packages";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import { revisionMeta, siteConfig, siteView } from "../../../test/site";
import { readopted } from "./RetrustDialog";

let calls: { method: string; url: string; body?: unknown }[] = [];
let presents = "SHA256:new";

const STEPS: Job["steps"][number]["name"][] = [
  "connect",
  "check",
  "install_key",
  "verify_key",
  "set_password",
  "inventory",
  "backup",
  "record",
  "watch",
];

function job(state: Job["state"], extra: Partial<Job> = {}, installed = false): Job {
  return {
    id: 40,
    kind: "retrust",
    target: "gw",
    node: 1,
    revision: null,
    state,
    steps: STEPS.map((name) => ({
      name,
      state:
        state === "running"
          ? "pending"
          : ["install_key", "verify_key", "set_password", "inventory", "backup"].includes(name) &&
              !installed
            ? "skipped"
            : "done",
      detail: null,
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

beforeEach(() => {
  calls = [];
  presents = "SHA256:new";
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = typeof init?.body === "string" ? JSON.parse(init.body) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/adopt/probe") {
        return Response.json({
          host: body.host,
          port: body.port,
          host_key: "ssh-ed25519 AAAA",
          fingerprint: presents,
          algorithm: "ED25519",
        });
      }
      if (url === "/api/v1/nodes/1/host-key") return Response.json(job("running"), { status: 202 });
      if (url === "/api/v1/site") return Response.json(siteView(2, siteConfig()));
      if (url === "/api/v1/ztp/pending") return Response.json({ pending: [], approvals: [] });
      if (url === "/api/v1/firmware") return Response.json({ nodes: [], error: null });
      if (url === "/api/v1/nodes/1/packages") return Response.json(NO_PACKAGES);
      if (url === "/api/v1/nodes/1/backups") return Response.json([]);
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderQuarantined(role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/devices/1?tab=config"));
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
  sendFleet(ws, [summary(1, "gw", { role: "gateway" })]);
  const node = detail(facts(1));
  node.status = {
    state: "offline",
    since: 1,
    reasons: [{ kind: "host_key_mismatch", presented: "SHA256:new", pinned: "SHA256:old" }],
  };
  ws.receive({ type: "node_snapshot", topic: "node:1", node });
  ws.receive({ type: "config", topic: "config", head: revisionMeta(2), applied: [] });
  await screen.findByRole("heading", { name: "gw" });
  return { ws, node: node.node };
}

const open = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.click(await screen.findByRole("button", { name: "Check the new key…" }));
  return screen.findByRole("dialog", { name: "Trust gw's new host key?" });
};

describe("Device › a changed host key", () => {
  it("tells re-adoption from a kept device by the key's install", () => {
    expect(readopted(job("succeeded", {}, true))).toBe(true);
    expect(readopted(job("succeeded"))).toBe(false);
  });

  it("is for admins to trust", async () => {
    await renderQuarantined("viewer");
    expect(await screen.findByText(/Its SSH host key changed/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Check the new key…" })).toBeNull();
  });

  it("reads the key again beside the pinned one, then trusts it", async () => {
    const user = userEvent.setup();
    const { ws, node } = await renderQuarantined();
    const dialog = await open(user);
    expect(await within(dialog).findByTestId("fingerprint")).toHaveTextContent("SHA256:new");
    expect(within(dialog).getByTestId("pinned")).toHaveTextContent("SHA256:old");
    expect(within(dialog).queryByRole("alert")).toBeNull();
    expect(calls).toContainEqual({
      method: "POST",
      url: "/api/v1/adopt/probe",
      body: { host: node.host, port: node.port },
    });
    await user.click(within(dialog).getByRole("button", { name: "It matches, trust it" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "POST",
        url: "/api/v1/nodes/1/host-key",
        body: { host_key: "ssh-ed25519 AAAA" },
      }),
    );
    expect(await within(dialog).findByText("Trusting gw's new host key")).toBeInTheDocument();
    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [job("succeeded")] });
    expect(await within(dialog).findByText(/Trusted\. uniwrt reconnects/)).toBeInTheDocument();
  });

  it("warns when the device presents yet another key", async () => {
    presents = "SHA256:third";
    const user = userEvent.setup();
    await renderQuarantined();
    const dialog = await open(user);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "This is not the key it presented",
    );
  });

  it("asks a reset device's root password, then adopts it back", async () => {
    const user = userEvent.setup();
    const { ws } = await renderQuarantined();
    const dialog = await open(user);
    await user.click(await within(dialog).findByRole("button", { name: "It matches, trust it" }));
    await within(dialog).findByText("Trusting gw's new host key");
    const refused = job("failed", {
      error: { code: "auth_rejected", message: "root@10.0.0.1 rejected the login" },
    });
    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [refused] });
    await user.click(await within(dialog).findByRole("button", { name: "Change the password" }));
    expect(
      await within(dialog).findByText("The device no longer trusts uniwrt"),
    ).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText("Root password"), "s3cret");
    await user.click(within(dialog).getByRole("button", { name: "Adopt it back" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "POST",
        url: "/api/v1/nodes/1/host-key",
        body: { host_key: "ssh-ed25519 AAAA", password: "s3cret" },
      }),
    );
    await within(dialog).findByText("Trusting gw's new host key");
    ws.receive({
      type: "jobs_delta",
      topic: "jobs",
      jobs: [job("succeeded", { id: 41 }, true), job("succeeded", {}, true)],
    });
    expect(await within(dialog).findByText(/Adopted back in observe mode/)).toBeInTheDocument();
  });

  it("closes on another device, whose message says to replace it", async () => {
    const user = userEvent.setup();
    const { ws } = await renderQuarantined();
    const dialog = await open(user);
    await user.click(await within(dialog).findByRole("button", { name: "It matches, trust it" }));
    await within(dialog).findByText("Trusting gw's new host key");
    const other = job("failed", {
      error: {
        code: "identity_mismatch",
        message: "This is not gw's device. Replace its device instead",
      },
    });
    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [other] });
    expect(await within(dialog).findByText(/Replace its device instead/)).toBeInTheDocument();
    await user.click(within(dialog).getAllByRole("button", { name: "Close" })[0] as HTMLElement);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});
