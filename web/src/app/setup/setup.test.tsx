import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GatewayView } from "../../api/gen/GatewayView";
import type { NodeFacts } from "../../api/gen/NodeFacts";
import type { SiteSave } from "../../api/gen/SiteSave";
import { adoptJob, detail, facts, summary } from "../../test/fixtures";
import { applyJob, applyPlan } from "../../test/plan";
import { LUMEN, PPPOE } from "../../test/presets";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { siteView } from "../../test/site";
import type { FakeWebSocket } from "../../test/socket";
import { createAppRouter } from "../routes";

let calls: { method: string; url: string; body: unknown }[] = [];
let found: GatewayView[] = [];
let head = 1;

const FACTORY: GatewayView = {
  host: "192.168.1.1",
  port: 22,
  host_key: "ssh-ed25519 AAAAC3Nza",
  fingerprint: "SHA256:5bXUECT2eXzZC5NK7F9nS1zBxpxX7K8ubpDXGxs9sGY",
  algorithm: "ssh-ed25519",
  blank: true,
  model: "GL.iNet GL-MT6000",
  hostname: "OpenWrt",
  release: "OpenWrt 25.12.6",
  unsupported: null,
  adopted: null,
};

/** The gateway's facts with a radio per band. */
function radioFacts(bands: string[]): NodeFacts {
  const radios = bands.map((band, i) => ({ name: `radio${i}`, band }));
  return { ...facts(10), radios: radios as unknown as NodeFacts["radios"] };
}

beforeEach(() => {
  calls = [];
  found = [FACTORY];
  head = 1;
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
      if (url === "/api/v1/isp/presets") return Response.json([PPPOE, LUMEN]);
      if (url === "/api/v1/bootstrap/gateway") {
        return Response.json({ candidates: ["192.168.1.1"], gateways: found });
      }
      if (url === "/api/v1/adopt") {
        return Response.json(adoptJob(11, "192.168.1.1", 0), { status: 202 });
      }
      if (url === "/api/v1/nodes/1/management") {
        return Response.json({ ...adoptJob(12, "192.168.1.1", 0), kind: "manage", node: 1 });
      }
      if (url === "/api/v1/nodes/1") return Response.json(detail(radioFacts(["2g", "5g", "6g"])));
      if (url === "/api/v1/apply/plan") return Response.json(applyPlan());
      if (url === "/api/v1/apply") return Response.json(applyJob("running"), { status: 202 });
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

const sent = (method: string, url: string) =>
  calls.find((call) => call.method === method && call.url === url)?.body;

async function renderSetup(role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory("/setup");
  const Router = createAppRouter(history);
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router />
      </SignedInProviders>
    </QueryClientProvider>
  ));
  await screen.findByRole("heading", { name: "Set up your internet router", level: 1 });
  return { ws, history };
}

const jobs = (ws: FakeWebSocket, job: unknown) =>
  ws.receive({ type: "jobs_delta", topic: "jobs", jobs: [job] } as never);

