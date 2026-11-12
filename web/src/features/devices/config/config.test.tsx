import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Job } from "../../../api/gen/Job";
import type { SiteSave } from "../../../api/gen/SiteSave";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { drift } from "../../../test/drift";
import { detail, facts } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders } from "../../../test/providers";
import { revisionMeta, siteView } from "../../../test/site";

let calls: { method: string; url: string; body: unknown }[] = [];
let rootPassword: string | null = null;
let head = 2;

function manageJob(state: Job["state"], done: number): Job {
  return stepsJob("manage", ["connect", "install_tool", "record"], state, done);
}

function stepsJob(
  kind: Job["kind"],
  names: Job["steps"][number]["name"][],
  state: Job["state"],
  done: number,
): Job {
  return {
    id: kind === "wpad" ? 10 : 9,
    kind,
    target: "gw",
    node: 1,
    revision: null,
    state,
    steps: names.map((name, i) => ({
      name,
      state: i < done ? "done" : i === done && state === "running" ? "running" : "pending",
      detail: null,
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

const WPAD_STEPS = ["connect", "check", "fetch", "swap", "verify"] as const;

function wpadJob(state: Job["state"], done: number): Job {
  return stepsJob("wpad", [...WPAD_STEPS], state, done);
}

/** The device with a hostapd: managed or not, with 802.11v or not. */
function withWifi(managed: boolean, wnm: boolean) {
  const node = detail(facts(1));
  node.node.mode = managed ? "managed" : "observe";
  if (node.facts) {
    node.facts.capabilities = {
      package_manager: "apk",
      switch: null,
      wifi: wnm ? ["11k", "11r", "11v"] : ["11k", "11r"],
      software: [],
    };
  }
  return node;
}

beforeEach(() => {
  calls = [];
  head = 2;
  rootPassword = "abcd-efgh-jkmn-pqrs-tuvw-xyz2";
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
      if (url === "/api/v1/nodes/1/management") {
        return Response.json(manageJob("running", 0), { status: 202 });
      }
      if (url === "/api/v1/nodes/1/role") return Response.json(body);
      if (url === "/api/v1/nodes/1/wpad")
        return Response.json(wpadJob("running", 2), { status: 202 });
      if (url === "/api/v1/nodes/1/root-password/reveal") {
        return rootPassword === null
          ? Response.json({ title: "Not found", status: 404 }, { status: 404 })
          : Response.json({ value: rootPassword });
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderConfig(role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/devices/1?tab=config"));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  // Admins' top bar follows the applies on `jobs`.
  const topics = ["fleet", "alerts", "config", "node:1", ...(role === "admin" ? ["jobs"] : [])];
  await waitFor(() => {
    const sent = ws.sent.at(-1) as { sub?: string[] } | undefined;
    expect(new Set(sent?.sub)).toEqual(new Set(topics));
  });
  ws.receive({ type: "node_snapshot", topic: "node:1", node: detail(facts(1)) });
  ws.receive({
    type: "config",
    topic: "config",
    head: revisionMeta(head),
    applied: [{ node: 1, revision: 1, job: 3, at: 0 }],
  });
  await screen.findByRole("heading", { name: "gw" });
  return ws;
}

describe("Device › Config", () => {
  it("turns management on after asking, and follows the job", async () => {
    const user = userEvent.setup();
    const ws = await renderConfig();
    expect(screen.getByRole("tab", { name: "Config", selected: true })).toBeInTheDocument();
    expect(await screen.findByText(/^Observe mode:/)).toBeInTheDocument();
    expect(screen.getByText("Applies leave this device alone.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Turn management on" }));
    const dialog = await screen.findByRole("dialog", { name: "Turn management on for gw?" });
    await user.click(within(dialog).getByRole("button", { name: "Turn on" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "PUT",
        url: "/api/v1/nodes/1/management",
        body: { enabled: true },
      }),
    );
    const progress = await screen.findByRole("list", { name: /gw/ });
    expect(progress).toHaveTextContent("Install the transaction tool");
    expect(screen.getByRole("button", { name: "Turn management on" })).toBeDisabled();

    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [manageJob("succeeded", 3)] });
    const managed = detail(facts(1));
    managed.node.mode = "managed";
    ws.receive({ type: "node_snapshot", topic: "node:1", node: managed });
    expect(await screen.findByText("Management is on")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Turn management off" })).toBeEnabled();
    expect(screen.getByText("Holds revision 1; the latest is 2.")).toBeInTheDocument();
  });

  it("makes the gateway an access point after saying what the apply changes", async () => {
    const user = userEvent.setup();
    const ws = await renderConfig();
    expect(await screen.findByText(/^The site's router:/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Change role" }));
    const dialog = await screen.findByRole("dialog", { name: "Change the role of gw" });
    const save = within(dialog).getByRole("button", { name: "Change role" });
    expect(save).toBeDisabled();
    await user.click(within(dialog).getByRole("radio", { name: /^Access point/ }));
    expect(dialog).toHaveTextContent("its DHCP server, firewall and WAN go");
    await user.click(save);
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "PUT",
        url: "/api/v1/nodes/1/role",
        body: { role: "ap" },
      }),
    );
    expect(await screen.findByText("gw is now: Access point")).toBeInTheDocument();
    const ap = detail(facts(1));
    ap.node.role = "ap";
    ws.receive({ type: "node_snapshot", topic: "node:1", node: ap });
    expect(await screen.findByText(/^A dumb access point:/)).toBeInTheDocument();
  });

  it("keeps a managed gateway's role, and says why", async () => {
    const user = userEvent.setup();
    const ws = await renderConfig();
    const managed = detail(facts(1));
    managed.node.mode = "managed";
    ws.receive({ type: "node_snapshot", topic: "node:1", node: managed });
    await screen.findByRole("button", { name: "Turn management off" });
    await user.click(screen.getByRole("button", { name: "Change role" }));
    const dialog = await screen.findByRole("dialog", { name: "Change the role of gw" });
    await user.click(within(dialog).getByRole("radio", { name: /^Access point/ }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      "The site's routing runs on this device.",
    );
    expect(within(dialog).getByRole("button", { name: "Change role" })).toBeDisabled();
    expect(calls.some((c) => c.url === "/api/v1/nodes/1/role")).toBe(false);
  });

  it("swaps the basic wpad for the full one after asking, and follows the job", async () => {
    const user = userEvent.setup();
    const ws = await renderConfig();
    ws.receive({ type: "node_snapshot", topic: "node:1", node: withWifi(false, false) });
    expect(await screen.findByText("Basic wpad")).toBeInTheDocument();
    // Observe mode: nothing to install yet.
    expect(screen.getByRole("button", { name: "Install the full wpad" })).toBeDisabled();
    expect(screen.getByText("Turn management on to install the full wpad.")).toBeVisible();

    ws.receive({ type: "node_snapshot", topic: "node:1", node: withWifi(true, false) });
    const install = screen.getByRole("button", { name: "Install the full wpad" });
    await waitFor(() => expect(install).toBeEnabled());
    await user.click(install);
    const dialog = await screen.findByRole("dialog", { name: "Install the full wpad on gw?" });
    expect(dialog).toHaveTextContent("Its WLANs get 802.11v at the next apply.");
    await user.click(within(dialog).getByRole("button", { name: "Install" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "POST",
        url: "/api/v1/nodes/1/wpad",
        body: undefined,
      }),
    );
    const progress = await screen.findByRole("list", { name: "Installing the full wpad on gw" });
    expect(progress).toHaveTextContent("Download the packages");
    expect(progress).toHaveTextContent("Find 802.11v");
    expect(install).toBeDisabled();

    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [wpadJob("succeeded", 5)] });
    ws.receive({ type: "node_snapshot", topic: "node:1", node: withWifi(true, true) });
    expect(await screen.findByText("The full wpad is installed")).toBeInTheDocument();
    expect(await screen.findByText("Full wpad")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Install the full wpad" })).toBeNull();
  });

  it("saves the chosen packages as the next revision", async () => {
    const user = userEvent.setup();
    await renderConfig();
    const network = await screen.findByRole("checkbox", { name: "network" });
    // The gateway's features are choosable on the gateway; WireGuard comes later.
    expect(screen.getByRole("checkbox", { name: "sqm" })).toBeEnabled();
    expect(screen.getByRole("checkbox", { name: "upnpd" })).toBeEnabled();
    expect(screen.getByRole("checkbox", { name: "wireguard" })).toBeDisabled();
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeDisabled();
    await user.click(network);
    await user.click(screen.getByRole("checkbox", { name: "dhcp" }));
    expect(screen.getByText("Your choice takes effect once management is on.")).toBeVisible();
    await user.click(save);
    expect(await screen.findByText("Saved as revision 3")).toBeInTheDocument();
    const put = calls.find((call) => call.method === "PUT" && call.url === "/api/v1/site");
    const sent = put?.body as SiteSave;
    expect(sent.base).toBe(2);
    expect(sent.message).toBe("Manage network and dhcp on gw");
    expect(sent.config.nodes).toEqual([
      { node: 1, hostname: null, managed: ["network", "dhcp"], radios: [], ports: [], raw: [] },
    ]);
  });

  it("shows admins the root password the vault keeps, on demand", async () => {
    const user = userEvent.setup();
    await renderConfig();
    await user.click(await screen.findByRole("button", { name: "Show root password" }));
    const shown = await screen.findByRole("status", { name: "Root password" });
    expect(shown).toHaveTextContent("abcd-efgh-jkmn-pqrs-tuvw-xyz2");
    expect(calls).toContainEqual(
      expect.objectContaining({ method: "POST", url: "/api/v1/nodes/1/root-password/reveal" }),
    );
    await user.click(screen.getByRole("button", { name: "Hide" }));
    expect(screen.queryByRole("status", { name: "Root password" })).toBeNull();
  });

  it("says so when the vault keeps no root password", async () => {
    rootPassword = null;
    const user = userEvent.setup();
    await renderConfig();
    await user.click(await screen.findByRole("button", { name: "Show root password" }));
    expect(await screen.findByText(/The vault keeps no root password/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Show root password" })).toBeNull();
  });

  it("shows viewers the state without the switches", async () => {
    await renderConfig("viewer");
    expect(screen.queryByRole("button", { name: "Show root password" })).toBeNull();
    expect(await screen.findByText("Only admins can turn management on or off.")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Turn management on" })).toBeNull();
    expect(await screen.findByRole("checkbox", { name: "network" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
  });

  it("shows a managed device's drift, and acknowledges, mutes, watches and imports", async () => {
    const user = userEvent.setup();
    const ws = await renderConfig();
    const managed = detail(facts(1));
    managed.node.mode = "managed";
    ws.receive({ type: "node_snapshot", topic: "node:1", node: managed });
    expect(
      await screen.findByText("No drift: the managed packages are as uniwrt confirmed them."),
    ).toBeVisible();
    ws.receive({
      type: "config",
      topic: "config",
      head: revisionMeta(head),
      applied: [{ node: 1, revision: 2, job: 3, at: 0 }],
      drift: {
        drifts: [drift(1, "system")],
        muted: [{ node: 1, package: "dhcp", at: Date.now() - 120_000, by: "ada" }],
      },
    });
    const item = await screen.findByRole("region", { name: "Drift of system" });
    expect(item).toHaveTextContent("Drifted");
    expect(item).toHaveTextContent("after revision 2 confirmed it");
    expect(within(item).getByRole("button", { name: "The system package" })).toBeVisible();
    expect(item).toHaveTextContent("log_size");
    expect(screen.getByText(/^dhcp, muted by ada/)).toBeVisible();
    expect(screen.getByRole("button", { name: /^Drift: 1 package changed/ })).toBeVisible();

    await user.click(within(item).getByRole("button", { name: "Acknowledge" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "POST",
        url: "/api/v1/nodes/1/drift/system/acknowledge",
        body: undefined,
      }),
    );
    await user.click(within(item).getByRole("button", { name: "Mute" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "PUT",
        url: "/api/v1/nodes/1/drift/system/mute",
        body: { muted: true },
      }),
    );
    await user.click(screen.getByRole("button", { name: "Watch again: dhcp" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "PUT",
        url: "/api/v1/nodes/1/drift/dhcp/mute",
        body: { muted: false },
      }),
    );

    await user.click(within(item).getByRole("button", { name: "Import" }));
    expect(
      await screen.findByRole("heading", { name: /^Import the drift of system on / }),
    ).toBeVisible();
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "POST",
        url: "/api/v1/import/preview",
        body: { drift: { node: 1, package: "system" } },
      }),
    );
  });

  it("shows viewers the drift without its actions", async () => {
    const ws = await renderConfig("viewer");
    const managed = detail(facts(1));
    managed.node.mode = "managed";
    ws.receive({ type: "node_snapshot", topic: "node:1", node: managed });
    ws.receive({
      type: "config",
      topic: "config",
      head: revisionMeta(head),
      applied: [{ node: 1, revision: 2, job: 3, at: 0 }],
      drift: {
        drifts: [drift(1, "system", { acknowledged: { at: Date.now(), by: null } })],
        muted: [],
      },
    });
    const item = await screen.findByRole("region", { name: "Drift of system" });
    expect(item).toHaveTextContent("Acknowledged by a removed user");
    expect(within(item).queryByRole("button", { name: "Import" })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Drift:/ })).toBeNull();
  });
});
