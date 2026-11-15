import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { openTestSocket, SignedInProviders } from "../test/providers";
import { createAppRouter } from "./routes";
import { Shell } from "./Shell";

describe("Shell", () => {
  it("opens the navigation drawer and closes it after navigating", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) =>
        url === "/api/v1/firmware"
          ? Response.json({ nodes: [], error: null })
          : Response.json({ status: "ok", version: "1.0.0", commit: "x", built_at: "y" }),
      ),
    );
    const user = userEvent.setup();
    const history = memoryHistory("/updates");
    const Router = createAppRouter(history);
    const { socket } = openTestSocket();
    render(() => (
      <QueryClientProvider client={new QueryClient()}>
        <SignedInProviders socket={socket}>
          <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
        </SignedInProviders>
      </QueryClientProvider>
    ));

    // The Updates page loads lazily: the shell shows once it has.
    await user.click(
      await screen.findByRole("button", { name: "Open navigation" }, { timeout: 2000 }),
    );
    const drawer = await screen.findByRole("dialog");
    await user.click(within(drawer).getByRole("link", { name: "Clients" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(history.get()).toBe("/clients");
    expect(document.querySelector("header")).toHaveTextContent("Clients");
  });

  it("opens the command palette with the shortcut and navigates from it", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) =>
        Response.json(url === "/api/v1/firmware" ? { nodes: [], error: null } : {}),
      ),
    );
    const user = userEvent.setup();
    const history = memoryHistory("/updates");
    const Router = createAppRouter(history);
    const { socket } = openTestSocket();
    render(() => (
      <QueryClientProvider client={new QueryClient()}>
        <SignedInProviders socket={socket}>
          <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
        </SignedInProviders>
      </QueryClientProvider>
    ));

    await screen.findByRole("button", { name: "Open navigation" }, { timeout: 2000 });
    await user.keyboard("{Control>}k{/Control}");
    const input = await screen.findByRole("combobox", { name: "Command palette" });
    await user.type(input, "clients{Enter}");
    await waitFor(() => expect(history.get()).toBe("/clients"));
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  });
});
