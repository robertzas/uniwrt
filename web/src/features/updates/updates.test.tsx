import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { FirmwareCatalog } from "../../api/gen/FirmwareCatalog";
import type { ImageBuild } from "../../api/gen/ImageBuild";
import type { NodeFirmware } from "../../api/gen/NodeFirmware";
import type { UpgradePlan } from "../../api/gen/UpgradePlan";
import { createAppRouter } from "../../app/routes";
import { summary } from "../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";

const MB = 1024 * 1024;

const entry = (node: number, version: string, extra: Partial<NodeFirmware> = {}): NodeFirmware => ({
  node,
  distribution: "OpenWrt",
  version,
  revision: "r29233-443ec4032a",
  target: "mediatek/filogic",
  board: "glinet,gl-mt3000",
  offers: [],
  note: null,
  ...extra,
});

const notes = (release: string) => `https://openwrt.org/releases/x/notes-${release}`;

const catalog: FirmwareCatalog = {
  nodes: [
    entry(1, "25.12.6", { board: "glinet,gl-mt6000" }),
    entry(2, "24.10.7", {
      offers: [
        {
          release: "25.12.6",
          kind: "branch",
          notes: notes("25.12.6"),
          profile: "glinet_gl-mt3000",
          image_bytes: 10 * MB,
        },
        {
          release: "24.10.8",
          kind: "update",
          notes: notes("24.10.8"),
          profile: "glinet_gl-mt3000",
          image_bytes: 10 * MB,
        },
      ],
    }),
    entry(3, "SNAPSHOT", { note: "SNAPSHOT isn't a release" }),
  ],
  error: null,
};

const plan = (release: string): UpgradePlan => ({
  node: 2,
  from: "24.10.7",
  release,
  target: "mediatek/filogic",
  profile: "glinet_gl-mt3000",
  packages: ["luci", "-wpad-basic-mbedtls"],
  kept: ["luci", "wpad-mbedtls"],
  renamed: release === "25.12.6" ? [{ from: "opkg", to: "apk-mbedtls" }] : [],
  added: [],
  dropped: release === "25.12.6" ? ["my-tool"] : [],
  removed: ["wpad-basic-mbedtls"],
  packages_at: Date.now(),
  image_bytes: 10 * MB,
  tmp_free_bytes: 200 * MB,
});

const done: ImageBuild = {
  hash: "0123456789abcdef",
  state: "done",
  queue_position: null,
  step: null,
  error: null,
  images: [
    {
      name: "openwrt-squashfs-sysupgrade.bin",
      kind: "sysupgrade",
      filesystem: "squashfs",
      size_bytes: 11 * MB,
      sha256: "a25a",
      url: "https://asu.example/store/0123/openwrt-squashfs-sysupgrade.bin",
    },
    {
      name: "openwrt-squashfs-factory.bin",
      kind: "factory",
      filesystem: "squashfs",
      size_bytes: 40 * MB,
      sha256: "582e",
      url: "https://asu.example/store/0123/openwrt-squashfs-factory.bin",
    },
  ],
};

type Route = (body: unknown) => unknown;

/** Answers the API by method and path; anything else is the health check. */
function stubApi(routes: Record<string, Route>) {
  const calls: [string, unknown][] = [];
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    const key = `${init?.method ?? "GET"} ${url}`;
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    calls.push([key, body]);
    const route = routes[key];
    if (!route) return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    const answer = route(body);
    return answer instanceof Response ? answer : Response.json(answer);
  });
  vi.stubGlobal("fetch", fetch);
  return calls;
}

function renderUpdates(role: "admin" | "viewer" = "admin", path = "/updates") {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory(path));
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
    summary(1, "Gateway", { role: "gateway" }),
    summary(2, "Office"),
    summary(3, "Lab"),
  ]);
  ws.receive({ type: "jobs_snapshot", topic: "jobs", jobs: [] });
}

