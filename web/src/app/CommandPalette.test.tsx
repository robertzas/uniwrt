import { render, screen, waitFor } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { createSignal } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import { currentSkin, setCurrentSkin } from "../lib/appearance";
import { CommandPalette } from "./CommandPalette";
import { appCommands } from "./commands";

function renderPalette() {
  const navigate = vi.fn();
  const [open, setOpen] = createSignal(true);
  render(() => (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        reopen
      </button>
      <CommandPalette open={open()} onOpenChange={setOpen} commands={appCommands(navigate)} />
    </>
  ));
  return { navigate, open };
}

describe("CommandPalette", () => {
  afterEach(() => setCurrentSkin("graphite"));

  it("filters as you type and runs the active command with Enter", async () => {
    const user = userEvent.setup();
    const { navigate, open } = renderPalette();
    const input = await screen.findByRole("combobox", { name: "Command palette" });
    // Kobalte moves focus after the next paint.
    await waitFor(() => expect(input).toHaveFocus());

    await user.type(input, "logs");
    const options = screen.getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual(["Logs"]);
    expect(input).toHaveAttribute("aria-activedescendant", options[0]?.id);

    await user.keyboard("{Enter}");
    expect(navigate).toHaveBeenCalledWith("/insights/logs");
    expect(open()).toBe(false);
  });

  it("moves with the arrow keys, wrapping around", async () => {
    const user = userEvent.setup();
    renderPalette();
    const input = await screen.findByRole("combobox");
    await user.type(input, "theme");
    const selected = () =>
      screen.getAllByRole("option").findIndex((o) => o.getAttribute("aria-selected") === "true");
    expect(selected()).toBe(0);
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(selected()).toBe(2);
    await user.keyboard("{ArrowDown}");
    expect(selected()).toBe(0);
    await user.keyboard("{ArrowUp}");
    expect(selected()).toBe(2);
  });

  it("switches the skin from a command", async () => {
    const user = userEvent.setup();
    renderPalette();
    await user.type(await screen.findByRole("combobox"), "midnight");
    await user.click(screen.getByRole("option", { name: "Use the Midnight glass skin" }));
    expect(currentSkin()).toBe("glass");
  });

  it("says when nothing matches", async () => {
    const user = userEvent.setup();
    renderPalette();
    await user.type(await screen.findByRole("combobox"), "zzz");
    expect(screen.queryAllByRole("option")).toEqual([]);
    expect(screen.getByText("Nothing matches “zzz”.")).toBeInTheDocument();
  });

  // Escape relies on the Kobalte patch (D-045): without it the dialog stays open.
  it("closes with Escape and starts fresh when reopened", async () => {
    const user = userEvent.setup();
    const { open } = renderPalette();
    await user.type(await screen.findByRole("combobox"), "wifi");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(open()).toBe(false);

    await user.click(screen.getByRole("button", { name: "reopen" }));
    expect(await screen.findByRole("combobox")).toHaveValue("");
  });
});
