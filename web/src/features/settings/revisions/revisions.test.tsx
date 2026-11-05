import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RevisionDiff } from "../../../api/gen/RevisionDiff";
import type { RevisionMeta } from "../../../api/gen/RevisionMeta";
import { createAppRouter } from "../../../app/routes";
import { summary } from "../../../test/fixtures";
import { openTestSocket, SignedInProviders, sendFleet } from "../../../test/providers";
import { siteView } from "../../../test/site";

const NOW = Date.now();
/** Reading the history needs no admin. */
const VIEWER = "viewer";
const SECRET_A = `config/${"a".repeat(32)}`;
const SECRET_B = `config/${"b".repeat(32)}`;
let calls: string[] = [];
let pages: Record<string, { revisions: RevisionMeta[]; more: boolean }> = {};
let diffs: Record<string, RevisionDiff> = {};

const meta = (id: number, extra: Partial<RevisionMeta> = {}): RevisionMeta => ({
  id,
  parent: id > 1 ? id - 1 : null,
  kind: "edit",
  restored_from: null,
  author_id: 1,
  author: "admin",
  message: "",
  created_at: NOW - (10 - id) * 60_000,
  schema: 1,
  changes: 1,
  ...extra,
});

const defaults = meta(1, { kind: "defaults", author: null, author_id: null, changes: 0 });
const iot = meta(2, { message: "IoT network", changes: 2 });
const wifi = meta(3, { message: "New Wi-Fi key", changes: 2 });

beforeEach(() => {
  calls = [];
  pages = { "": { revisions: [wifi, iot, defaults], more: false } };
  diffs = {
    "3": {
      from: 2,
      to: 3,
      changes: [
        {
          path: ["wlans", "home", "passphrase"],
          op: "changed",
          before: SECRET_A,
          after: SECRET_B,
        },
        { path: ["nodes", "2", "hostname"], op: "changed", before: null, after: "attic" },
      ],
    },
    "3?against=1": {
      from: 1,
      to: 3,
      changes: [
        {
          path: ["networks", "iot"],
          op: "added",
          before: null,
          after: { id: "iot", vlan: 20 },
        },
      ],
    },
    "1": { from: null, to: 1, changes: [] },
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      calls.push(init?.method === "POST" ? `POST ${url} ${init.body}` : url);
      if (init?.method === "POST" && url.endsWith("/restore")) return Response.json(siteView(4));
      const page = url.match(/^\/api\/v1\/revisions\?limit=\d+(?:&before=(\d+))?$/);
      if (page) return Response.json(pages[page[1] ?? ""] ?? { revisions: [], more: false });
      const diff = url.match(/^\/api\/v1\/revisions\/(\d+)\/diff(.*)$/);
      if (diff) {
        const answer = diffs[`${diff[1]}${diff[2]}`];
        return answer ? Response.json(answer) : new Response(null, { status: 500 });
      }
      return Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" });
    }),
  );
});

async function renderRevisions(role: "admin" | "viewer" = VIEWER) {
  const { socket, ws } = openTestSocket();
  const history = memoryHistory("/settings?tab=revisions");
  const Router = createAppRouter(history);
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket} role={role}>
        <Router />
      </SignedInProviders>
    </QueryClientProvider>
  ));
  sendFleet(ws, [summary(1, "Gateway", { role: "gateway" }), summary(2, "Attic AP")]);
  await screen.findByRole("heading", { name: "Settings", level: 1 });
  lastHistory = history;
  return screen.findByRole("region", { name: "Revisions" });
}

let lastHistory: ReturnType<typeof memoryHistory> | undefined;

