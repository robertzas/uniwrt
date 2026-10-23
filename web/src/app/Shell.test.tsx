import { createMemoryHistory, MemoryRouter, Route } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
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
    const history = createMemoryHistory();
    history.set({ value: "/", replace: true, scroll: false });
    render(() => (
      <QueryClientProvider client={new QueryClient()}>
        <MemoryRouter history={history} root={Shell}>
          <Route path="*" component={() => <p>page</p>} />
        </MemoryRouter>
      </QueryClientProvider>
    ));

    await user.click(screen.getByRole("button", { name: "Open navigation" }));
    const drawer = await screen.findByRole("dialog");
    await user.click(within(drawer).getByRole("link", { name: "Clients" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(history.get()).toBe("/clients");
    expect(document.querySelector("header")).toHaveTextContent("Clients");
  });
});
