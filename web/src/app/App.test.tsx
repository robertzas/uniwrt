import { render, screen, waitFor } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AuthState } from "../api/gen/AuthState";
import type { UserInfo } from "../api/gen/UserInfo";
import { FakeWebSocket, stubWebSocket } from "../test/socket";
import { Gate } from "./App";

const admin: UserInfo = { id: 1, username: "admin", role: "admin" };

function authState(fields: Partial<AuthState>): AuthState {
  return { setup: "done", setup_closes_in_s: null, user: null, min_password_chars: 12, ...fields };
}

const problem = (status: number, detail: string, headers: Record<string, string> = {}) =>
  new Response(JSON.stringify({ type: "about:blank", title: "Error", status, detail }), {
    status,
    headers: { "content-type": "application/problem+json", ...headers },
  });

/** Answers the auth routes; `login` decides what a sign-in gets. */
function stubServer(state: AuthState, login: () => Response = () => Response.json(admin)) {
  const fetch = vi.fn(async (url: string, _init?: RequestInit) => {
    if (url === "/api/v1/auth") return Response.json(state);
    if (url === "/api/v1/auth/login" || url === "/api/v1/auth/setup") return login();
    if (url === "/api/v1/auth/logout") return new Response(null, { status: 204 });
    return Response.json({ status: "ok", version: "1.0.0", commit: "x", built_at: "y" });
  });
  vi.stubGlobal("fetch", fetch);
  return fetch;
}

function renderGate() {
  stubWebSocket();
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <Gate />
    </QueryClientProvider>
  ));
}

describe("Gate", () => {
  it("creates the admin during first-run setup, then shows the app", async () => {
    const fetch = stubServer(authState({ setup: "open", setup_closes_in_s: 540 }));
    const user = userEvent.setup();
    renderGate();

    await screen.findByRole("heading", { name: "Create the admin account" });
    expect(screen.getByText("Setup stays open for 9 more minutes.")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Password"), "correct horse battery");
    await user.type(screen.getByLabelText("Confirm password"), "correct horse batter");
    await user.click(screen.getByRole("button", { name: "Create account" }));
    expect(await screen.findByText("The passwords don’t match.")).toBeInTheDocument();
    expect(fetch).not.toHaveBeenCalledWith("/api/v1/auth/setup", expect.anything());

    await user.type(screen.getByLabelText("Confirm password"), "y");
    await user.click(screen.getByRole("button", { name: "Create account" }));
    await screen.findByRole("navigation", { name: "Main" });
    const [, init] = fetch.mock.calls.find(([url]) => url === "/api/v1/auth/setup") ?? [];
    expect(new Headers(init?.headers).get("x-uniwrt-csrf")).toBe("1");
    expect(JSON.parse(String(init?.body))).toEqual({
      username: "admin",
      password: "correct horse battery",
    });
    expect(FakeWebSocket.last.url).toMatch(/\/api\/v1\/ws$/);
  });

  it("explains a wrong password and throttling, then signs in", async () => {
    const answers = [
      () => problem(401, "Wrong username or password"),
      () => problem(429, "Too many failed logins", { "retry-after": "60" }),
      () => Response.json(admin),
    ];
    stubServer(authState({}), () => answers.shift()?.() ?? Response.json(admin));
    const user = userEvent.setup();
    renderGate();

    await screen.findByRole("heading", { name: "Sign in" });
    await user.type(screen.getByLabelText("Username"), "admin");
    await user.type(screen.getByLabelText("Password"), "nope");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Wrong username or password");

    await user.click(screen.getByRole("button", { name: "Sign in" }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("Too many attempts. Try again in 60 s."),
    );

    await user.click(screen.getByRole("button", { name: "Sign in" }));
    await screen.findByRole("navigation", { name: "Main" });
  });

  it("returns to sign-in when the hub ends the session, and signs out", async () => {
    stubServer(authState({ user: admin }));
    const user = userEvent.setup();
    renderGate();

    await screen.findByRole("navigation", { name: "Main" });
    FakeWebSocket.last.open();
    FakeWebSocket.last.drop(4401);
    await screen.findByRole("heading", { name: "Sign in" });
    expect(screen.getByRole("status")).toHaveTextContent("Your session ended");

    await user.type(screen.getByLabelText("Username"), "admin");
    await user.type(screen.getByLabelText("Password"), "correct horse battery");
    await user.click(screen.getByRole("button", { name: "Sign in" }));
    await user.click(await screen.findByRole("button", { name: "Account" }));
    await user.click(await screen.findByRole("button", { name: "Sign out" }));
    await screen.findByRole("heading", { name: "Sign in" });
    expect(screen.queryByText(/session ended/)).not.toBeInTheDocument();
  });

  it("says when setup closed without an admin", async () => {
    stubServer(authState({ setup: "closed" }));
    renderGate();
    await screen.findByRole("heading", { name: "Setup is closed" });
  });
});
