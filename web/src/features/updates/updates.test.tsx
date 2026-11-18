import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { FirmwareCatalog } from "../../api/gen/FirmwareCatalog";
import type { FirmwareUpload } from "../../api/gen/FirmwareUpload";
import type { GitHubRelease } from "../../api/gen/GitHubRelease";
import type { ImageBuild } from "../../api/gen/ImageBuild";
import type { Job } from "../../api/gen/Job";
import type { NodeFirmware } from "../../api/gen/NodeFirmware";
import type { UpgradePlan } from "../../api/gen/UpgradePlan";
import { createAppRouter } from "../../app/routes";
import { controllerUpdate } from "../../test/controller";
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
  uploads: [],
  source: { kind: "site" },
  repo: null,
  releases: [],
  ...extra,
});

const SHA = "ab".repeat(32);

const custom: FirmwareUpload = {
  sha256: SHA,
  name: "custom-mt3000.bin",
  size_bytes: 12 * MB,
  uploaded_at: Date.now(),
  target: "mediatek/filogic",
  boards: ["glinet,gl-mt3000"],
  distribution: "OpenWrt",
  version: "25.12.7",
  revision: "r1-custom",
};

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
  uploads: [],
  error: null,
  sources: { repo: null, prereleases: false, token: false },
};

/** The catalog with `custom` uploaded: it fits the Lab (no release on offer) and the Office. */
const withUpload: FirmwareCatalog = {
  ...catalog,
  nodes: catalog.nodes.map((node) => (node.node === 1 ? node : { ...node, uploads: [SHA] })),
  uploads: [custom],
};

const plan = (release: string, source: UpgradePlan["source"] = "asu"): UpgradePlan => ({
  node: 2,
  source,
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
  images: [],
  upload: null,
  repo: null,
});

const githubImage = {
  name: "openwrt-25.12.6-mediatek-filogic-glinet_gl-mt3000-squashfs-sysupgrade.bin",
  kind: "sysupgrade",
  filesystem: "squashfs",
  size_bytes: 9 * MB,
  sha256: "a25a".repeat(16),
  url: "https://github.com/owner/images/releases/download/v2/x.bin",
};

const githubRelease = (tag: string, prerelease = false): GitHubRelease => ({
  tag,
  name: `Build ${tag}`,
  prerelease,
  published_at: Date.UTC(2026, 10, 1),
  notes: `https://github.com/owner/images/releases/tag/${tag}`,
  images: [githubImage],
});

/** The site's repo is `owner/images`: the Office gets two of its releases, the others none. */
const withGitHub: FirmwareCatalog = {
  ...catalog,
  nodes: catalog.nodes.map((node) =>
    node.node === 2
      ? {
          ...node,
          offers: [],
          repo: "owner/images",
          releases: [githubRelease("v3", true), githubRelease("v2")],
        }
      : { ...node, offers: [], repo: "owner/images", note: "No image named for it" },
  ),
  sources: { repo: "owner/images", prereleases: true, token: false },
};

const done: ImageBuild = {
  hash: "0123456789abcdef",
  release: "25.12.6",
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
    const body = typeof init?.body === "string" ? JSON.parse(init.body) : init?.body;
    calls.push([key, body]);
    const route = routes[key];
    if (key === "GET /api/v1/controller/update" && !route) {
      return Response.json(controllerUpdate({ offer: null }));
    }
    if (key === "GET /api/v1/firmware/rollouts" && !route) return Response.json([]);
    if (!route) return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    const answer = route(body);
    return answer instanceof Response ? answer : Response.json(answer);
  });
  vi.stubGlobal("fetch", fetch);
  return calls;
}

const UPGRADE_STEPS = [
  "connect",
  "check",
  "backup",
  "download",
  "stage",
  "flash",
  "reboot",
  "install",
  "verify",
  "soak",
  "record",
] as const;

