import { memoryHistory } from "@solidjs/router";
import { render, screen, waitFor, within } from "@solidjs/testing-library";
import { QueryClient, QueryClientProvider } from "@tanstack/solid-query";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { createAppRouter } from "../../app/routes";
import { Shell } from "../../app/Shell";
import { controllerUpdate } from "../../test/controller";
import { openTestSocket, SignedInProviders } from "../../test/providers";

function renderAt(path: string) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) =>
      url === "/api/v1/firmware"
        ? Response.json({ nodes: [], error: null })
        : url === "/api/v1/controller/update"
          ? Response.json(controllerUpdate())
          : Response.json({ status: "ok", version: "1.0.0", commit: "x", built_at: "y" }),
    ),
  );
  const history = memoryHistory(path);
  const Router = createAppRouter(history);
  const { socket } = openTestSocket();
  render(() => (
    <QueryClientProvider client={new QueryClient()}>
      <SignedInProviders socket={socket}>
        <Router>{(props) => <Shell>{props.children}</Shell>}</Router>
      </SignedInProviders>
    </QueryClientProvider>
  ));
  return history;
}

describe("in-app help", () => {
  it("opens beside the page at its section, follows links, goes back and closes", async () => {
    const user = userEvent.setup();
    renderAt("/updates");
    const button = await screen.findByRole("button", { name: "Help" }, { timeout: 2000 });
    expect(button).toHaveAttribute("aria-expanded", "false");

    await user.click(button);
    const panel = await screen.findByRole("complementary", { name: "Help" });
    expect(button).toHaveAttribute("aria-expanded", "true");
    expect(await within(panel).findByRole("heading", { name: "Controller" })).toBeInTheDocument();
    expect(within(panel).getByText("Updates", { selector: "p" })).toBeInTheDocument();
    expect(within(panel).queryByRole("button", { name: "Back" })).not.toBeInTheDocument();

    // A link to another chapter stays in the panel; Back returns.
    const link = within(panel)
      .getAllByRole("link")
      .find((a) => /^\/help\/(?!updates)/.test(a.getAttribute("href") ?? ""));
    if (!link) throw new Error("the Updates chapter links to another chapter");
    await user.click(link);
    await within(panel).findByRole("button", { name: "Back" });
    expect(within(panel).queryByText("Updates", { selector: "p" })).toBeNull();
    await user.click(within(panel).getByRole("button", { name: "Back" }));
    expect(await within(panel).findByRole("heading", { name: "Controller" })).toBeInTheDocument();

    // Escape closes it and gives focus back to the button.
    panel.focus();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("complementary", { name: "Help" })).toBeNull());
    expect(button).toHaveFocus();
  });

  it("toggles with ?, but not while typing", async () => {
    const user = userEvent.setup();
    renderAt("/updates");
    await screen.findByRole("button", { name: "Help" }, { timeout: 2000 });
    await user.keyboard("?");
    expect(await screen.findByRole("complementary", { name: "Help" })).toBeInTheDocument();
    await user.keyboard("?");
    await waitFor(() => expect(screen.queryByRole("complementary", { name: "Help" })).toBeNull());

    await user.click(screen.getByRole("button", { name: "Search or run a command" }));
    await waitFor(() => expect(screen.getByRole("combobox")).toHaveFocus());
    await user.keyboard("?");
    expect(screen.getByRole("combobox")).toHaveValue("?");
    expect(screen.queryByRole("complementary", { name: "Help" })).toBeNull();
  });

  it("finds a guide section in the palette and opens it in the panel", async () => {
    const user = userEvent.setup();
    renderAt("/updates");
    await screen.findByRole("button", { name: "Help" }, { timeout: 2000 });
    await user.keyboard("{Control>}k{/Control}");
    await user.keyboard("adopt several devices");
    await user.click(
      await screen.findByRole("option", { name: /To adopt several devices at once/ }),
    );
    const panel = await screen.findByRole("complementary", { name: "Help" });
    expect(await within(panel).findByText("Devices and adoption", { selector: "p" })).toBeVisible();
  });

  it("opens the help center from the panel, which hides the panel and its button", async () => {
    const user = userEvent.setup();
    const history = renderAt("/updates");
    await user.click(await screen.findByRole("button", { name: "Help" }, { timeout: 2000 }));
    const panel = await screen.findByRole("complementary", { name: "Help" });
    await within(panel).findByRole("heading", { name: "Controller" });
    await user.click(within(panel).getByRole("link", { name: "Open in the help center" }));

    await waitFor(() => expect(history.get()).toBe("/help/updates#controller"));
    expect(await screen.findByRole("heading", { level: 1, name: "Updates" })).toBeInTheDocument();
    expect(screen.queryByRole("complementary", { name: "Help" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Help" })).toBeNull();
    const chapters = screen.getByRole("navigation", { name: "Chapters" });
    expect(within(chapters).getByRole("link", { name: "Updates" })).toHaveAttribute(
      "aria-current",
      "page",
    );

    await user.click(within(chapters).getByRole("link", { name: "Wi-Fi and mesh" }));
    await waitFor(() => expect(history.get()).toBe("/help/wifi"));
    expect(await screen.findByRole("heading", { level: 1, name: "Wi-Fi and mesh" })).toBeVisible();
  });

  it("says so for a chapter the guide doesn't have", async () => {
    renderAt("/help/nope");
    expect(await screen.findByRole("alert")).toHaveTextContent("No such chapter");
  });

  it("shows the contents at /help", async () => {
    renderAt("/help");
    expect(
      await screen.findByRole("heading", { level: 1, name: "uniwrt user guide" }),
    ).toBeInTheDocument();
    expect(document.querySelector("header")).toHaveTextContent("Help center");
  });
});
