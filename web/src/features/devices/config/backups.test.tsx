import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Backup } from "../../../api/gen/Backup";
import type { Job } from "../../../api/gen/Job";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { formatDateTime } from "../../../lib/format";
import { detail, facts } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders } from "../../../test/providers";
import { revisionMeta, siteView } from "../../../test/site";
import { backupDetail, concerns, endedCount } from "./backups";

const DAY = 86_400_000;
const T0 = Date.UTC(2026, 10, 15, 3, 0);

let calls: { method: string; url: string }[] = [];
let backups: Backup[] = [];

function backup(id: number, reason: Backup["reason"], taken: number, seen = taken): Backup {
  return {
    id,
    node: 1,
    taken_at: taken,
    seen_at: seen,
    reason,
    size_bytes: 12_000,
    sha256: "ab".repeat(32),
  };
}

function job(kind: "backup" | "restore", state: Job["state"], nodes: number[] = []): Job {
  const names: Job["steps"][number]["name"][] =
    kind === "backup"
      ? ["backup"]
      : ["connect", "check", "backup", "stage", "apply", "verify", "confirm"];
  return {
    id: kind === "backup" ? 20 : 21,
    kind,
    target: "gw",
    node: nodes.length > 1 ? null : 1,
    revision: null,
    state,
    steps: names.map((name, i) => ({
      name,
      state: state === "running" ? (i === 0 ? "running" : "pending") : "done",
      detail: null,
      started_at: null,
      finished_at: null,
    })),
    nodes: nodes.map((node) => ({ node, name: `n${node}`, state: "pending", detail: null })),
    error: null,
    created_by: 1,
    started_at: 1,
    finished_at: state === "running" ? null : 2,
  };
}

beforeEach(() => {
  calls = [];
  backups = [backup(2, "scheduled", T0, T0 + 7 * DAY), backup(1, "adoption", T0 - 30 * DAY)];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      calls.push({ method, url });
      if (url === "/api/v1/site") return Response.json(siteView(2));
      if (url === "/api/v1/firmware") return Response.json({ nodes: [], error: null });
      if (url === "/api/v1/nodes/1/backups" && method === "GET") return Response.json(backups);
      if (url === "/api/v1/nodes/1/backups") {
        return Response.json(job("backup", "running"), { status: 202 });
      }
      if (url === "/api/v1/nodes/1/backups/2/restore") {
        return Response.json(job("restore", "running"), { status: 202 });
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderBackups(options: { role?: "admin" | "viewer"; managed?: boolean } = {}) {
  const role = options.role ?? "admin";
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/devices/1?tab=config"));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  const topics = ["fleet", "alerts", "config", "node:1", ...(role === "admin" ? ["jobs"] : [])];
  await waitFor(() => {
    const sent = ws.sent.at(-1) as { sub?: string[] } | undefined;
    expect(new Set(sent?.sub)).toEqual(new Set(topics));
  });
  const node = detail(facts(1));
  if (options.managed) node.node.mode = "managed";
  ws.receive({ type: "node_snapshot", topic: "node:1", node });
  ws.receive({ type: "config", topic: "config", head: revisionMeta(2), applied: [] });
  await screen.findByRole("heading", { name: "gw" });
  const card = await screen.findByRole("region", { name: "Backups" });
  await within(card).findByText(`Taken ${formatDateTime(T0)}`);
  return { ws, card };
}

describe("backups", () => {
  it("knows the jobs that touch a node, and counts those seen ending", () => {
    expect(concerns(job("backup", "running"), 1)).toBe(true);
    expect(concerns(job("backup", "running", [2, 3]), 1)).toBe(false);
    expect(concerns(job("backup", "running", [1, 3]), 1)).toBe(true);
    expect(concerns({ ...job("restore", "running"), node: 2 }, 1)).toBe(false);
    const seen = new Set<number>();
    // A job the snapshot brings already ended doesn't count.
    expect(endedCount([job("restore", "succeeded")], 1, seen)).toBe(0);
    expect(endedCount([job("backup", "running")], 1, seen)).toBe(0);
    expect(endedCount([job("backup", "failed")], 1, seen)).toBe(1);
  });

  it("says why a backup was taken, its size and since when it is unchanged", () => {
    expect(backupDetail(backup(1, "adoption", T0))).toBe("At adoption · 12 KB");
    expect(backupDetail(backup(2, "scheduled", T0, T0 + DAY))).toBe(
      `Weekly · 12 KB · unchanged as of ${formatDateTime(T0 + DAY)}`,
    );
  });
});

describe("Device › Config › Backups", () => {
  it("lists the backups, downloads one and backs up now, reading the list again after", async () => {
    const user = userEvent.setup();
    const { ws, card } = await renderBackups();
    const rows = within(card).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent(`unchanged as of ${formatDateTime(T0 + 7 * DAY)}`);
    expect(rows[1]).toHaveTextContent("At adoption");
    expect(
      within(card).getByRole("link", {
        name: `Download the backup taken ${formatDateTime(T0)}`,
      }),
    ).toHaveAttribute("href", "/api/v1/nodes/1/backups/2/archive");
    // Observe mode: nothing to restore onto.
    expect(within(card).queryByRole("button", { name: /^Restore/ })).toBeNull();
    expect(within(card).getByText(/^Turn management on to restore/)).toBeInTheDocument();

    await user.click(within(card).getByRole("button", { name: "Back up now" }));
    expect(await within(card).findByRole("list", { name: "Backing up gw" })).toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Back up now" })).toBeDisabled();

    backups = [backup(3, "manual", T0 + 8 * DAY), ...backups];
    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [job("backup", "succeeded")] });
    expect(await screen.findByText("gw is backed up")).toBeInTheDocument();
    await within(card).findByText(`Taken ${formatDateTime(T0 + 8 * DAY)}`);
    expect(within(card).getByRole("button", { name: "Back up now" })).toBeEnabled();
    expect(calls.filter((call) => call.url === "/api/v1/nodes/1/backups")).toEqual([
      { method: "GET", url: "/api/v1/nodes/1/backups" },
      { method: "POST", url: "/api/v1/nodes/1/backups" },
      { method: "GET", url: "/api/v1/nodes/1/backups" },
    ]);
  });

  it("restores a backup onto a managed device after saying what comes back", async () => {
    const user = userEvent.setup();
    const { card } = await renderBackups({ managed: true });
    await user.click(
      within(card).getByRole("button", {
        name: `Restore the backup taken ${formatDateTime(T0)}`,
      }),
    );
    const dialog = await screen.findByRole("dialog", { name: "Restore a backup onto gw?" });
    expect(dialog).toHaveTextContent("(/etc/config)");
    expect(dialog).toHaveTextContent("are not restored");
    expect(dialog).toHaveTextContent("until the next apply");
    await user.click(within(dialog).getByRole("button", { name: "Restore" }));
    await waitFor(() =>
      expect(calls).toContainEqual({ method: "POST", url: "/api/v1/nodes/1/backups/2/restore" }),
    );
    const progress = await within(card).findByRole("list", { name: "Restoring gw" });
    expect(progress).toHaveTextContent("Stage the backup's configuration");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("shows viewers the backups without their actions", async () => {
    const { card } = await renderBackups({ role: "viewer", managed: true });
    expect(within(card).getAllByRole("listitem")).toHaveLength(2);
    expect(within(card).queryByRole("button", { name: "Back up now" })).toBeNull();
    expect(within(card).queryByRole("link")).toBeNull();
    expect(within(card).queryByRole("button", { name: /^Restore/ })).toBeNull();
    expect(within(card).getByText(/^Only admins back up/)).toBeInTheDocument();
  });
});
