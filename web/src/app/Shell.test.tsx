import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { createAppRouter } from "./routes";
import { Shell } from "./Shell";

describe("Shell", () => {
  it("opens the navigation drawer and closes it after navigating", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        Response.json({ status: "ok", version: "1.0.0", commit: "x", built_at: "y" }),
      ),
    );
    const user = userEvent.setup();
    const history = memoryHistory("/");
    const Router = createAppRouter(history);
    render(() => (
      <QueryClientProvider client={new QueryClient()}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </QueryClientProvider>
    ));

    await user.click(screen.getByRole("button", { name: "Open navigation" }));
    const drawer = await screen.findByRole("dialog");
    await user.click(within(drawer).getByRole("link", { name: "Clients" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(history.get()).toBe("/clients");
    expect(document.querySelector("header")).toHaveTextContent("Clients");
  });

  it("opens the command palette with the shortcut and navigates from it", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => Response.json({})),
    );
    const user = userEvent.setup();
    const history = memoryHistory("/");
    const Router = createAppRouter(history);
    render(() => (
      <QueryClientProvider client={new QueryClient()}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </QueryClientProvider>
    ));

    await user.keyboard("{Control>}k{/Control}");
    const input = await screen.findByRole("combobox", { name: "Command palette" });
    await user.type(input, "updates{Enter}");
    await waitFor(() => expect(history.get()).toBe("/updates"));
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
  });
});
