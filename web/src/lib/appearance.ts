import { createSignal } from "solid-js";

/**
 * The theme and the skin (D-046), two independent choices saved per browser. public/theme-init.js
 * applies both before the first paint, so its keys and values must match these.
 */

export type ThemePreference = "system" | "light" | "dark";
export type Skin = "graphite" | "glass" | "unifi";

export const themes: readonly ThemePreference[] = ["system", "light", "dark"];
export const skins: readonly Skin[] = ["graphite", "glass", "unifi"];

const THEME_KEY = "uniwrt.theme";
const SKIN_KEY = "uniwrt.skin";
const DEFAULT_SKIN: Skin = "graphite";

export function isThemePreference(value: string): value is ThemePreference {
  return (themes as readonly string[]).includes(value);
}

export function isSkin(value: string): value is Skin {
  return (skins as readonly string[]).includes(value);
}

function load(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Saves a choice, or forgets it when it is the default. */
function save(key: string, value: string | null): void {
  try {
    if (value === null) {
      localStorage.removeItem(key);
    } else {
      localStorage.setItem(key, value);
    }
  } catch {
    // Unavailable storage only means the choice is not remembered.
  }
}

/** Sets `data-<name>` on <html>, or removes it for the default. */
function apply(name: "theme" | "skin", value: string | null): void {
  const root = document.documentElement;
  if (value === null) {
    root.removeAttribute(`data-${name}`);
  } else {
    root.setAttribute(`data-${name}`, value);
  }
}

const storedTheme = load(THEME_KEY);
const storedSkin = load(SKIN_KEY);
const [theme, setTheme] = createSignal<ThemePreference>(
  storedTheme === "light" || storedTheme === "dark" ? storedTheme : "system",
);
const [skin, setSkin] = createSignal<Skin>(
  storedSkin !== null && isSkin(storedSkin) ? storedSkin : DEFAULT_SKIN,
);

/** The current theme preference; `system` follows the operating system's setting. */
export const themePreference = theme;

/** The current skin. */
export const currentSkin = skin;

/** Applies a theme preference to the page and remembers it in this browser. */
export function setThemePreference(next: ThemePreference): void {
  setTheme(next);
  const value = next === "system" ? null : next;
  apply("theme", value);
  save(THEME_KEY, value);
}

/** Applies a skin to the page and remembers it in this browser. */
export function setCurrentSkin(next: Skin): void {
  setSkin(next);
  const value = next === DEFAULT_SKIN ? null : next;
  apply("skin", value);
  save(SKIN_KEY, value);
}
