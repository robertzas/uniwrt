import { createMemoryHistory, MemoryRouter, Route } from "@solidjs/router";
import { render, screen, within } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { navItems } from "./nav";
import { Sidebar } from "./Sidebar";

describe("Sidebar", () => {
  it("links every section and marks the current page", async () => {
    const history = createMemoryHistory();
    history.set({ value: "/insights/logs", replace: true, scroll: false });
    render(() => (
      <MemoryRouter history={history}>
        <Route path="*" component={Sidebar} />
      </MemoryRouter>
    ));
    const nav = await screen.findByRole("navigation", { name: "Main" });

    const hrefs = within(nav)
      .getAllByRole("link")
      .map((link) => link.getAttribute("href"));
    expect(hrefs).toEqual(navItems.map((item) => item.path));
    expect(within(nav).getByRole("link", { name: "Logs" })).toHaveAttribute("aria-current", "page");
    expect(within(nav).getByRole("link", { name: "Dashboard" })).not.toHaveAttribute(
      "aria-current",
    );
    expect(within(nav).getByRole("list", { name: "Insights" })).toBeInTheDocument();
  });
});
