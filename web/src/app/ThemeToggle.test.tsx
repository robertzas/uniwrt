import { render, screen } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { setThemePreference } from "../lib/appearance";
import { ThemeToggle } from "./ThemeToggle";

describe("ThemeToggle", () => {
  afterEach(() => setThemePreference("system"));

  it("applies and remembers the chosen theme", async () => {
    const user = userEvent.setup();
    render(() => <ThemeToggle />);
    const root = document.documentElement;
    expect(screen.getByRole("radio", { name: "System" })).toBeChecked();

    await user.click(screen.getByText("Light"));
    expect(root.dataset.theme).toBe("light");
    expect(localStorage.getItem("uniwrt.theme")).toBe("light");
    expect(screen.getByRole("radio", { name: "Light" })).toBeChecked();

    await user.click(screen.getByText("System"));
    expect(root).not.toHaveAttribute("data-theme");
    expect(localStorage.getItem("uniwrt.theme")).toBeNull();
  });
});
