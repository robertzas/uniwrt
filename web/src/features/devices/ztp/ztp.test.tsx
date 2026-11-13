import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ImageBuild } from "../../../api/gen/ImageBuild";
import type { PendingDevice } from "../../../api/gen/PendingDevice";
import type { ZtpInbox } from "../../../api/gen/ZtpInbox";
import type { ZtpOneLiner } from "../../../api/gen/ZtpOneLiner";
import { createAppRouter } from "../../../app/routes";
import { adoptJob, summary } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";

const NOW = Date.now();

const device: PendingDevice = {
  mac: "94:83:c4:00:00:07",
  board_name: "glinet,gl-mt6000",
  model: "GL.iNet GL-MT6000",
  hostname: "uniwrt-0007",
  release: "24.10.4",
  role: "ap",
  address: "10.0.0.47",
  source: { kind: "hello" },
  locked: false,
  host_key: "ssh-ed25519 AAAA",
  fingerprint: "SHA256:M+sb+ikwtPDQxF10CkImjS44GcoH92RcQ6sj8kVW+r8",
  first_seen: NOW - 120_000,
  last_seen: NOW - 30_000,
  job: null,
};

const oneLiner: ZtpOneLiner = {
  token: "Xk3abc",
  role: "bridge",
  name: "Attic",
  command:
    "uclient-fetch -4 --no-check-certificate -qO /tmp/uniwrt.sh 'https://10.0.0.5:8443/b/Xk3abc' && …",
  sha256: "ab08",
  created_at: NOW,
  expires_at: NOW + 15 * 60_000,
  state: "waiting",
  fetched_from: null,
  device: null,
  created_by: "admin",
};

type Route = (body: unknown) => unknown;

/** Answers the API by method and path; anything else is the health check. */
function stubApi(routes: Record<string, Route>) {
  const calls: [string, unknown][] = [];
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    const key = `${init?.method ?? "GET"} ${url}`;
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    if (init?.method && init.method !== "GET") calls.push([key, body]);
    const route = routes[key];
    if (!route) return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    const answer = route(body);
    if (answer === undefined) return new Response(null, { status: 204 });
    return answer instanceof Response ? answer : Response.json(answer);
  });
  vi.stubGlobal("fetch", fetch);
  return calls;
}

function renderDevices() {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/devices"));
  render(() => (
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <SignedInProviders socket={socket}>
        <Router>{(props) => props.children}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "gateway", { role: "gateway" })]);
  ws.receive({ type: "jobs_snapshot", topic: "jobs", jobs: [] });
  return ws;
}