describe("Settings › Revisions", () => {
  it("lists the revisions newest first with their author, message and size", async () => {
    const list = await renderRevisions();
    const rows = await within(list).findAllByRole("listitem");
    expect(rows).toHaveLength(3);
    expect(within(list).getByText("3 revisions")).toBeVisible();
    expect(rows[0]).toHaveTextContent("#3");
    expect(rows[0]).toHaveTextContent("New Wi-Fi key");
    expect(rows[0]).toHaveTextContent("Latest");
    expect(rows[0]).toHaveTextContent("by admin · 2 changes");
    expect(rows[2]).toHaveTextContent("The defaults");
    expect(rows[2]).toHaveTextContent("by uniwrt · No changes");
    expect(rows[2]).not.toHaveTextContent("Latest");
  });

  it("opens a revision's changes from its parent, never showing a secret", async () => {
    const user = userEvent.setup();
    const list = await renderRevisions();
    await user.click(await within(list).findByRole("button", { name: "Revision 3" }));
    const changes = await within(list).findByText("Changed (secret)");
    const item = changes.closest("li") as HTMLElement;
    expect(item).toHaveTextContent("wlans › home › passphrase");
    expect(list).not.toHaveTextContent(SECRET_A);
    expect(list).not.toHaveTextContent(SECRET_B);
    expect(list).not.toHaveTextContent("config/");
    expect(within(list).getByText(/Changes from #2 to #3/)).toHaveTextContent("2 changed");
    // Devices are named in paths.
    expect(list).toHaveTextContent("nodes › Attic AP › hostname");
    expect(within(list).getByText("attic")).toBeVisible();
    expect(calls).toContain("/api/v1/revisions/3/diff");
  });

  it("compares with another revision", async () => {
    const user = userEvent.setup();
    const list = await renderRevisions();
    await user.click(await within(list).findByRole("button", { name: "Revision 3" }));
    const compare = await within(list).findByLabelText("Compare with");
    expect(
      within(compare)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["Its parent, #2", "#1 · The defaults"]);
    await user.selectOptions(compare, "1");
    await within(list).findByText(/Changes from #1 to #3/);
    expect(calls).toContain("/api/v1/revisions/3/diff?against=1");
    expect(list).toHaveTextContent("networks › iot");
    expect(list).toHaveTextContent("vlan");
    expect(within(list).getByText("20")).toBeVisible();
  });

  it("says the first revision has nothing before it, and pages older ones", async () => {
    const user = userEvent.setup();
    pages = {
      "": { revisions: [wifi, iot], more: true },
      "2": { revisions: [defaults], more: false },
    };
    const list = await renderRevisions();
    expect(await within(list).findByText("Latest 2")).toBeVisible();
    await user.click(within(list).getByRole("button", { name: "Show older" }));
    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(3));
    expect(within(list).queryByRole("button", { name: "Show older" })).toBeNull();
    expect(calls).toContain("/api/v1/revisions?limit=50&before=2");
    await user.click(within(list).getByRole("button", { name: "Revision 1" }));
    expect(await within(list).findByText(/first revision/)).toBeVisible();
  });

  it("lets admins restore an older revision, then review it", async () => {
    const user = userEvent.setup();
    const list = await renderRevisions("admin");
    await user.click(await within(list).findByRole("button", { name: "Revision 3" }));
    // The latest is what restoring would copy already.
    await within(list).findByText(/Changes from #2 to #3/);
    expect(within(list).queryByRole("button", { name: "Restore revision 3" })).toBeNull();
    await user.click(within(list).getByRole("button", { name: "Revision 2" }));
    await user.click(await within(list).findByRole("button", { name: "Restore revision 2" }));
    const dialog = await screen.findByRole("dialog", { name: "Restore revision 2?" });
    await user.click(within(dialog).getByRole("button", { name: "Restore" }));
    expect(await within(dialog).findByText("Revision 2 restored as revision 4")).toBeVisible();
    expect(calls).toContain('POST /api/v1/revisions/2/restore {"base":3}');
    await user.click(within(dialog).getByRole("button", { name: "Review and apply" }));
    await waitFor(() => expect(lastHistory?.get()).toBe("/changes"));
  });

  it("offers viewers no restore", async () => {
    const user = userEvent.setup();
    const list = await renderRevisions();
    await user.click(await within(list).findByRole("button", { name: "Revision 2" }));
    await within(list).findByLabelText("Compare with");
    expect(within(list).queryByRole("button", { name: /Restore/ })).toBeNull();
    expect(within(list).queryByRole("button", { name: "Import from the devices" })).toBeNull();
  });

  it("links admins to the import", async () => {
    const user = userEvent.setup();
    const list = await renderRevisions("admin");
    await user.click(within(list).getByRole("button", { name: "Import from the devices" }));
    await waitFor(() => expect(lastHistory?.get()).toBe("/settings/import"));
  });

  it("offers to try again when the list fails", async () => {
    pages = {};
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) =>
        url.startsWith("/api/v1/revisions")
          ? new Response(null, { status: 500 })
          : Response.json({ status: "ok", version: "1", commit: "x", built_at: "y" }),
      ),
    );
    const { socket } = openTestSocket();
    const Router = createAppRouter(memoryHistory("/settings?tab=revisions"));
    render(() => (
      <QueryClientProvider client={new QueryClient()}>
        <SignedInProviders socket={socket}>
          <Router />
        </SignedInProviders>
      </QueryClientProvider>
    ));
    expect(await screen.findByText("The revisions could not be loaded.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Try again" })).toBeVisible();
  });
});
