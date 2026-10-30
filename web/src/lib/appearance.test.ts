import { flush } from "solid-js";
import { afterEach, describe, expect, it } from "vitest";
import { currentSkin, setCurrentSkin, setThemePreference, themePreference } from "./appearance";

describe("appearance", () => {
  afterEach(() => {
    setCurrentSkin("graphite");
    setThemePreference("system");
  });
  const root = document.documentElement;

  it("applies and remembers a skin, and forgets the default", () => {
    setCurrentSkin("glass");
    flush();
    expect(currentSkin()).toBe("glass");
    expect(root.dataset.skin).toBe("glass");
    expect(localStorage.getItem("uniwrt.skin")).toBe("glass");

    setCurrentSkin("graphite");
    expect(root).not.toHaveAttribute("data-skin");
    expect(localStorage.getItem("uniwrt.skin")).toBeNull();
  });

  it("keeps skin and theme independent", () => {
    setCurrentSkin("unifi");
    setThemePreference("dark");
    flush();
    expect(root.dataset.skin).toBe("unifi");
    expect(root.dataset.theme).toBe("dark");
    expect(themePreference()).toBe("dark");
  });
});
