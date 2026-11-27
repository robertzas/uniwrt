import { render, screen, waitFor, within } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Account } from "../../../api/gen/Account";
import { type Session, SessionContext } from "../../../lib/session";
import { UsersTab } from "./UsersTab";

const NOW = Date.now();
let calls: { url: string; method: string; body: unknown }[] = [];
let users: Account[] = [];
let answer: ((url: string, method: string) => Response | undefined) | undefined;

const admin: Account = {
  id: 1,
  username: "admin",
  role: "admin",
  created_at: NOW - 86_400_000,
  updated_at: NOW - 86_400_000,
  last_seen_at: NOW - 60_000,
};
const bob: Account = {
  ...admin,
  id: 2,
  username: "bob",
  role: "viewer",
  last_seen_at: NOW - 7_200_000,
};

beforeEach(() => {
  calls = [];
  users = [admin, bob];
  answer = undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? "GET";
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ url, method, body });
      const given = answer?.(url, method);
      if (given) return given;
      if (url === "/api/v1/users" && method === "GET") return Response.json(users);
      if (url === "/api/v1/users" && method === "POST") {
        return Response.json({ id: 3, username: body.username, role: body.role }, { status: 201 });
      }
      if (url.startsWith("/api/v1/users/") && method === "PATCH") {
        const user = users.find((u) => u.id === Number(url.split("/").pop()));
        return Response.json({ ...user, ...body });
      }
      if (url.startsWith("/api/v1/users/") && method === "DELETE") {
        return new Response(null, { status: 204 });
      }
      return new Response(null, { status: 404 });
    }),
  );
});

function renderTab(role: "admin" | "viewer" = "admin") {
  const session: Session = {
    user: role === "admin" ? { id: 1, username: "admin", role } : { id: 2, username: "bob", role },
    signOut: () => Promise.resolve(),
  };
  return render(() => (
    <SessionContext value={session}>
      <UsersTab admin={role === "admin"} />
    </SessionContext>
  ));
}

const writes = () => calls.filter((c) => c.method !== "GET");

describe("Settings → Users", () => {
  it("lists the users with their role and last activity", async () => {
    renderTab();
    const table = await screen.findByRole("table", { name: "Users" });
    const rows = within(table).getAllByRole("row");
    expect(rows[1]).toHaveTextContent(/admin.*You.*Administrator.*Now/);
    expect(rows[2]).toHaveTextContent(/bob.*Viewer.*2h ago/);
    expect(screen.getByText("2 users, 1 administrator")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete admin" })).toBeDisabled();
  });

  it("tells viewers that only administrators manage users", () => {
    renderTab("viewer");
    expect(screen.getByText("Only administrators see and manage users.")).toBeInTheDocument();
    expect(calls).toEqual([]);
  });

  it("adds a user with a role and a first password", async () => {
    const user = userEvent.setup();
    renderTab();
    await screen.findByRole("table", { name: "Users" });
    await user.click(screen.getByRole("button", { name: "Add user" }));
    const dialog = await screen.findByRole("dialog", { name: "Add a user" });
    await user.type(within(dialog).getByLabelText("Username"), "carol");
    await user.click(within(dialog).getByRole("radio", { name: /Administrator/ }));
    await user.type(within(dialog).getByLabelText("Password"), "too short");
    await user.click(within(dialog).getByRole("button", { name: "Add user" }));
    expect(within(dialog).getByText("Use at least 12 characters.")).toBeInTheDocument();
    expect(writes()).toEqual([]);

    await user.type(within(dialog).getByLabelText("Password"), " but now long");
    await user.click(within(dialog).getByRole("button", { name: "Add user" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(writes()).toEqual([
      {
        url: "/api/v1/users",
        method: "POST",
        body: { username: "carol", password: "too short but now long", role: "admin" },
      },
    ]);
    await waitFor(() => expect(calls.filter((c) => c.method === "GET")).toHaveLength(2));
  });

  it("changes a role, sends only what changed and shows the server's refusal", async () => {
    const user = userEvent.setup();
    renderTab();
    await screen.findByRole("table", { name: "Users" });
    await user.click(screen.getByRole("button", { name: "Edit bob" }));
    let dialog = await screen.findByRole("dialog", { name: "Edit bob" });
    await user.click(within(dialog).getByRole("radio", { name: /Administrator/ }));
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(writes()).toEqual([
      { url: "/api/v1/users/2", method: "PATCH", body: { role: "admin" } },
    ]);

    answer = (_, method) =>
      method === "PATCH"
        ? Response.json(
            {
              title: "Conflict",
              detail: "bob is the last admin: make another user an admin first",
            },
            { status: 409, headers: { "content-type": "application/problem+json" } },
          )
        : undefined;
    await user.click(screen.getByRole("button", { name: "Edit bob" }));
    dialog = await screen.findByRole("dialog", { name: "Edit bob" });
    await user.type(within(dialog).getByLabelText("New password"), "a new password for bob");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("bob is the last admin");
    expect(writes()[1]).toEqual({
      url: "/api/v1/users/2",
      method: "PATCH",
      body: { password: "a new password for bob" },
    });
  });

  it("keeps the only administrator one and their own password out of reach", async () => {
    const user = userEvent.setup();
    renderTab();
    await screen.findByRole("table", { name: "Users" });
    await user.click(screen.getByRole("button", { name: "Edit admin" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit admin" });
    expect(within(dialog).getByRole("radio", { name: /Viewer/ })).toBeDisabled();
    expect(within(dialog).getByText(/The only administrator stays one/)).toBeInTheDocument();
    expect(within(dialog).queryByLabelText("New password")).not.toBeInTheDocument();
    expect(within(dialog).getByText(/from the account menu/)).toBeInTheDocument();
  });

  it("deletes a user after asking", async () => {
    const user = userEvent.setup();
    renderTab();
    await screen.findByRole("table", { name: "Users" });
    await user.click(screen.getByRole("button", { name: "Delete bob" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete bob?" });
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(writes()).toEqual([{ url: "/api/v1/users/2", method: "DELETE", body: undefined }]),
    );
  });
});
