import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ServerMessage } from "../../api/gen/ServerMessage";
import type { SiteSave } from "../../api/gen/SiteSave";
import { createAppRouter } from "../../app/routes";
import { detail } from "../../test/clients";
import { summary } from "../../test/fixtures";
import { metricsAnswer } from "../../test/metrics";
import { openTestSocket, SignedInProviders, sendFleet } from "../../test/providers";
import { siteView } from "../../test/site";

const MAC = "f0:18:98:00:00:01";
let calls: { method: string; url: string; body: unknown }[] = [];
let existing = false;

beforeEach(() => {
  calls = [];
  existing = false;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ method, url, body });
      if (url === "/api/v1/site" && method === "GET") return Response.json(siteView(2));
      if (url === "/api/v1/site" && method === "PUT") {
        return Response.json(siteView(3, (body as SiteSave).config));
      }
      if (url.startsWith("/api/v1/ipam/reservation?")) {
        return Response.json({ network: "lan", ip: "192.168.1.40", existing });
      }
      if (url.startsWith("/api/v1/metrics?")) return Response.json(metricsAnswer(url));
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderClient() {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory(`/clients/${MAC}`));
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket}>
        <Router />
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "Gateway", { role: "gateway" })]);
  const topic = `client:${MAC}`;
  await waitFor(() => expect(ws.sent).toContainEqual({ sub: expect.arrayContaining([topic]) }));
  ws.receive({ type: "client_snapshot", topic, client: detail() } as ServerMessage);
  await screen.findByRole("heading", { name: "Annas-iPhone" });
}

describe("Client › Fix IP", () => {
  it("reserves the client's current address on its network", async () => {
    const user = userEvent.setup();
    await renderClient();
    await user.click(screen.getByRole("button", { name: "Fix IP" }));
    const dialog = await screen.findByRole("dialog", { name: "Fix this client's address" });
    expect(await within(dialog).findByText("On Default")).toBeInTheDocument();
    expect(within(dialog).getByText(/Its current address/)).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Host name (optional)")).toHaveValue("annas-iphone");
    expect(calls.map((c) => c.url)).toContain(
      `/api/v1/ipam/reservation?ip=192.168.1.40&mac=${encodeURIComponent(MAC)}`,
    );
    await user.click(within(dialog).getByRole("button", { name: "Reserve" }));
    expect(await within(dialog).findByText(/Saved as revision 3/)).toBeInTheDocument();
    const put = calls.find((c) => c.method === "PUT")?.body as SiteSave;
    expect(put.message).toBe("Fix IP: Annas-iPhone at 192.168.1.40");
    expect(put.config.networks[0]?.ipv4?.dhcp?.reservations).toEqual([
      { mac: MAC, ip: "192.168.1.40", name: "annas-iphone" },
    ]);
  });

  it("says when the client has a reservation already", async () => {
    existing = true;
    const user = userEvent.setup();
    await renderClient();
    await user.click(screen.getByRole("button", { name: "Fix IP" }));
    const dialog = await screen.findByRole("dialog", { name: "Fix this client's address" });
    expect(
      await within(dialog).findByText("Reserved already: 192.168.1.40 on Default."),
    ).toBeInTheDocument();
    expect(within(dialog).queryByRole("button", { name: "Reserve" })).toBeNull();
  });
});
