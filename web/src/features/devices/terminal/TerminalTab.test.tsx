import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { detail, facts, summary } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import { revisionMeta, siteView } from "../../../test/site";
import { TerminalSocket } from "../../../test/terminalSocket";

/** xterm.js stands in: jsdom has no canvas or layout. */
const fake = vi.hoisted(() => {
  class FakeTerm {
    written: string[] = [];
    focused = 0;
    disposed = false;
    size = { cols: 100, rows: 30 };
    input: (bytes: Uint8Array<ArrayBuffer>) => void = () => undefined;
    constructor(readonly host: HTMLElement) {}
    write(data: Uint8Array | string) {
      this.written.push(typeof data === "string" ? data : new TextDecoder().decode(data));
    }
    onInput(handler: (bytes: Uint8Array<ArrayBuffer>) => void) {
      this.input = handler;
    }
    onResize() {}
    fit() {
      return this.size;
    }
    focus() {
      this.focused++;
    }
    dispose() {
      this.disposed = true;
    }
  }
  return { FakeTerm, terms: [] as InstanceType<typeof FakeTerm>[] };
});

vi.mock("./xterm", () => ({
  createXterm: async (host: HTMLElement) => {
    const term = new fake.FakeTerm(host);
    fake.terms.push(term);
    return term;
  },
}));

beforeEach(() => {
  fake.terms.length = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      if (url === "/api/v1/site") return Response.json(siteView(2));
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderTab(options: { role?: "admin" | "viewer"; offline?: boolean } = {}) {
  const { socket, ws } = openTestSocket();
  const Router = createAppRouter(memoryHistory("/devices/1?tab=terminal"));
  const view = render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={options.role ?? "admin"}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  await waitFor(() => {
    const sent = ws.sent.at(-1) as { sub?: string[] } | undefined;
    expect(sent?.sub).toContain("node:1");
  });
  sendFleet(ws, [
    summary(
      1,
      "gw",
      options.offline ? { status: { state: "offline", since: 1, reasons: [] } } : {},
    ),
  ]);
  ws.receive({ type: "node_snapshot", topic: "node:1", node: detail(facts(1)) });
  ws.receive({ type: "config", topic: "config", head: revisionMeta(2), applied: [] });
  await screen.findByRole("heading", { name: "gw" });
  // The hub's socket is open; the terminal's sockets are the fake's from here on.
  TerminalSocket.instances = [];
  vi.stubGlobal("WebSocket", TerminalSocket);
  return view;
}

async function openTerminal() {
  const card = await screen.findByRole("region", { name: "Terminal on gw" });
  await userEvent.click(within(card).getByRole("button", { name: "Open terminal" }));
  await waitFor(() => expect(TerminalSocket.instances).toHaveLength(1));
  const ws = TerminalSocket.last;
  ws.open();
  const term = fake.terms[0];
  if (!term) throw new Error("no terminal");
  return { card, ws, term };
}

describe("TerminalTab", () => {
  it("is for admins only", async () => {
    await renderTab({ role: "viewer" });
    expect(await screen.findByText("Only admins can open a terminal.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open terminal" })).not.toBeInTheDocument();
  });

  it("can't open on an offline device", async () => {
    await renderTab({ offline: true });
    const card = await screen.findByRole("region", { name: "Terminal on gw" });
    expect(within(card).getByRole("button", { name: "Open terminal" })).toBeDisabled();
    expect(within(card).getByText("The device is offline.")).toBeInTheDocument();
  });

  it("opens the shell at the terminal's size and carries bytes both ways", async () => {
    await renderTab();
    const card = await screen.findByRole("region", { name: "Terminal on gw" });
    expect(within(card).getByRole("note")).toHaveTextContent("drift");
    const { ws, term } = await openTerminal();
    expect(ws.url).toMatch(/\/api\/v1\/nodes\/1\/terminal\?cols=100&rows=30$/);
    expect(within(card).getByRole("status")).toHaveTextContent("Connecting…");

    ws.text({ type: "opened" });
    await waitFor(() => expect(within(card).getByRole("status")).toHaveTextContent("Connected"));
    expect(term.focused).toBe(1);
    // The size the URL asked for isn't sent again.
    expect(ws.texts).toEqual([]);

    ws.output("root@gw:~# ");
    expect(term.written).toEqual(["root@gw:~# "]);
    term.input(new TextEncoder().encode("uptime\r"));
    expect(ws.bytes.map((bytes) => new TextDecoder().decode(bytes))).toEqual(["uptime\r"]);
    expect(term.host.isConnected).toBe(true);
  });

  it("says how it ended, and reconnects in the same terminal", async () => {
    await renderTab();
    const { card, ws, term } = await openTerminal();
    ws.text({ type: "opened" });
    ws.text({ type: "ended", exit_code: 0, reason: "The shell ended" });
    ws.drop(1000);
    await waitFor(() =>
      expect(within(card).getByRole("status")).toHaveTextContent("The shell ended"),
    );
    expect(term.written.at(-1)).toContain("[The shell ended]");

    await userEvent.click(within(card).getByRole("button", { name: "Reconnect" }));
    await waitFor(() => expect(TerminalSocket.instances).toHaveLength(2));
    expect(fake.terms).toHaveLength(1);
    expect(within(card).getByRole("status")).toHaveTextContent("Connecting…");
  });

  it("disconnects on request", async () => {
    await renderTab();
    const { card, ws } = await openTerminal();
    ws.text({ type: "opened" });
    await userEvent.click(await within(card).findByRole("button", { name: "Disconnect" }));
    expect(ws.closedWith).toBe(1000);
    expect(within(card).getByRole("status")).toHaveTextContent("Disconnected");
  });

  it("goes on behind other tabs and ends with the page", async () => {
    const view = await renderTab();
    const { ws, term } = await openTerminal();
    ws.text({ type: "opened" });
    await userEvent.click(screen.getByRole("tab", { name: "Overview" }));
    await waitFor(() => expect(term.host.isConnected).toBe(false));
    await userEvent.click(screen.getByRole("tab", { name: "Terminal" }));
    await waitFor(() => expect(term.host.isConnected).toBe(true));
    expect(TerminalSocket.instances).toHaveLength(1);
    expect(ws.closedWith).toBeUndefined();

    view.unmount();
    expect(ws.closedWith).toBe(1000);
    expect(term.disposed).toBe(true);
  });
});
