import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ImportPreview } from "../../../api/gen/ImportPreview";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { summary } from "../../../test/fixtures";
import { importPreview } from "../../../test/import";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import { revisionMeta, siteView } from "../../../test/site";
import type { FakeWebSocket } from "../../../test/socket";

let preview: ImportPreview;
let previews = 0;
let accepted: Response | undefined;
let posts: { url: string; body: unknown }[] = [];

beforeEach(() => {
  preview = importPreview();
  previews = 0;
  accepted = undefined;
  posts = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url === "/api/v1/import/preview") {
        previews += 1;
        return Response.json({ ...preview, id: `p-${previews}` });
      }
      if (url === "/api/v1/import/accept") {
        posts.push({ url, body: JSON.parse(String(init?.body ?? "null")) });
        return accepted ?? Response.json(siteView(2));
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

function sendHead(ws: FakeWebSocket, head: number) {
  ws.receive({
    type: "config",
    topic: "config",
    head: revisionMeta(head),
    applied: [],
  });
}

async function renderImport(role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory("/settings/import");
  const Router = createAppRouter(history);
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [
    summary(1, "Gateway", { role: "gateway" }),
    summary(2, "Attic AP"),
    summary(4, "Shed AP"),
  ]);
  sendHead(ws, 1);
  await screen.findByRole(
    "heading",
    { name: "Import from the devices", level: 1 },
    { timeout: 3000 },
  );
  return { ws, history };
}

describe("Settings › Import", () => {
  it("reads the devices and shows the site's changes, each device's fate and residual", async () => {
    const user = userEvent.setup();
    await renderImport();
    const summaryCard = await screen.findByRole("region", { name: "Import" });
    expect(summaryCard).toHaveTextContent("Into revision 1");
    expect(summaryCard).toHaveTextContent("2 imported");
    expect(summaryCard).toHaveTextContent("1 not read");
    expect(summaryCard).toHaveTextContent("2 changes to the site");
    expect(summaryCard).toHaveTextContent("Accepting saves this as revision 2");

    const changes = screen.getByRole("region", { name: "Changes to the site" });
    expect(changes).toHaveTextContent("10.0.0.1/24");
    expect(changes).not.toHaveTextContent("config/");

    const devices = screen.getByRole("region", { name: "Devices" });
    const cards = within(devices).getAllByRole("region", { name: /AP$|Gateway/ });
    expect(cards.map((c) => c.getAttribute("aria-label"))).toEqual([
      "Gateway",
      "Attic AP",
      "Shed AP",
    ]);
    const [gateway, attic, shed] = cards as [HTMLElement, HTMLElement, HTMLElement];
    expect(gateway).toHaveTextContent("93% in the model");
    expect(gateway).toHaveTextContent("2 mapped · 1 kept raw");
    expect(gateway).not.toHaveTextContent("batwired");
    await user.click(within(gateway).getByRole("button", { name: "Gateway's sections" }));
    expect(gateway).toHaveTextContent("config interface 'batwired'Kept raw");
    expect(gateway).toHaveTextContent("config zone @zone → 'lan'Mapped· keeps log, mtu_fix");
    expect(gateway).toHaveTextContent("What the first apply would still change");
    expect(gateway).toHaveTextContent("/etc/config/dhcp1 changed");
    expect(gateway).not.toHaveTextContent("force");
    await user.click(within(gateway).getByRole("button", { name: "The dhcp package" }));
    expect(gateway).toHaveTextContent("option force '1'");

    // Its warnings, once per message with their count.
    expect(attic).toHaveTextContent("Ignored while wireless is off×2");
    expect(attic).toHaveTextContent("1 dropped");
    expect(summaryCard).not.toHaveTextContent("Ignored while");

    expect(shed).toHaveTextContent("Not read");
    expect(shed).toHaveTextContent("connection refused");
    expect(shed).toHaveTextContent("keeps the settings the site has");
  });

  it("accepts the preview reviewed with a message, then offers the review", async () => {
    const user = userEvent.setup();
    const { ws, history } = await renderImport();
    const summaryCard = await screen.findByRole("region", { name: "Import" });
    await user.type(within(summaryCard).getByLabelText("Revision message"), "From the fleet");
    await user.click(within(summaryCard).getByRole("button", { name: "Accept as revision 2" }));
    expect(posts).toEqual([
      { url: "/api/v1/import/accept", body: { id: "p-1", message: "From the fleet" } },
    ]);
    const done = await screen.findByRole("region", { name: "Import" });
    await within(done).findByText("Imported as revision 2");
    // The new head is the import's own revision: it is not stale.
    sendHead(ws, 2);
    expect(screen.queryByText(/was saved since this preview/)).toBeNull();
    await user.click(within(done).getByRole("button", { name: "Review changes" }));
    await waitFor(() => expect(history.get()).toBe("/changes"));
  });

  it("reads again when the preview expired, and blocks a stale one", async () => {
    const user = userEvent.setup();
    const { ws } = await renderImport();
    const summaryCard = await screen.findByRole("region", { name: "Import" });
    accepted = Response.json(
      { title: "Not Found", detail: "This preview is no longer held: import again" },
      { status: 404, headers: { "content-type": "application/problem+json" } },
    );
    await user.click(within(summaryCard).getByRole("button", { name: "Accept as revision 2" }));
    expect(await screen.findByText("The import could not be saved")).toBeVisible();
    await waitFor(() => expect(previews).toBe(2));

    sendHead(ws, 2);
    const stale = await screen.findByText(/Revision 2 was saved since this preview/);
    expect(stale).toBeVisible();
    expect(screen.getByRole("button", { name: "Accept as revision 2" })).toBeDisabled();
  });

  it("can't be accepted while the import has errors, or when it changes nothing", async () => {
    preview.issues.push({
      path: "wlans/home/passphrase",
      severity: "error",
      message: "A passphrase has 8 to 63 characters (as the node holds it)",
    });
    const user = userEvent.setup();
    await renderImport();
    const summaryCard = await screen.findByRole("region", { name: "Import" });
    expect(within(summaryCard).getByRole("alert")).toHaveTextContent("8 to 63 characters");
    expect(summaryCard).toHaveTextContent("Fix the errors below on the devices");
    expect(within(summaryCard).queryByRole("button", { name: /Accept/ })).toBeNull();

    preview = { ...importPreview(), changes: [] };
    await user.click(await within(summaryCard).findByRole("button", { name: "Read again" }));
    await waitFor(() => expect(summaryCard).toHaveTextContent("Nothing new"));
    expect(summaryCard).toHaveTextContent("there is nothing to import");
    expect(within(summaryCard).queryByRole("button", { name: /Accept/ })).toBeNull();
  });

  it("is for admins only", async () => {
    await renderImport("viewer");
    expect(await screen.findByText("Only admins import")).toBeVisible();
    expect(previews).toBe(0);
  });
});
