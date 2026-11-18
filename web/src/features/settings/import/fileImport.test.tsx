import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SiteImportPreview } from "../../../api/gen/SiteImportPreview";
import { createAppRouter } from "../../../app/routes";
import { Shell } from "../../../app/Shell";
import { summary } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import { revisionMeta, siteView } from "../../../test/site";
import type { FakeWebSocket } from "../../../test/socket";

const EXPORTED = Date.UTC(2026, 10, 17, 9, 30);
const MISSING = "wlans/guest/passphrase";

/** A file exported from revision 7, with sealed secrets unless `secrets` is null. */
const siteFile = (secrets: string | null = "-----BEGIN AGE ENCRYPTED FILE-----") => ({
  format: "uniwrt-site",
  version: 1,
  exported_at: EXPORTED,
  uniwrt: "0.9.0",
  revision: 7,
  nodes: [],
  config: {},
  secrets,
});

/**
 * Importing into revision 1: two changes, the attic AP matched by MAC and an old AP not, the shed
 * AP kept; the home key from the file, the guest key missing (an error until it's given).
 */
function filePreview(opened = true): SiteImportPreview {
  return {
    id: "f-1",
    base: 1,
    file: {
      exported_at: EXPORTED,
      uniwrt: "0.9.0",
      revision: 7,
      schema: 2,
      secrets: true,
      opened,
    },
    changes: [
      { path: ["site", "name"], op: "changed", before: "Demo home", after: "Cabin" },
      { path: ["nodes", "2", "hostname"], op: "changed", before: null, after: "attic" },
    ],
    issues: [
      { path: MISSING, severity: "error", message: "This security mode needs a passphrase" },
      { path: "site/ntp_servers", severity: "warning", message: "No time server is set" },
      { path: "nodes/2/raw/0", severity: "warning", message: "Ignored while observed" },
      { path: "nodes/2/raw/1", severity: "warning", message: "Ignored while observed" },
      { path: "nodes/2/raw/2", severity: "warning", message: "Ignored while observed" },
      { path: "nodes/2/raw/3", severity: "warning", message: "Ignored while observed" },
      { path: "nodes/2/raw/4", severity: "warning", message: "Ignored while observed" },
    ],
    nodes: [
      { file_node: 5, mac: "02:00:00:00:00:02", name: "attic", node: 2 },
      { file_node: 9, mac: "02:00:00:00:00:09", name: "old-ap", node: null },
    ],
    kept: [4],
    secrets: [
      { path: "wlans/home/passphrase", source: opened ? "file" : "kept" },
      { path: MISSING, source: "missing" },
    ],
  };
}

let preview: SiteImportPreview;
let posts: { url: string; body: unknown }[] = [];
let accepted: Response | undefined;