/** Office's upgrade job, `done` steps in. */
function upgradeJob(state: Job["state"], done: number): Job {
  return {
    id: 21,
    kind: "upgrade",
    target: "Office to 25.12.6",
    node: 2,
    revision: null,
    state,
    steps: UPGRADE_STEPS.map((name, i) => ({
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

function renderUpdates(role: "admin" | "viewer" = "admin", path = "/updates", managed = false) {
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
    summary(2, "Office", { mode: managed ? "managed" : "observe" }),
    summary(3, "Lab"),
  ]);
  ws.receive({ type: "jobs_snapshot", topic: "jobs", jobs: [] });
  return ws;
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
    expect(calls).toContainEqual([
      "POST /api/v1/firmware/nodes/2/plan",
      { source: "asu", release: "24.10.8" },
    ]);
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

  it("installs the built image on a managed device once asked, and follows the job", async () => {
    const user = userEvent.setup();
    const calls = stubApi({
      "GET /api/v1/firmware": () => catalog,
      "POST /api/v1/firmware/nodes/2/plan": (body) => plan((body as { release: string }).release),
      "POST /api/v1/firmware/nodes/2/builds": () => ({ packages: [], build: done }),
      "POST /api/v1/firmware/nodes/2/install": () => upgradeJob("running", 2),
    });
    const ws = renderUpdates("admin", "/updates?node=2", true);
    const dialog = await screen.findByRole("dialog", { name: "Upgrade Office" }, { timeout: 2000 });
    await user.click(await within(dialog).findByRole("radio", { name: /25\.12\.6/ }));
    await within(dialog).findByText("opkg → apk-mbedtls");
    // ASU's image first: no Install before it is built.
    expect(within(dialog).queryByRole("button", { name: "Install" })).toBeNull();
    await user.click(within(dialog).getByRole("button", { name: "Build the image" }));
    await user.click(await within(dialog).findByRole("button", { name: "Install" }));
    const asked = within(dialog).getByText(/Office restarts with the new firmware/);
    expect(asked).toBeTruthy();
    expect(calls.some(([key]) => key.endsWith("/install"))).toBe(false);

    await user.click(within(dialog).getByRole("button", { name: "Install now" }));
    expect(calls).toContainEqual([
      "POST /api/v1/firmware/nodes/2/install",
      { source: "asu", release: "25.12.6", build: done.hash },
    ]);
    const steps = await within(dialog).findByRole("list", { name: "Upgrading Office to 25.12.6" });
    expect(within(steps).getByText("Write the image, keeping the settings")).toBeTruthy();
    expect(within(dialog).getByText(/you can close this window/)).toBeTruthy();
    expect(
      within(dialog)
        .queryByRole("radio", { name: /24\.10\.8/ })
        ?.hasAttribute("disabled"),
    );

    ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [upgradeJob("succeeded", 11)] });
    await within(dialog).findByText("Office is upgraded", { selector: "p" });
    expect(within(dialog).queryByRole("button", { name: "Install" })).toBeNull();
  });

  it("never installs on a device in observe mode", async () => {
    const user = userEvent.setup();
    const calls = stubApi({
      "GET /api/v1/firmware": () => catalog,
      "POST /api/v1/firmware/nodes/2/plan": (body) => ({
        ...plan((body as { release: string }).release),
        source: (body as { source: UpgradePlan["source"] }).source,
      }),
    });
    renderUpdates("admin", "/updates?node=2", false);
    const dialog = await screen.findByRole("dialog", { name: "Upgrade Office" }, { timeout: 2000 });
    await within(dialog).findByText("2 of your packages carried over");
    // The official image needs no build: Install shows, but stays off.
    await user.click(within(dialog).getByRole("button", { name: "Official image" }));
    await waitFor(() =>
      expect(calls).toContainEqual([
        "POST /api/v1/firmware/nodes/2/plan",
        { source: "official", release: "24.10.8" },
      ]),
    );
    const install = await within(dialog).findByRole("button", { name: "Install" });
    expect(install.hasAttribute("disabled")).toBe(true);
    expect(within(dialog).getByText(/is in observe mode/)).toBeTruthy();
    expect(calls.some(([key]) => key.endsWith("/install"))).toBe(false);
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
        uploads: [],
        error: "Firmware upgrades are off: the controller runs without ASU (--asu-url off)",
        sources: catalog.sources,
      }),
    });
    renderUpdates("viewer");
    await screen.findByText(/--asu-url off/, undefined, { timeout: 2000 });
    expect(screen.queryByRole("button", { name: /^Prepare upgrade/ })).toBeNull();
    await waitFor(() => expect(screen.getAllByText("Unknown")).toHaveLength(3));
  });

  it("plans with the official image: the packages installed after, no build", async () => {
    const user = userEvent.setup();
    const sysupgrade = { ...(done.images[0] as ImageBuild["images"][number]) };
    const calls = stubApi({
      "GET /api/v1/firmware": () => catalog,
      "POST /api/v1/firmware/nodes/2/plan": (body) => {
        const { release, source } = body as { release: string; source: "asu" | "official" };
        return { ...plan(release, source), images: source === "official" ? [sysupgrade] : [] };
      },
    });
    renderUpdates("admin", "/updates?node=2");
    const dialog = await screen.findByRole("dialog", { name: "Upgrade Office" }, { timeout: 2000 });
    await within(dialog).findByText("2 of your packages carried over");
    await user.click(within(dialog).getByRole("button", { name: "Official image" }));
    await within(dialog).findByText("2 of your packages installed after the upgrade");
    expect(calls).toContainEqual([
      "POST /api/v1/firmware/nodes/2/plan",
      { source: "official", release: "24.10.8" },
    ]);
    const files = within(dialog).getByRole("list", { name: "The official image" });
    expect(within(files).getByText("openwrt-squashfs-sysupgrade.bin")).toBeTruthy();
    expect(within(dialog).queryByRole("button", { name: "Build the image" })).toBeNull();
  });

  it("offers an uploaded image to the devices it fits and plans it as it is", async () => {
    const user = userEvent.setup();
    const calls = stubApi({
      "GET /api/v1/firmware": () => withUpload,
      "POST /api/v1/firmware/nodes/3/plan": () => ({
        ...plan("25.12.7", "upload"),
        node: 3,
        kept: [],
        packages_at: null,
        image_bytes: custom.size_bytes,
        upload: custom,
      }),
    });
    renderUpdates();
    // The Lab has no release on offer, but an upload fits it.
    await user.click(
      await screen.findByRole("button", { name: "Prepare upgrade: Lab" }, { timeout: 2000 }),
    );
    const dialog = await screen.findByRole("dialog", { name: "Upgrade Lab" });
    await within(dialog).findByText(/Flashed as it is/);
    expect(calls).toContainEqual([
      "POST /api/v1/firmware/nodes/3/plan",
      { source: "upload", sha256: SHA },
    ]);
    expect(within(dialog).queryByRole("button", { name: "Official image" })).toBeNull();
    expect(within(dialog).queryByRole("button", { name: "Build the image" })).toBeNull();
    expect(within(dialog).getByText(/The image is 12/)).toBeTruthy();
  });

  it("uploads an image, then deletes it after asking", async () => {
    const user = userEvent.setup();
    let current = catalog;
    const calls = stubApi({
      "GET /api/v1/firmware": () => current,
      [`POST /api/v1/firmware/uploads?name=${encodeURIComponent("custom mt3000.bin")}`]: () => {
        current = withUpload;
        return Response.json(custom, { status: 201 });
      },
      [`DELETE /api/v1/firmware/uploads/${SHA}`]: () => {
        current = catalog;
        return new Response(null, { status: 204 });
      },
    });
    renderUpdates();
    const card = await screen.findByRole("region", { name: "Uploaded images" }, { timeout: 2000 });
    expect(within(card).getByText("No images uploaded.")).toBeTruthy();
    const file = new File([new Uint8Array([1, 2, 3])], "custom mt3000.bin");
    await user.upload(within(card).getByLabelText("Upload image"), file);
    const item = await within(card).findByRole("listitem", { name: "custom-mt3000.bin" });
    expect(item.textContent).toContain("OpenWrt 25.12.7");
    expect(item.textContent).toContain("Fits 2 devices");
    const sent = calls.find(([key]) => key.startsWith("POST /api/v1/firmware/uploads"));
    expect(sent?.[1]).toBe(file);

    await user.click(within(item).getByRole("button", { name: "Delete custom-mt3000.bin" }));
    expect(within(item).getByText("Delete custom-mt3000.bin?")).toBeTruthy();
    await user.click(within(item).getByRole("button", { name: "Delete" }));
    await within(card).findByText("No images uploaded.");
    expect(calls.map(([key]) => key)).toContain(`DELETE /api/v1/firmware/uploads/${SHA}`);
  });

  it("offers a GitHub repo's releases and plans one as it is", async () => {
    const user = userEvent.setup();
    const calls = stubApi({
      "GET /api/v1/firmware": () => withGitHub,
      "POST /api/v1/firmware/nodes/2/plan": (body) => ({
        ...plan((body as { release: string }).release, "github"),
        kept: [],
        removed: [],
        renamed: [],
        dropped: [],
        packages_at: null,
        image_bytes: githubImage.size_bytes,
        images: [githubImage],
        repo: "owner/images",
      }),
    });
    renderUpdates();
    const table = await screen.findByRole("table", { name: "Firmware" }, { timeout: 2000 });
    const office = within(table).getAllByRole("row")[3] as HTMLElement;
    expect(within(office).getByText("Builds available")).toBeTruthy();
    // The newest release, and how many older ones the dialog offers.
    expect(within(office).getByText("Prerelease")).toBeTruthy();
    expect(
      within(office).getByRole("link", { name: "Release notes v3" }).getAttribute("href"),
    ).toBe("https://github.com/owner/images/releases/tag/v3");
    expect(within(office).getByText("and 1 older")).toBeTruthy();
    expect(within(office).getByText("Site default · owner/images")).toBeTruthy();
    expect(screen.getByText("1 device with GitHub releases to choose from")).toBeTruthy();

    await user.click(screen.getByRole("button", { name: "Prepare upgrade: Office" }));
    const dialog = await screen.findByRole("dialog", { name: "Upgrade Office" });
    const files = await within(dialog).findByRole("list", {
      name: "The release's images on owner/images",
    });
    expect(within(files).getByText(githubImage.name)).toBeTruthy();
    expect(within(dialog).getByText(/Flashed as it is: your build/)).toBeTruthy();
    expect(calls).toContainEqual([
      "POST /api/v1/firmware/nodes/2/plan",
      { source: "github", release: "v3" },
    ]);
    await user.click(within(dialog).getByRole("radio", { name: /^v2/ }));
    await waitFor(() =>
      expect(calls).toContainEqual([
        "POST /api/v1/firmware/nodes/2/plan",
        { source: "github", release: "v2" },
      ]),
    );
    expect(within(dialog).queryByRole("button", { name: "Official image" })).toBeNull();
    expect(within(dialog).queryByRole("button", { name: "Build the image" })).toBeNull();
  });

  it("sets a device's own source", async () => {
    const user = userEvent.setup();
    const calls = stubApi({
      "GET /api/v1/firmware": () => catalog,
      "PUT /api/v1/firmware/nodes/2/source": (body) => body,
    });
    renderUpdates();
    await user.click(
      await screen.findByRole("button", { name: "Change: Office" }, { timeout: 2000 }),
    );
    const dialog = await screen.findByRole("dialog", { name: "Image source of Office" });
    const save = within(dialog).getByRole("button", { name: "Save" });
    await user.click(within(dialog).getByRole("radio", { name: /Another GitHub repo/ }));
    expect((save as HTMLButtonElement).disabled).toBe(true);
    await user.type(within(dialog).getByLabelText("GitHub repo"), "owner/ap-builds");
    await user.click(save);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls).toContainEqual([
      "PUT /api/v1/firmware/nodes/2/source",
      { kind: "github", repo: "owner/ap-builds" },
    ]);
    expect(calls.filter(([key]) => key === "GET /api/v1/firmware").length).toBeGreaterThan(1);
  });

  it("saves the site's repo with a token, then removes the token", async () => {
    const user = userEvent.setup();
    let sources = catalog.sources;
    const calls = stubApi({
      "GET /api/v1/firmware": () => ({ ...catalog, sources }),
      "PUT /api/v1/firmware/sources": (body) => {
        const sent = body as {
          repo: string | null;
          prereleases: boolean;
          token: { action: string };
        };
        sources = {
          repo: sent.repo,
          prereleases: sent.prereleases,
          token: sent.token.action === "set",
        };
        return sources;
      },
    });
    renderUpdates();
    const card = await screen.findByRole("region", { name: "Image sources" }, { timeout: 2000 });
    await user.type(within(card).getByLabelText("GitHub repo"), "owner/images");
    await user.type(within(card).getByLabelText("Access token"), "github_pat_x");
    await user.click(within(card).getByRole("checkbox", { name: /Offer prereleases/ }));
    await user.click(within(card).getByRole("button", { name: "Save" }));
    await within(card).findByText("A token is kept in the vault.");
    expect(within(card).getByText("Saved")).toBeTruthy();
    expect(calls).toContainEqual([
      "PUT /api/v1/firmware/sources",
      { repo: "owner/images", prereleases: true, token: { action: "set", token: "github_pat_x" } },
    ]);

    await user.click(within(card).getByRole("button", { name: "Remove token" }));
    expect(within(card).getByText("The token will be removed when you save.")).toBeTruthy();
    await user.click(within(card).getByRole("button", { name: "Save" }));
    await within(card).findByLabelText("Access token");
    expect(calls).toContainEqual([
      "PUT /api/v1/firmware/sources",
      { repo: "owner/images", prereleases: true, token: { action: "clear" } },
    ]);
  });

  it("shows the sources to viewers without letting them change any", async () => {
    stubApi({ "GET /api/v1/firmware": () => withGitHub });
    renderUpdates("viewer");
    const card = await screen.findByRole("region", { name: "Image sources" }, { timeout: 2000 });
    expect(within(card).getByText("owner/images")).toBeTruthy();
    expect(within(card).getByText("No token: public repos only.")).toBeTruthy();
    expect(within(card).queryByRole("button", { name: "Save" })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Change/ })).toBeNull();
  });

  it("hides uploading and deleting from viewers", async () => {
    stubApi({ "GET /api/v1/firmware": () => withUpload });
    renderUpdates("viewer");
    const card = await screen.findByRole("region", { name: "Uploaded images" }, { timeout: 2000 });
    await within(card).findByRole("listitem", { name: "custom-mt3000.bin" });
    expect(within(card).queryByLabelText("Upload image")).toBeNull();
    expect(within(card).queryByRole("button", { name: /^Delete/ })).toBeNull();
  });
});
