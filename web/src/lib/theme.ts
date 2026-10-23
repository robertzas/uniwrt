import { createSignal } from "solid-js";

export type ThemePreference = "system" | "light" | "dark";

/** Also read by public/theme-init.js, which applies the theme before the first paint. */
const STORAGE_KEY = "uniwrt.theme";

export function isThemePreference(value: string): value is ThemePreference {
  return value === "system" || value === "light" || value === "dark";
}

function stored(): ThemePreference {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    return value === "light" || value === "dark" ? value : "system";
  } catch {
    return "system";
  }
}

const [preference, setPreference] = createSignal<ThemePreference>(stored());

/** The current preference; `system` follows the operating system's setting. */
export const themePreference = preference;

/** Applies a preference to the page and remembers it in this browser. */
export function setThemePreference(next: ThemePreference): void {
  setPreference(next);
  const root = document.documentElement;
  if (next === "system") {
    root.removeAttribute("data-theme");
  } else {
    root.dataset.theme = next;
  }
  try {
    if (next === "system") {
      localStorage.removeItem(STORAGE_KEY);
    } else {
      localStorage.setItem(STORAGE_KEY, next);
    }
  } catch {
    // Unavailable storage only means the choice is not remembered.
  }
}