describe("Updates", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("lists each device's release and the upgrades on offer", async () => {
    stubApi({ "GET /api/v1/firmware": () => catalog });
    renderUpdates();
    const table = await screen.findByRole("table", { name: "Firmware" }, { timeout: 2000 });
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows.map((row) => within(row).getAllByRole("cell")[0]?.textContent)).toEqual([
      expect.stringContaining("Gateway"),
      expect.stringContaining("Lab"),
      expect.stringContaining("Office"),
    ]);
    expect(within(rows[0] as HTMLElement).getByText("Up to date")).toBeTruthy();
    expect(within(rows[1] as HTMLElement).getByText("SNAPSHOT isn't a release")).toBeTruthy();
    const office = rows[2] as HTMLElement;
    expect(within(office).getByText("Upgrade available")).toBeTruthy();
    expect(within(office).getByText("New release")).toBeTruthy();
    expect(
      within(office).getByRole("link", { name: "Release notes 24.10.8" }).getAttribute("href"),
    ).toBe(notes("24.10.8"));
    expect(screen.getByText("1 of 3 devices can upgrade")).toBeTruthy();
    expect(screen.getAllByRole("button", { name: /^Prepare upgrade/ })).toHaveLength(1);
  });

  it("plans the update first, the next branch on request, and builds the image", async () => {
    const user = userEvent.setup();
    const calls = stubApi({
      "GET /api/v1/firmware": () => catalog,
      "POST /api/v1/firmware/nodes/2/plan": (body) => plan((body as { release: string }).release),
      "POST /api/v1/firmware/nodes/2/builds": () => ({ packages: [], build: done }),
    });
    renderUpdates();
    await user.click(
      await screen.findByRole("button", { name: "Prepare upgrade: Office" }, { timeout: 2000 }),
    );
    const dialog = await screen.findByRole("dialog", { name: "Upgrade Office" });
    // The update within the branch comes first: fixes only.
    await within(dialog).findByText("2 of your packages carried over");
    expect(calls).toContainEqual(["POST /api/v1/firmware/nodes/2/plan", { release: "24.10.8" }]);
    expect(within(dialog).queryByRole("alert")).toBeNull();

    await user.click(within(dialog).getByRole("radio", { name: /25\.12\.6/ }));
    const warning = await within(dialog).findByRole("alert");
    expect(warning.textContent).toContain("Not in OpenWrt 25.12.6");
    expect(warning.textContent).toContain("my-tool");
    expect(within(dialog).getByText("opkg → apk-mbedtls")).toBeTruthy();

    await user.click(within(dialog).getByRole("button", { name: "Build the image" }));
    expect(calls).toContainEqual(["POST /api/v1/firmware/nodes/2/builds", { release: "25.12.6" }]);
    const images = await within(dialog).findByRole("list", { name: "The image is ready." });
    // Only the sysupgrade image: the device runs OpenWrt already.
    expect(within(images).getAllByRole("listitem")).toHaveLength(1);
    expect(within(images).getByText("openwrt-squashfs-sysupgrade.bin")).toBeTruthy();
    expect(within(dialog).queryByRole("button", { name: "Build the image" })).toBeNull();
  });

  it("opens a device's upgrade from its link, and closing it forgets the link", async () => {
    stubApi({
      "GET /api/v1/firmware": () => catalog,
      "POST /api/v1/firmware/nodes/2/plan": (body) => plan((body as { release: string }).release),
    });
    renderUpdates("admin", "/updates?node=2");
    const dialog = await screen.findByRole("dialog", { name: "Upgrade Office" }, { timeout: 2000 });
    await within(dialog).findByText("2 of your packages carried over");
    await userEvent.setup().keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("shows a plan's error and lets viewers only look", async () => {
    stubApi({
      "GET /api/v1/firmware": () => ({
        nodes: catalog.nodes,
        error: "Firmware upgrades are off: the controller runs without ASU (--asu-url off)",
      }),
    });
    renderUpdates("viewer");
    await screen.findByText(/--asu-url off/, undefined, { timeout: 2000 });
    expect(screen.queryByRole("button", { name: /^Prepare upgrade/ })).toBeNull();
    await waitFor(() => expect(screen.getAllByText("Unknown")).toHaveLength(3));
  });
});
