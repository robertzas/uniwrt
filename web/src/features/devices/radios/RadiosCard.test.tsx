import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NodeFacts } from "../../../api/gen/NodeFacts";
import type { SiteConfig } from "../../../api/gen/SiteConfig";
import type { SiteSave } from "../../../api/gen/SiteSave";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { detail, facts } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders } from "../../../test/providers";
import { phy5, radioFacts } from "../../../test/radios";
import { revisionMeta, siteConfig, siteView } from "../../../test/site";

let calls: { method: string; url: string; body: unknown }[] = [];
let head = 2;
let stored: SiteConfig | undefined;

/** The site fixing 2.4 GHz to 6, the node managing its Wi-Fi. */
function config(): SiteConfig {
  const base = siteConfig();
  return {
    ...base,
    radio: {
      ...base.radio,
      bands: base.radio.bands.map((p) => (p.band === "2g" ? { ...p, channel: { fixed: 6 } } : p)),
    },
    nodes: [
      {
        node: 1,
        hostname: null,
        managed: ["wireless"],
        radios: [],
        ports: [],
        raw: [],
        reboot: { kind: "site" as const },
        led_schedule: true,
        ssh: { key_only: false, management_only: false },
      },
    ],
  };
}

function withRadios(): NodeFacts {
  const f = facts(1);
  return {
    ...f,
    phys: [phy5("phy1")],
    radios: [
      radioFacts({ name: "radio0", band: "2g", phy: "phy0", channel: 6, width: 20 }),
      radioFacts({ name: "radio1", band: "5g", phy: "phy1" }),
    ],
  };
}

beforeEach(() => {
  calls = [];
  head = 2;
  stored = undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site" && method === "GET")
        return Response.json(siteView(head, stored ?? config()));
      if (url === "/api/v1/site" && method === "PUT") {
        head += 1;
        stored = (body as SiteSave).config;
        return Response.json(siteView(head, stored));
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

describe("Device › Radios", () => {
  it("shows what is fixed and fixes a radio's channel, width and power by hand", async () => {
    const user = userEvent.setup();
    const { socket, ws } = openTestSocket();
    const Router = createAppRouter(memoryHistory("/devices/1"));
    render(() => (
      <QueryClientProvider client={new QueryClient()}>
        <SignedInProviders socket={socket}>
          <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
        </SignedInProviders>
      </QueryClientProvider>
    ));
    await waitFor(() =>
      expect(ws.sent).toContainEqual({ sub: expect.arrayContaining(["node:1"]) }),
    );
    ws.receive({ type: "node_snapshot", topic: "node:1", node: detail(withRadios()) });
    ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
    const card = await screen.findByRole("region", { name: "Radios" });
    // The band's fixed channel shows on the 2.4 GHz radio.
    expect(await within(card).findByText("Fixed: channel 6")).toBeInTheDocument();

    await user.click(within(card).getByRole("button", { name: "Edit radio1" }));
    const dialog = await screen.findByRole("dialog", { name: "5 GHz radio radio1" });
    const channel = within(dialog).getByLabelText("Channel");
    const options = within(channel)
      .getAllByRole("option")
      .map((o) => o.textContent);
    expect(options).toEqual([
      "As the band: Auto",
      "Auto: the RF planner or the device chooses",
      "36",
      "40",
      "100 · DFS · radar heard",
    ]);
    await user.selectOptions(channel, "40");
    await user.selectOptions(within(dialog).getByLabelText("Width"), "40");
    const power = within(dialog).getByLabelText("Transmit power");
    // Up to the phy's 23 dBm.
    expect(within(power).queryByRole("option", { name: "24 dBm" })).toBeNull();
    await user.selectOptions(power, "17");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText(/Saved as revision 3/)).toBeInTheDocument();
    const put = calls.find((c) => c.method === "PUT")?.body as SiteSave;
    expect(put.message).toBe("radio1 on gw set by hand");
    expect(put.config.nodes[0]?.radios).toEqual([
      {
        radio: "radio1",
        disabled: false,
        channel: { fixed: 40 },
        width: 40,
        tx_power: { dbm: 17 },
      },
    ]);
    expect(screen.queryByRole("dialog")).toBeNull();
    ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
    expect(
      await within(card).findByText("Fixed: channel 40 · 40 MHz · 17 dBm"),
    ).toBeInTheDocument();

    // Back to the site's settings: the override goes.
    await user.click(within(card).getByRole("button", { name: "Edit radio1" }));
    const again = await screen.findByRole("dialog", { name: "5 GHz radio radio1" });
    await user.selectOptions(within(again).getByLabelText("Channel"), "site");
    await user.selectOptions(within(again).getByLabelText("Width"), "site");
    await user.selectOptions(within(again).getByLabelText("Transmit power"), "site");
    await user.click(within(again).getByRole("button", { name: "Save" }));
    expect(await within(card).findByText(/Saved as revision 4/)).toBeInTheDocument();
    const last = calls.filter((c) => c.method === "PUT").at(-1)?.body as SiteSave;
    expect(last.message).toBe("radio1 on gw follows the site again");
    expect(last.config.nodes[0]?.radios).toEqual([]);
  });
});
