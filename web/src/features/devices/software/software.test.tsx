import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Job } from "../../../api/gen/Job";
import type { NodePackages } from "../../../api/gen/NodePackages";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { detail, facts } from "../../../test/fixtures";
import { installed, nodePackages, packageCatalog } from "../../../test/packages";
import { openTestSocket, SignedInProviders } from "../../../test/providers";
import { revisionMeta, siteView } from "../../../test/site";
import {
  changeRows,
  endedCount,
  installBytes,
  NO_CHANGES,
  offered,
  pruned,
  removeBlock,
  searched,
  shownPackages,
  toggled,
  upgradeBlock,
} from "./software";

let calls: { method: string; url: string; body: unknown }[] = [];
let listing: NodePackages = nodePackages();
let catalogFails = false;
/** Holds the catalog's answer until it resolves, when set. */
let catalogHold: Promise<void> | undefined;

function packagesJob(state: Job["state"], done: number): Job {
  const names = ["connect", "check", "install", "inventory"] as const;
  return {
    id: 30,
    kind: "packages",
    target: "gw",
    node: 1,
    revision: null,
    state,
    steps: names.map((name, i) => ({
      name,
      state: i < done ? "done" : i === done && state === "running" ? "running" : "pending",
      detail: name === "check" && i < done ? "Adds tcpdump and libpcap1; removes htop" : null,
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
  listing = nodePackages();
  catalogFails = false;
  catalogHold = undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site") return Response.json(siteView(2));
      if (url === "/api/v1/firmware") return Response.json({ nodes: [], error: null });
      if (url === "/api/v1/nodes/1/backups") return Response.json([]);
      if (url === "/api/v1/nodes/1/packages" && method === "GET") return Response.json(listing);
      if (url === "/api/v1/nodes/1/packages/catalog") {
        await catalogHold;
        if (catalogFails) {
          catalogFails = false;
          return Response.json(
            { title: "Bad gateway", status: 502, detail: "The feeds can't be reached" },
            { status: 502 },
          );
        }
        return Response.json(packageCatalog());
      }
      if (url === "/api/v1/nodes/1/packages") {
        return Response.json(packagesJob("running", 2), { status: 202 });
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderCard(options: { role?: "admin" | "viewer"; managed?: boolean } = {}) {
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
  // The top bar's tray follows applies for every role (M9.5d).
  const topics = ["fleet", "alerts", "config", "node:1", "jobs"];
  await waitFor(() => {
    const sent = ws.sent.at(-1) as { sub?: string[] } | undefined;
    expect(new Set(sent?.sub)).toEqual(new Set(topics));
  });
  const node = detail(facts(1));
  if (options.managed) node.node.mode = "managed";
  ws.receive({ type: "node_snapshot", topic: "node:1", node });
  ws.receive({ type: "config", topic: "config", head: revisionMeta(2), applied: [] });
  await screen.findByRole("heading", { name: "gw" });
  const card = await screen.findByRole("region", { name: "Installed packages" });
  const table = await within(card).findByRole("table", { name: "Installed packages" });
  return { ws, card, table };
}

/** The package names in a table's body, in order. */
function names(table: HTMLElement): string[] {
  return within(table)
    .getAllByRole("row")
    .slice(1)
    .map((row) => within(row).getAllByRole("cell")[0]?.textContent ?? "");
}

describe("package changes", () => {
  it("removes or upgrades a package, never both, and keeps each list sorted", () => {
    let changes = toggled(NO_CHANGES, "install", "tcpdump", true);
    changes = toggled(changes, "install", "iperf3", true);
    expect(changes.install).toEqual(["iperf3", "tcpdump"]);
    changes = toggled(changes, "upgrade", "curl", true);
    changes = toggled(changes, "remove", "curl", true);
    expect(changes).toEqual({ install: ["iperf3", "tcpdump"], remove: ["curl"], upgrade: [] });
    changes = toggled(changes, "upgrade", "curl", true);
    expect(changes.remove).toEqual([]);
    expect(toggled(changes, "install", "iperf3", false).install).toEqual(["tcpdump"]);
  });

  it("drops what the packages read again make moot", () => {
    const now = [installed("tcpdump"), installed("curl")];
    expect(
      pruned({ install: ["tcpdump", "nano"], remove: ["htop", "curl"], upgrade: ["htop"] }, now),
    ).toEqual({ install: ["nano"], remove: ["curl"], upgrade: [] });
  });

  it("filters the installed packages and says why one stays", () => {
    const { packages } = nodePackages();
    expect(shownPackages(packages, "dependency", "").map((pkg) => pkg.name)).toEqual([
      "libncurses6",
    ]);
    expect(shownPackages(packages, "all", "SQM").map((pkg) => pkg.name)).toEqual(["sqm-scripts"]);
    expect(shownPackages(packages, "explicit", "")).toHaveLength(5);
    expect(removeBlock(installed("htop"), null)).toBeUndefined();
    expect(removeBlock(installed("htop"), "it doesn't render")).toMatch(/can't tell/);
    expect(removeBlock(installed("x", { lock: "required" }), null)).toMatch(/configuration/);
    expect(upgradeBlock("kernel", packages)).toMatch(/firmware/);
    expect(upgradeBlock("curl", packages)).toBeUndefined();
  });

  it("offers what isn't installed and ranks name matches first", () => {
    const catalog = packageCatalog();
    const offer = offered(catalog, nodePackages().packages);
    expect(offer.map((pkg) => pkg.name)).toEqual(["iperf3", "tcpdump", "tcpdump-mini", "nano"]);
    expect(searched(offer, "packet").map((pkg) => pkg.name)).toEqual(["tcpdump", "tcpdump-mini"]);
    expect(searched(offer, "mini").map((pkg) => pkg.name)).toEqual(["tcpdump-mini"]);
    // A name that starts with the query beats one that holds it, which beats a description.
    expect(
      searched(
        [
          { name: "a-ip", version: "1", installed_bytes: null, description: "" },
          { name: "x", version: "1", installed_bytes: null, description: "ip tools" },
          { name: "ip-full", version: "1", installed_bytes: null, description: "" },
        ],
        "ip",
      ).map((pkg) => pkg.name),
    ).toEqual(["ip-full", "a-ip", "x"]);
  });

  it("lists the changes with their versions and the flash the installs take", () => {
    const changes = { install: ["tcpdump", "tcpdump-mini"], remove: ["htop"], upgrade: ["curl"] };
    const catalog = packageCatalog();
    expect(installBytes(changes, catalog)).toBe(900_000);
    expect(installBytes({ ...changes, install: ["tcpdump-mini"] }, catalog)).toBeNull();
    expect(changeRows(changes, catalog, nodePackages().packages)).toEqual([
      { kind: "remove", rows: [{ name: "htop", detail: "3.3.0-r1" }] },
      { kind: "upgrade", rows: [{ name: "curl", detail: "8.10.1-r1 → 8.11.0-r1" }] },
      {
        kind: "install",
        rows: [
          { name: "tcpdump", detail: "4.99.5-r1 · 900 KB" },
          { name: "tcpdump-mini", detail: "4.99.5-r1" },
        ],
      },
    ]);
    // Without the catalog, an upgrade shows the installed version.
    expect(
      changeRows({ ...NO_CHANGES, upgrade: ["curl"] }, undefined, nodePackages().packages),
    ).toEqual([{ kind: "upgrade", rows: [{ name: "curl", detail: "8.10.1-r1" }] }]);
  });

  it("counts the packages jobs seen ending", () => {
    const seen = new Set<number>();
    expect(endedCount([packagesJob("succeeded", 4)], 1, seen)).toBe(0);
    expect(endedCount([packagesJob("running", 1)], 1, seen)).toBe(0);
    expect(endedCount([{ ...packagesJob("running", 1), id: 31, node: 2 }], 1, seen)).toBe(0);
    expect(endedCount([packagesJob("failed", 2)], 1, seen)).toBe(1);
  });
});

describe("Device › Config › Installed packages", () => {
  it("lists the packages on purpose, each lock explained, read-only in observe mode", async () => {
    const user = userEvent.setup();
    const { card, table } = await renderCard();
    expect(within(card).getByText(/^6 packages/)).toBeInTheDocument();
    expect(names(table)).toEqual([
      "curl",
      "dropbearEssential",
      "htop",
      "kernelEssentialFirmware",
      "sqm-scriptsRequired",
    ]);
    expect(within(card).getByText(/^Read-only in observe mode/)).toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Add or upgrade…" })).toBeNull();
    expect(within(card).queryByRole("checkbox", { name: /^Remove/ })).toBeNull();

    await user.click(within(card).getByRole("button", { name: "Dependencies (1)" }));
    expect(names(table)).toEqual(["libncurses6"]);
    await user.click(within(card).getByRole("button", { name: "All (6)" }));
    await user.type(within(card).getByRole("textbox", { name: "Search packages" }), "curl");
    expect(names(table)).toEqual(["curl"]);
  });

  it("removes, upgrades and installs on a managed device after confirming every change", async () => {
    const user = userEvent.setup();
    const { ws, card } = await renderCard({ managed: true });
    expect(within(card).getByRole("checkbox", { name: "Remove dropbear" })).toBeDisabled();
    expect(within(card).getByRole("checkbox", { name: "Remove sqm-scripts" })).toBeDisabled();
    await user.click(within(card).getByRole("checkbox", { name: "Remove htop" }));

    let release = () => {};
    catalogHold = new Promise((resolve) => {
      release = resolve;
    });
    await user.click(within(card).getByRole("button", { name: "Add or upgrade…" }));
    const browse = await screen.findByRole("dialog", { name: "Add or upgrade packages on gw" });
    expect(await within(browse).findByText(/can take a minute/)).toBeInTheDocument();
    release();
    const upgrades = await within(browse).findByRole("region", { name: "Upgrades on offer" });
    expect(within(upgrades).getByRole("checkbox", { name: "kernel" })).toBeDisabled();
    await user.click(within(upgrades).getByRole("checkbox", { name: "curl" }));
    const available = within(browse).getByRole("table", { name: "Available" });
    expect(names(available)).toEqual([
      "Install iperf3",
      "Install tcpdump",
      "Install tcpdump-mini",
      "Install nano",
    ]);
    await user.type(within(browse).getByRole("textbox", { name: "Search the feeds" }), "tcp");
    await user.click(within(available).getByRole("checkbox", { name: "Install tcpdump" }));
    expect(within(browse).getByText("3 changes chosen")).toBeInTheDocument();
    await user.click(within(browse).getByRole("button", { name: "Done" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    expect(within(card).getByText("1 to remove · 1 to upgrade · 1 to install")).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Review changes…" }));
    const confirm = await screen.findByRole("dialog", { name: "Change the packages of gw?" });
    expect(within(confirm).getByRole("region", { name: "Remove" })).toHaveTextContent(
      "htop3.3.0-r1",
    );
    expect(within(confirm).getByRole("region", { name: "Upgrade" })).toHaveTextContent(
      "curl8.10.1-r1 → 8.11.0-r1",
    );
    expect(within(confirm).getByRole("region", { name: "Install" })).toHaveTextContent(
      "tcpdump4.99.5-r1 · 900 KB",
    );
    expect(confirm).toHaveTextContent("about 900 KB of flash");
    expect(confirm).toHaveTextContent("nothing rolls back");
    await user.click(within(confirm).getByRole("button", { name: "Make 3 changes" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "POST",
        url: "/api/v1/nodes/1/packages",
        body: { install: ["tcpdump"], remove: ["htop"], upgrade: ["curl"] },
      }),
    );
    const steps = await within(confirm).findByRole("list", { name: "Changing the packages of gw" });
    expect(steps).toHaveTextContent("Check the changes with a dry run");
    expect(steps).toHaveTextContent("Adds tcpdump and libpcap1; removes htop");

    listing = nodePackages({
      packages: [
        ...nodePackages().packages.filter((pkg) => pkg.name !== "htop"),
        installed("tcpdump"),
      ],
    });
    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [packagesJob("succeeded", 4)] });
    expect(await screen.findByText("The packages of gw changed")).toBeInTheDocument();
    expect(within(confirm).getByText(/^Done: the list shows/)).toBeInTheDocument();
    // The last: the dialog's own close button comes first.
    const close = within(confirm).getAllByRole("button", { name: "Close" }).at(-1);
    await user.click(close as HTMLElement);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await within(card).findByRole("checkbox", { name: "Remove tcpdump" });
    expect(within(card).queryByRole("button", { name: "Review changes…" })).toBeNull();
    expect(
      calls.filter((call) => call.url === "/api/v1/nodes/1/packages" && call.method === "GET"),
    ).toHaveLength(2);
  });

  it("removes nothing while the site's needs are unknown, and reads the feeds again after a failure", async () => {
    const user = userEvent.setup();
    listing = nodePackages({ required_unknown: "The configuration doesn't render: no WAN" });
    catalogFails = true;
    const { card } = await renderCard({ managed: true });
    expect(within(card).getByText(/^Nothing can be removed until/)).toHaveTextContent("no WAN");
    expect(within(card).getByRole("checkbox", { name: "Remove htop" })).toBeDisabled();

    await user.click(within(card).getByRole("button", { name: "Add or upgrade…" }));
    const browse = await screen.findByRole("dialog", { name: "Add or upgrade packages on gw" });
    await user.click(await within(browse).findByRole("button", { name: "Try again" }));
    await within(browse).findByRole("region", { name: "Upgrades on offer" });
    expect(calls.filter((call) => call.url.endsWith("/catalog"))).toHaveLength(2);
  });

  it("shows viewers the packages without their actions", async () => {
    const { card } = await renderCard({ role: "viewer", managed: true });
    expect(
      within(card).getByText("Only admins change the packages of devices."),
    ).toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Add or upgrade…" })).toBeNull();
    expect(within(card).queryByRole("checkbox", { name: /^Remove/ })).toBeNull();
  });
});
