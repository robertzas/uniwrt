import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { FirmwareCatalog } from "../../api/gen/FirmwareCatalog";
import type { Job } from "../../api/gen/Job";
import type { NodeFirmware } from "../../api/gen/NodeFirmware";
import type { Rollout } from "../../api/gen/Rollout";
import type { RolloutNode } from "../../api/gen/RolloutNode";
import type { UpgradePlan } from "../../api/gen/UpgradePlan";
import { createAppRouter } from "../../app/routes";
import { controllerUpdate } from "../../test/controller";
import { summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";

const MB = 1024 * 1024;

const offers = [
  { release: "24.10.8", kind: "update", notes: "", profile: "p", image_bytes: 10 * MB },
  { release: "25.12.6", kind: "branch", notes: "", profile: "p", image_bytes: 10 * MB },
] as const;

const entry = (node: number, extra: Partial<NodeFirmware> = {}): NodeFirmware => ({
  node,
  distribution: "OpenWrt",
  version: "24.10.7",
  revision: "r1",
  target: "mediatek/filogic",
  board: "glinet,gl-mt3000",
  offers: [...offers],
  note: null,
  uploads: [],
  source: { kind: "site" },
  repo: null,
  releases: [],
  ...extra,
});

/** Gateway (1), Office (2) and Lab (3) can upgrade; Attic (4) is in observe mode. */
const catalog: FirmwareCatalog = {
  nodes: [entry(1), entry(2), entry(3), entry(4)],
  uploads: [],
  error: null,
  sources: { repo: null, prereleases: false, token: false },
};

const plan = (node: number, release: string, source: UpgradePlan["source"]): UpgradePlan => ({
  node,
  source,
  from: "24.10.7",
  release,
  target: "mediatek/filogic",
  profile: "glinet_gl-mt3000",
  packages: ["luci"],
  kept: ["luci", "wpad-mbedtls"],
  renamed: [],
  added: [],
  dropped: node === 3 ? ["my-tool"] : [],
  removed: [],
  packages_at: Date.now(),
  image_bytes: 10 * MB,
  tmp_free_bytes: 200 * MB,
  images: [],
  upload: null,
  repo: null,
});

const node = (id: number, name: string, extra: Partial<RolloutNode> = {}): RolloutNode => ({
  node: id,
  name,
  choice: { source: "asu", release: "25.12.6" },
  from: "24.10.7",
  state: "pending",
  job: null,
  detail: null,
  ...extra,
});

const rollout = (extra: Partial<Rollout> = {}): Rollout => ({
  id: 7,
  state: "running",
  nodes: [node(3, "Lab"), node(2, "Office"), node(1, "Gateway")],
  canary: { soak_min: 60 },
  window: null,
  waiting: null,
  error: null,
  created_by: 1,
  started_at: Date.UTC(2026, 10, 17, 3, 30),
  finished_at: null,
  ...extra,
});

function upgradeJob(state: Job["state"]): Job {
  return {
    id: 31,
    kind: "upgrade",
    target: "Lab to 25.12.6",
    node: 3,
    revision: null,
    state,
    steps: ["connect", "check", "backup", "flash"].map((name, i) => ({
      name,
      state: i < 2 ? "done" : i === 2 && state === "running" ? "running" : "pending",
      detail: null,
      started_at: null,
      finished_at: null,
    })),
    nodes: [],
    error: state === "failed" ? { code: "not_back", message: "Lab didn't come back" } : null,
    created_by: 1,
    started_at: 1,
    finished_at: state === "running" ? null : 2,
  } as Job;
}

type Route = (body: unknown) => unknown;

function stubApi(routes: Record<string, Route>) {
  const calls: [string, unknown][] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const key = `${init?.method ?? "GET"} ${url}`;
      const body = typeof init?.body === "string" ? JSON.parse(init.body) : init?.body;
      calls.push([key, body]);
      const route = routes[key];
      if (route) return Response.json(route(body));
      if (key === "GET /api/v1/controller/update") {
        return Response.json(controllerUpdate({ offer: null }));
      }
      if (key === "GET /api/v1/firmware") return Response.json(catalog);
      if (key === "GET /api/v1/firmware/rollouts") return Response.json([]);
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
  return calls;
}

function renderUpdates(role: "admin" | "viewer" = "admin", jobs: Job[] = []) {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/updates"));
  render(() => (
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => props.children}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [
    summary(1, "Gateway", { role: "gateway", mode: "managed" }),
    summary(2, "Office", { mode: "managed" }),
    summary(3, "Lab", { mode: "managed" }),
    summary(4, "Attic"),
  ]);
  ws.receive({ type: "jobs_snapshot", topic: "jobs", jobs });
  return ws;
}

const card = async () =>
  screen.findByRole("region", { name: "Rolling upgrades" }, { timeout: 2000 });

describe("Rolling upgrades", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("chooses devices and images, shows every plan and the order, then starts", async () => {
    const user = userEvent.setup();
    const calls = stubApi({
      "POST /api/v1/firmware/nodes/1/plan": (b) =>
        plan(1, (b as { release: string }).release, "asu"),
      "POST /api/v1/firmware/nodes/2/plan": (b) =>
        plan(2, (b as { release: string }).release, (b as { source: "asu" }).source),
      "POST /api/v1/firmware/nodes/3/plan": (b) =>
        plan(3, (b as { release: string }).release, "asu"),
      "POST /api/v1/firmware/rollouts": () => rollout(),
    });
    renderUpdates();
    const start = await within(await card()).findByRole("button", { name: "Roll out upgrades" });
    await waitFor(() => expect((start as HTMLButtonElement).disabled).toBe(false));
    await user.click(start);
    const dialog = await screen.findByRole("dialog", { name: "Roll out upgrades" });

    // The devices that can upgrade are chosen, each on its newest release; observe mode says why.
    expect(
      within(dialog).getByText("In observe mode: turn its management on to upgrade it."),
    ).toBeTruthy();
    const attic = within(dialog).getByRole("checkbox", { name: /Attic/ }) as HTMLInputElement;
    expect(attic.disabled).toBe(true);
    const office = within(dialog).getByRole("combobox", {
      name: "Image for Office",
    }) as HTMLSelectElement;
    expect(office.value).toBe("asu:25.12.6");
    // Every plan is read and summed up; a dropped package warns.
    await within(dialog).findByText("2 of your packages carried over · 1 left out");
    expect(calls).toContainEqual([
      "POST /api/v1/firmware/nodes/2/plan",
      { source: "asu", release: "25.12.6" },
    ]);
    // The Office gets the official image instead: planned again.
    await user.selectOptions(office, "official:25.12.6");
    await waitFor(() =>
      expect(calls).toContainEqual([
        "POST /api/v1/firmware/nodes/2/plan",
        { source: "official", release: "25.12.6" },
      ]),
    );

    // The order: the canary (on by default) first, the gateway last.
    const order = within(dialog).getByRole("region", { name: "Order" });
    expect(
      within(order)
        .getAllByRole("listitem")
        .map((li) => li.textContent),
    ).toEqual(["1LabCanary", "2Office", "3Gateway"]);
    await user.selectOptions(within(dialog).getByRole("combobox", { name: "Canary device" }), "2");
    expect(within(order).getAllByRole("listitem")[0]?.textContent).toBe("1OfficeCanary");
    await user.selectOptions(within(dialog).getByRole("combobox", { name: "Watch it for" }), "120");

    // The window is off until asked for, then 03:30–05:30.
    expect(within(dialog).queryByRole("combobox", { name: "From" })).toBeNull();
    await user.click(within(dialog).getByRole("checkbox", { name: /Maintenance window/ }));
    expect(
      (within(dialog).getByRole("combobox", { name: "From" }) as HTMLSelectElement).value,
    ).toBe("210");
    await user.selectOptions(within(dialog).getByRole("combobox", { name: "To" }), "210");
    expect(
      within(dialog).getByText("The window must open and close at different times."),
    ).toBeTruthy();
    const upgrade = within(dialog).getByRole("button", {
      name: "Upgrade 3 devices",
    }) as HTMLButtonElement;
    expect(upgrade.disabled).toBe(true);
    await user.selectOptions(within(dialog).getByRole("combobox", { name: "To" }), "330");

    // Start asks first.
    await waitFor(() => expect(upgrade.disabled).toBe(false));
    await user.click(upgrade);
    expect(within(dialog).getByText(/^Each of the 3 devices restarts in turn/)).toBeTruthy();
    await user.click(within(dialog).getByRole("button", { name: "Start now" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "Roll out upgrades" })).toBeNull(),
    );
    expect(calls).toContainEqual([
      "POST /api/v1/firmware/rollouts",
      {
        nodes: [
          { node: 1, choice: { source: "asu", release: "25.12.6" } },
          { node: 3, choice: { source: "asu", release: "25.12.6" } },
          { node: 2, choice: { source: "official", release: "25.12.6" } },
        ],
        canary: { node: 2, soak_min: 120 },
        window: { start_min: 210, end_min: 330 },
      },
    ]);
    // The pipeline shows the controller's order; another can't start meanwhile.
    const pipeline = await within(await card()).findByRole("region", { name: "Rollout #7" });
    expect(
      within(pipeline)
        .getAllByRole("listitem")
        .map((li) => li.getAttribute("aria-label")),
    ).toEqual(["Lab", "Office", "Gateway"]);
    expect(
      (within(await card()).getByRole("button", { name: "Roll out upgrades" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });

  it("follows the running rollout, its node's job, and the halt with its recovery", async () => {
    const user = userEvent.setup();
    const halted = rollout({
      state: "halted",
      error: "Lab's upgrade failed: Lab didn't come back",
      nodes: [node(3, "Lab", { state: "failed", job: 31 }), node(2, "Office"), node(1, "Gateway")],
    });
    const calls = stubApi({
      "GET /api/v1/firmware/rollouts": () => [
        rollout({
          nodes: [
            node(3, "Lab", { state: "upgrading", job: 31 }),
            node(2, "Office"),
            node(1, "Gateway"),
          ],
        }),
        rollout({ id: 6, state: "done", finished_at: Date.UTC(2026, 10, 16, 4) }),
      ],
      "GET /api/v1/firmware/rollouts/7": () => halted,
      "POST /api/v1/firmware/rollouts/7/nodes/3/skip": () =>
        rollout({
          nodes: [node(3, "Lab", { state: "skipped" }), node(2, "Office"), node(1, "Gateway")],
        }),
    });
    const ws = renderUpdates("admin", [upgradeJob("running")]);
    const pipeline = await within(await card()).findByRole("region", { name: "Rollout #7" });
    const lab = within(pipeline).getByRole("listitem", { name: "Lab" });
    expect(within(lab).getByText("Upgrading")).toBeTruthy();
    expect(within(lab).getByText("Canary")).toBeTruthy();
    expect(within(lab).getByRole("list", { name: /Lab to 25.12.6/ })).toBeTruthy();
    expect(within(pipeline).getByRole("button", { name: "Pause" })).toBeTruthy();
    expect(within(pipeline).getByText(/pause, and cancel once it is done/)).toBeTruthy();
    // History keeps the finished one.
    const history = within(await card()).getByRole("region", { name: "History" });
    expect(within(history).getByText("Rollout #6")).toBeTruthy();

    // The next read finds it halted on Lab, its job failed.
    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [upgradeJob("failed")] });
    const alert = await within(pipeline).findByText("The rollout stopped", {}, { timeout: 5000 });
    expect(alert.parentElement?.textContent).toContain("Resume tries Lab again");
    expect(within(pipeline).getByText("Lab didn't come back")).toBeTruthy();

    // Skip asks first.
    await user.click(within(pipeline).getByRole("button", { name: "Skip Lab" }));
    expect(within(pipeline).getByText(/Leave Lab out of the rollout\?/)).toBeTruthy();
    const confirm = within(pipeline).getAllByRole("button", { name: "Skip Lab" });
    await user.click(confirm[confirm.length - 1] as HTMLElement);
    await waitFor(() =>
      expect(calls.map(([key]) => key)).toContain("POST /api/v1/firmware/rollouts/7/nodes/3/skip"),
    );
    await within(pipeline).findByText("Skipped");
  });

  it("shows viewers the pipeline without actions", async () => {
    stubApi({
      "GET /api/v1/firmware/rollouts": () => [
        rollout({ waiting: { kind: "window", opens: Date.UTC(2026, 10, 18, 2, 30) } }),
      ],
    });
    renderUpdates("viewer");
    const pipeline = await within(await card()).findByRole("region", { name: "Rollout #7" });
    expect(within(pipeline).getByText(/Waiting for the maintenance window/)).toBeTruthy();
    expect(within(pipeline).queryByRole("button")).toBeNull();
    expect(within(await card()).queryByRole("button", { name: "Roll out upgrades" })).toBeNull();
  });
});
