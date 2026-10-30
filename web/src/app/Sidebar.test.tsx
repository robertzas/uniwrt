import { memoryHistory } from "@solidjs/router";
import { render, screen, within } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { openTestSocket, SignedInProviders } from "../test/providers";
import { navItems } from "./nav";
import { createAppRouter } from "./routes";
import { Sidebar } from "./Sidebar";

describe("Sidebar", () => {
  it("links every section and marks the current page", async () => {
    const Router = createAppRouter(memoryHistory("/insights/logs"));
    const { socket } = openTestSocket();
    render(() => (
      <SignedInProviders socket={socket}>
        <Router>{() => <Sidebar />}</Router>
      </SignedInProviders>
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

  it("keeps every link named as an icon rail", async () => {
    const Router = createAppRouter(memoryHistory("/devices"));
    const { socket } = openTestSocket();
    render(() => (
      <SignedInProviders socket={socket}>
        <Router>{() => <Sidebar rail />}</Router>
      </SignedInProviders>
    ));
    const nav = await screen.findByRole("navigation", { name: "Main" });
    const links = within(nav).getAllByRole("link");
    expect(links.map((link) => link.textContent)).toEqual(navItems.map((item) => item.label));
    expect(within(nav).getByRole("link", { name: "Devices" })).toHaveAttribute("data-active");
    expect(screen.getByRole("button", { name: "Appearance" })).toBeInTheDocument();
  });
});