beforeEach(() => {
  preview = filePreview();
  posts = [];
  accepted = undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url === "/api/v1/site/import/preview" || url === "/api/v1/site/import/accept") {
        posts.push({ url, body: JSON.parse(String(init?.body ?? "null")) });
        if (url.endsWith("preview")) return Response.json(preview);
        return accepted ?? Response.json(siteView(2));
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderPage(role: "admin" | "viewer" = "admin") {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory("/settings/import/file");
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
  await screen.findByRole("heading", { name: "Import a site file", level: 1 }, { timeout: 3000 });
  return { ws, history };
}

function sendHead(ws: FakeWebSocket, head: number) {
  ws.receive({ type: "config", topic: "config", head: revisionMeta(head), applied: [] });
}

const pick = (user: ReturnType<typeof userEvent.setup>, text: string, name = "site.json") =>
  user.upload(
    screen.getByLabelText("Site file"),
    new File([text], name, { type: "application/json" }),
  );

describe("Settings › Import a file", () => {
  it("is for admins only", async () => {
    await renderPage("viewer");
    expect(screen.getByText("Only admins import")).toBeVisible();
    expect(screen.queryByLabelText("Site file")).toBeNull();
  });

  it("refuses a file that isn't a site file before sending it", async () => {
    const user = userEvent.setup();
    await renderPage();
    await pick(user, "not json");
    expect(await screen.findByText(/This file isn't JSON/)).toBeVisible();
    await pick(user, '{"format": "something-else"}');
    expect(await screen.findByText("This JSON isn't a uniwrt site file.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Preview" })).toBeDisabled();
    expect(posts).toEqual([]);
  });

  it("previews with the passphrase, takes the missing secret, accepts, then offers the review", async () => {
    const user = userEvent.setup();
    const { history } = await renderPage();
    await pick(user, JSON.stringify(siteFile()));
    expect(await screen.findByText(/Revision 7 of uniwrt 0\.9\.0, exported/)).toBeVisible();
    await user.type(screen.getByLabelText("Passphrase"), "correct horse battery");
    await user.click(screen.getByRole("button", { name: "Preview" }));

    const summaryCard = await screen.findByRole("region", { name: "Import a file" });
    expect(within(summaryCard).getByText("Into revision 1")).toBeVisible();
    expect(within(summaryCard).getByText("2 changes to the site")).toBeVisible();
    expect(within(summaryCard).getByText("1 of 2 devices matched")).toBeVisible();
    expect(within(summaryCard).getByText("1 secret missing")).toBeVisible();
    expect(
      within(summaryCard).getByText("Give the missing secrets below to accept it."),
    ).toBeVisible();
    // The missing secret's error shows at its field, the rest of the issues on the card.
    expect(within(summaryCard).queryByText("Why it can't be accepted")).toBeNull();
    expect(within(summaryCard).getByText("No time server is set")).toBeVisible();
    // Past five, the warnings wait for "Show all".
    expect(within(summaryCard).getAllByText("Ignored while observed")).toHaveLength(4);
    await user.click(within(summaryCard).getByRole("button", { name: "Show all 6" }));
    expect(within(summaryCard).getAllByText("Ignored while observed")).toHaveLength(5);
    const accept = within(summaryCard).getByRole("button", { name: "Accept as revision 2" });
    expect(accept).toBeDisabled();
    expect(posts[0]).toEqual({
      url: "/api/v1/site/import/preview",
      body: { file: siteFile(), passphrase: "correct horse battery" },
    });

    const secrets = screen.getByRole("region", { name: "Secrets" });
    expect(within(secrets).getByText("From the file")).toBeVisible();
    expect(within(secrets).getByText("This security mode needs a passphrase")).toBeVisible();
    const devices = screen.getByRole("region", { name: "Devices" });
    expect(within(devices).getByText("02:00:00:00:00:09")).toBeVisible();
    expect(within(devices).getByText("Not adopted here: its settings are left out")).toBeVisible();
    expect(within(devices).getByText("Attic AP")).toBeVisible();
    expect(within(devices).getByText("Shed AP")).toBeVisible();
    const changes = screen.getByRole("region", { name: "Changes to the site" });
    expect(within(changes).getByText(/Attic AP/)).toBeVisible();

    await user.type(
      within(secrets).getByLabelText(`Value of wlans › guest › passphrase`),
      "guest wifi key",
    );
    expect(within(summaryCard).queryByText("1 secret missing")).toBeNull();
    await user.type(within(summaryCard).getByLabelText("Revision message"), "From the cabin");
    await user.click(accept);
    expect(await screen.findByText("Imported as revision 2")).toBeVisible();
    expect(posts[1]).toEqual({
      url: "/api/v1/site/import/accept",
      body: { id: "f-1", message: "From the cabin", secrets: { [MISSING]: "guest wifi key" } },
    });
    expect(screen.queryByRole("region", { name: "Secrets" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Review changes" }));
    await waitFor(() => expect(history.get()).toBe("/changes"));
  });

  it("shows a value the accept refused at its field, keeping the preview", async () => {
    const user = userEvent.setup();
    accepted = Response.json(
      {
        title: "Unprocessable Entity",
        detail: "The import has an error",
        issues: [
          { path: MISSING, severity: "error", message: "A passphrase has 8 to 63 characters" },
        ],
      },
      { status: 422, headers: { "content-type": "application/problem+json" } },
    );
    await renderPage();
    await pick(user, JSON.stringify(siteFile()));
    await user.type(screen.getByLabelText("Passphrase"), "correct horse battery");
    await user.click(screen.getByRole("button", { name: "Preview" }));
    const secrets = await screen.findByRole("region", { name: "Secrets" });
    await user.type(within(secrets).getByLabelText(/Value of/), "short");
    await user.click(screen.getByRole("button", { name: "Accept as revision 2" }));
    expect(await within(secrets).findByText("A passphrase has 8 to 63 characters")).toBeVisible();
    expect(posts.filter((p) => p.url.endsWith("preview"))).toHaveLength(1);
  });

  it("goes back for the passphrase when the secrets weren't opened", async () => {
    const user = userEvent.setup();
    preview = filePreview(false);
    await renderPage();
    await pick(user, JSON.stringify(siteFile()));
    await user.click(screen.getByRole("button", { name: "Preview" }));
    expect(posts[0]?.body).toEqual({ file: siteFile() });
    expect(await screen.findByText(/The file's secrets weren't opened/)).toBeVisible();
    expect(screen.getByText("Kept from the site")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Enter the passphrase" }));
    // The file stays picked.
    expect(await screen.findByText(/Revision 7 of uniwrt 0\.9\.0/)).toBeVisible();
    expect(screen.getByLabelText("Passphrase")).toBeVisible();
    expect(screen.getByRole("button", { name: "Preview" })).toBeEnabled();
  });

  it("asks for a new preview once another revision is saved", async () => {
    const user = userEvent.setup();
    const { ws } = await renderPage();
    await pick(user, JSON.stringify(siteFile(null)));
    expect(await screen.findByText(/It holds no secrets/)).toBeVisible();
    expect(screen.queryByLabelText("Passphrase")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Preview" }));
    await screen.findByText("Into revision 1");
    sendHead(ws, 2);
    expect(await screen.findByText(/Revision 2 was saved since this preview/)).toBeVisible();
    preview = { ...filePreview(), id: "f-2", base: 2 };
    await user.click(screen.getByRole("button", { name: "Preview again" }));
    expect(await screen.findByText("Into revision 2")).toBeVisible();
    expect(screen.queryByText(/was saved since this preview/)).toBeNull();
    expect(posts).toHaveLength(2);
  });
});