describe("the first-run wizard", () => {
  it("finds a factory-default router, adopts it as the gateway and manages it", async () => {
    const user = userEvent.setup();
    const { ws } = await renderSetup();
    sendFleet(ws, []);

    expect(await screen.findByText("GL.iNet GL-MT6000")).toBeInTheDocument();
    expect(screen.getByText("Factory defaults")).toBeInTheDocument();
    expect(screen.getByText(FACTORY.fingerprint)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Use this router" }));
    // Without a password, and as the gateway: nothing else says so before it has internet.
    await waitFor(() =>
      expect(sent("POST", "/api/v1/adopt")).toEqual({
        host: "192.168.1.1",
        host_key: FACTORY.host_key,
        role: "gateway",
        password: "",
      }),
    );
    expect(await screen.findByText("Adopting the router")).toBeInTheDocument();

    jobs(ws, adoptJob(11, "192.168.1.1", 9, "done", { state: "succeeded", node: 1 }));
    await waitFor(() =>
      expect(sent("PUT", "/api/v1/nodes/1/management")).toEqual({ enabled: true }),
    );
    expect(await screen.findByText("Turning on management")).toBeInTheDocument();

    sendFleet(ws, [summary(1, "OpenWrt", { role: "gateway", mode: "managed" })]);
    expect(await screen.findByText("OpenWrt is your gateway")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("heading", { name: "Connect to the internet" })).toBeVisible();
  });

  it("asks what to do with a router that is configured, and leaves for the import to keep it", async () => {
    found = [{ ...FACTORY, blank: false, model: null, release: null }];
    const user = userEvent.setup();
    const { ws, history } = await renderSetup();
    sendFleet(ws, []);

    expect(await screen.findByText("Already configured")).toBeInTheDocument();
    const adopt = screen.getByRole("button", { name: "Adopt and continue" });
    expect(adopt).toBeDisabled();
    await user.type(screen.getByLabelText("Root password"), "hunter2!");
    await user.click(screen.getByRole("radio", { name: /Keep its configuration/ }));
    await user.click(screen.getByRole("button", { name: "Adopt and import" }));
    await waitFor(() =>
      expect(sent("POST", "/api/v1/adopt")).toMatchObject({
        role: "gateway",
        password: "hunter2!",
      }),
    );

    jobs(ws, adoptJob(11, "192.168.1.1", 9, "done", { state: "succeeded", node: 1 }));
    await waitFor(() => expect(history.get()).toBe("/settings/import"));
    // Kept as it is: management stays off.
    expect(sent("PUT", "/api/v1/nodes/1/management")).toBeUndefined();
  });

  it("says so when no router answers, and takes an address", async () => {
    found = [];
    const { ws } = await renderSetup();
    sendFleet(ws, []);
    expect(await screen.findByText("No OpenWrt router answered")).toBeInTheDocument();
    expect(screen.getByLabelText("Router address")).toBeVisible();
    expect(screen.getByRole("button", { name: "Look there" })).toBeDisabled();
  });

  it("saves every choice as one revision, reviews it with the router and applies it", async () => {
    const user = userEvent.setup();
    const { ws } = await renderSetup();
    sendFleet(ws, [summary(1, "OpenWrt", { role: "gateway", mode: "managed" })]);

    await user.click(await screen.findByRole("button", { name: "Continue" }));
    await user.click(await screen.findByRole("button", { name: /Lumen/ }));
    await user.click(screen.getByRole("button", { name: "Continue" }));

    await screen.findByRole("heading", { name: "Plan your addresses" });
    const space = screen.getByLabelText("Address space");
    await user.clear(space);
    await user.type(space, "10.0.0.0/16");
    const router = screen.getByLabelText("Router address");
    await user.clear(router);
    await user.type(router, "10.0.0.150");
    expect(screen.getByText(/DHCP pool: pick one outside/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
    await user.clear(router);
    await user.type(router, "10.0.0.1");
    expect(screen.getByText(/moves to 10.0.0.1/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Continue" }));

    await screen.findByRole("heading", { name: "Start your Wi-Fi" });
    expect(await screen.findByText("On 2.4 GHz and 5 GHz.")).toBeInTheDocument();
    expect(screen.getByText(/6 GHz radio stays off/)).toBeInTheDocument();
    // What is missing shows once Continue is tried.
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByText("Enter a name of up to 32 bytes.")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Network name (SSID)"), "Maple Street");
    await user.type(screen.getByLabelText("Passphrase"), "a long passphrase");
    await user.selectOptions(screen.getByLabelText("Country"), "US");
    await user.click(screen.getByRole("button", { name: "Continue" }));

    await screen.findByRole("heading", { name: "Review and apply" });
    expect(screen.getByText("Maple Street on 2.4 GHz and 5 GHz · US")).toBeInTheDocument();
    expect(screen.getByText("10.0.0.0/24, router at 10.0.0.1")).toBeInTheDocument();
    expect(screen.getByText("This network is renumbered")).toBeInTheDocument();
    expect(sent("PUT", "/api/v1/site")).toBeUndefined();

    await user.click(screen.getByRole("button", { name: "Check with the router" }));
    const apply = await screen.findByRole("button", { name: "Apply" });
    const save = sent("PUT", "/api/v1/site") as SiteSave;
    expect(save.base).toBe(1);
    expect(save.message).toBe("First-run setup");
    expect(save.secrets).toEqual({ "wlans/home/passphrase": "a long passphrase" });
    expect(save.config.wan.preset).toBe("us-lumen-ipoe");
    expect(save.config.addressing.ipv4).toBe("10.0.0.0/16");
    expect(save.config.networks[0]?.ipv4?.address).toBe("10.0.0.1/24");
    expect(save.config.wlans.find((w) => w.id === "home")).toMatchObject({
      ssid: "Maple Street",
      bands: ["2g", "5g"],
    });
    expect(save.config.site.country).toBe("US");
    expect(save.config.nodes.find((n) => n.node === 1)?.managed).toEqual([
      "dhcp",
      "firewall",
      "network",
      "system",
      "wireless",
    ]);

    await user.click(apply);
    await waitFor(() => expect(sent("POST", "/api/v1/apply")).toEqual({ revision: 5 }));
    expect(await screen.findByRole("progressbar")).toBeInTheDocument();
    jobs(ws, applyJob("succeeded"));
    expect(await screen.findByText("Your router is set up")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Skip for now" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open the dashboard" })).toBeEnabled();
  });

  it("is for admins", async () => {
    await renderSetup("viewer");
    expect(screen.getByText("Only admins set up the router.")).toBeInTheDocument();
    expect(sent("GET", "/api/v1/bootstrap/gateway")).toBeUndefined();
  });
});