describe("Waiting for adoption", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("adopts a waiting device after its fingerprint, and follows the job", async () => {
    const user = userEvent.setup();
    let inbox: ZtpInbox = { pending: [device], approvals: [] };
    const calls = stubApi({
      "GET /api/v1/ztp/pending": () => inbox,
      "POST /api/v1/ztp/pending/94%3A83%3Ac4%3A00%3A00%3A07/adopt": () => {
        inbox = { pending: [{ ...device, job: 9 }], approvals: [] };
        return adoptJob(9, "10.0.0.47", 1);
      },
    });
    const ws = renderDevices();
    const card = await screen.findByRole("region", { name: "Waiting for adoption" });
    const row = await within(card).findByText("uniwrt-0007");
    expect(within(card).getByText("10.0.0.47")).toBeInTheDocument();
    expect(within(card).getByText("SHA256:M+sb+ikw…W+r8")).toBeInTheDocument();
    expect(row.closest("li")).toHaveTextContent("Built as access point");

    await user.click(within(card).getByRole("button", { name: "Adopt" }));
    const dialog = await screen.findByRole("dialog", { name: "Adopt uniwrt-0007" });
    expect(within(dialog).getByTestId("ztp-fingerprint")).toHaveTextContent(device.fingerprint);
    await user.type(within(dialog).getByLabelText("Name"), "Attic");
    await user.click(within(dialog).getByRole("button", { name: "Bridge" }));
    await user.click(within(dialog).getByRole("button", { name: "It matches, adopt" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(calls).toEqual([
      [
        "POST /api/v1/ztp/pending/94%3A83%3Ac4%3A00%3A00%3A07/adopt",
        { name: "Attic", role: "bridge" },
      ],
    ]);
    expect(await within(card).findByText(/Adopting · \d+%/)).toBeInTheDocument();

    ws.receive({
      type: "jobs_delta",
      topic: "jobs",
      jobs: [
        adoptJob(9, "10.0.0.47", 1, "failed", {
          state: "failed",
          error: { code: "identity_mismatch", message: "board differs" },
        }),
      ],
    });
    expect(await within(card).findByText("board differs")).toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  it("shows devices on the adoption port, locked or not, without adopting them yet", async () => {
    const port = { kind: "port", gateway: 1, port: "lan4" } as const;
    const factory: PendingDevice = {
      ...device,
      mac: "94:83:c4:00:00:09",
      hostname: "OpenWrt",
      role: null,
      address: "192.168.1.1",
      source: port,
    };
    const locked: PendingDevice = {
      ...factory,
      mac: "02:42:ac:11:00:03",
      board_name: "",
      model: "",
      hostname: "",
      release: "",
      locked: true,
    };
    stubApi({ "GET /api/v1/ztp/pending": () => ({ pending: [factory, locked], approvals: [] }) });
    renderDevices();
    const card = await screen.findByRole("region", { name: "Waiting for adoption" });
    const open = (await within(card).findByText("OpenWrt")).closest("li") as HTMLElement;
    expect(open).toHaveTextContent("On the adoption port (lan4) · Last seen");
    expect(open).not.toHaveTextContent("Root password set");
    const adopt = within(open).getByRole("button", { name: "Adopt" });
    expect(adopt).toBeDisabled();
    expect(adopt).toHaveAttribute("title", "Adopting from the adoption port isn't available yet.");
    expect(within(open).getByRole("button", { name: "Dismiss" })).toHaveAttribute(
      "title",
      "It shows up again when it's plugged in again.",
    );
    const shut = within(card).getByText("Unnamed device").closest("li") as HTMLElement;
    expect(shut).toHaveTextContent("02:42:ac:11:00:03");
    expect(shut).toHaveTextContent("Root password set");
    expect(within(shut).getByRole("button", { name: "Adopt" })).toBeDisabled();
  });

  it("dismisses, pre-approves by MAC and withdraws", async () => {
    const user = userEvent.setup();
    let inbox: ZtpInbox = { pending: [device], approvals: [] };
    const calls = stubApi({
      "GET /api/v1/ztp/pending": () => inbox,
      "DELETE /api/v1/ztp/pending/94%3A83%3Ac4%3A00%3A00%3A07": () => {
        inbox = { pending: [], approvals: [] };
        return undefined;
      },
      "PUT /api/v1/ztp/approvals/94%3A83%3Ac4%3A00%3A00%3A08": () => {
        const approval = {
          mac: "94:83:c4:00:00:08",
          name: "Garage",
          role: null,
          created_at: NOW,
          expires_at: NOW + 7 * 86_400_000,
          created_by: "admin",
        };
        inbox = { pending: [], approvals: [approval] };
        return approval;
      },
      "DELETE /api/v1/ztp/approvals/94%3A83%3Ac4%3A00%3A00%3A08": () => {
        inbox = { pending: [], approvals: [] };
        return undefined;
      },
    });
    renderDevices();
    const card = await screen.findByRole("region", { name: "Waiting for adoption" });
    await within(card).findByText("uniwrt-0007");
    await user.click(within(card).getByRole("button", { name: "Dismiss" }));
    expect(await within(card).findByText(/No device is waiting/)).toBeInTheDocument();

    await user.click(within(card).getByRole("button", { name: "Pre-approve a device" }));
    const dialog = await screen.findByRole("dialog", { name: "Pre-approve a device" });
    const mac = within(dialog).getByLabelText(/MAC address/);
    await user.type(mac, "nope");
    await user.click(within(dialog).getByRole("button", { name: "Pre-approve for 7 days" }));
    expect(within(dialog).getByText(/Enter a MAC address/)).toBeInTheDocument();
    await user.clear(mac);
    await user.type(mac, "94-83-C4-00-00-08");
    await user.type(within(dialog).getByLabelText("Name"), "Garage");
    await user.click(within(dialog).getByRole("button", { name: "Pre-approve for 7 days" }));
    const approvals = await within(card).findByRole("region", { name: "Pre-approved" });
    expect(approvals).toHaveTextContent("94:83:c4:00:00:08");
    expect(approvals).toHaveTextContent("Garage");
    expect(approvals).toHaveTextContent(/Expires in 6d 23h|Expires in 7d/);

    await user.click(within(approvals).getByRole("button", { name: "Withdraw" }));
    await waitFor(() =>
      expect(within(card).queryByRole("region", { name: "Pre-approved" })).not.toBeInTheDocument(),
    );
    expect(calls.map(([key]) => key)).toEqual([
      "DELETE /api/v1/ztp/pending/94%3A83%3Ac4%3A00%3A00%3A07",
      "PUT /api/v1/ztp/approvals/94%3A83%3Ac4%3A00%3A00%3A08",
      "DELETE /api/v1/ztp/approvals/94%3A83%3Ac4%3A00%3A00%3A08",
    ]);
    expect(calls[1]?.[1]).toEqual({ name: "Garage" });
  });

  it("makes a one-liner, follows it until the device calls home, and links the script", async () => {
    const user = userEvent.setup();
    let reads = 0;
    const calls = stubApi({
      "GET /api/v1/ztp/pending": () => ({ pending: [], approvals: [] }),
      "POST /api/v1/ztp/one-liners": () => oneLiner,
      "GET /api/v1/ztp/one-liners/Xk3abc": () => {
        reads += 1;
        return reads === 1
          ? { ...oneLiner, state: "fetched", fetched_from: "10.0.0.47" }
          : { ...oneLiner, state: "used", device: "94:83:c4:00:00:07" };
      },
    });
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    renderDevices();
    const card = await screen.findByRole("region", { name: "Waiting for adoption" });
    await user.click(within(card).getByRole("button", { name: "Set up a device" }));
    const dialog = await screen.findByRole("dialog", { name: "Set up a new device" });
    expect(within(dialog).queryByRole("button", { name: "Gateway" })).not.toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Bridge" }));
    await user.type(within(dialog).getByLabelText("Name"), "Attic");
    await user.click(within(dialog).getByRole("button", { name: "Make a command" }));

    expect(await within(dialog).findByTestId("one-liner")).toHaveTextContent(oneLiner.command);
    expect(within(dialog).getByText(/Single-use; expires in 1[45]m/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "Copy" }));
    expect(writeText).toHaveBeenCalledWith(oneLiner.command);
    expect(within(dialog).getByRole("button", { name: "Copied" })).toBeInTheDocument();
    expect(calls).toEqual([["POST /api/v1/ztp/one-liners", { role: "bridge", name: "Attic" }]]);

    expect(
      await within(dialog).findByText(/Fetched from 10.0.0.47/, {}, { timeout: 5_000 }),
    ).toBeInTheDocument();
    expect(
      await within(dialog).findByText(/94:83:c4:00:00:07 called home/, {}, { timeout: 5_000 }),
    ).toBeInTheDocument();

    await user.click(within(dialog).getByRole("tab", { name: "First-boot script" }));
    await user.click(within(dialog).getByRole("button", { name: "Gateway" }));
    expect(within(dialog).getByRole("link", { name: "Download the script" })).toHaveAttribute(
      "href",
      "/api/v1/ztp/script?role=gateway",
    );
  }, 15_000);
  it("builds an image of a model through ASU and lists its downloads", async () => {
    const user = userEvent.setup();
    const hash = "0123456789abcdef0123456789abcdef";
    const sha256 = "a25a05977027deae000b10b025b75ed3e9c12a4b7d42ae57e8d5a8a019493658";
    const queued: ImageBuild = {
      hash,
      state: "queued",
      queue_position: 2,
      step: null,
      error: null,
      images: [],
    };
    const calls = stubApi({
      "GET /api/v1/ztp/pending": () => ({ pending: [], approvals: [] }),
      "GET /api/v1/ztp/releases": () => ({ releases: ["25.12.6", "24.10.8"] }),
      "GET /api/v1/ztp/profiles?release=25.12.6": () => [
        { id: "glinet_gl-mt3000", target: "mediatek/filogic", titles: ["GL.iNet GL-MT3000"] },
        { id: "glinet_gl-mt6000", target: "mediatek/filogic", titles: ["GL.iNet GL-MT6000"] },
      ],
      "POST /api/v1/ztp/builds": () => ({
        packages: ["-wpad-basic-mbedtls", "wpad-mbedtls", "kmod-batman-adv", "batctl-default"],
        build: queued,
      }),
      [`GET /api/v1/ztp/builds/${hash}`]: () => ({
        ...queued,
        state: "done",
        queue_position: null,
        images: [
          {
            name: "openwrt-squashfs-factory.bin",
            kind: "factory",
            filesystem: "squashfs",
            size_bytes: 39_797_056,
            sha256,
            url: `https://sysupgrade.openwrt.org/store/${hash}/openwrt-squashfs-factory.bin`,
          },
        ],
      }),
    });
    renderDevices();
    const card = await screen.findByRole("region", { name: "Waiting for adoption" });
    await user.click(within(card).getByRole("button", { name: "Set up a device" }));
    const dialog = await screen.findByRole("dialog", { name: "Set up a new device" });
    await user.click(within(dialog).getByRole("tab", { name: "Build an image" }));
    const release = await within(dialog).findByLabelText("OpenWrt release");
    await waitFor(() => expect(release).toHaveDisplayValue("25.12.6 (latest)"));
    const search = await within(dialog).findByLabelText("Model");
    await waitFor(() => expect(search).toBeEnabled());
    expect(within(dialog).getByRole("button", { name: "Build the image" })).toBeDisabled();
    await user.type(search, "mt6000");
    await user.click(within(dialog).getByRole("button", { name: /GL\.iNet GL-MT6000/ }));
    expect(within(dialog).getByTestId("ztp-model")).toHaveTextContent("GL.iNet GL-MT6000");
    await user.click(within(dialog).getByRole("button", { name: "Bridge" }));
    await user.click(within(dialog).getByRole("button", { name: "Build the image" }));

    expect(await within(dialog).findByText("Queued at ASU: 2 ahead.")).toBeInTheDocument();
    expect(
      within(dialog).getByText(
        "Adds wpad-mbedtls, kmod-batman-adv, batctl-default, replacing wpad-basic-mbedtls.",
      ),
    ).toBeInTheDocument();
    expect(calls).toEqual([
      [
        "POST /api/v1/ztp/builds",
        {
          release: "25.12.6",
          target: "mediatek/filogic",
          profile: "glinet_gl-mt6000",
          role: "bridge",
        },
      ],
    ]);
    const download = await within(dialog).findByRole(
      "link",
      { name: "Download Factory image" },
      { timeout: 5_000 },
    );
    expect(download).toHaveAttribute(
      "href",
      `https://sysupgrade.openwrt.org/store/${hash}/openwrt-squashfs-factory.bin`,
    );
    expect(within(dialog).getByText(`SHA-256 ${sha256}`)).toBeInTheDocument();
    expect(within(dialog).getByText("39.8 MB")).toBeInTheDocument();
    expect(within(dialog).getByRole("link", { name: "How to flash this model" })).toHaveAttribute(
      "href",
      "https://openwrt.org/start?do=search&q=GL.iNet%20GL-MT6000",
    );
  }, 15_000);
});
