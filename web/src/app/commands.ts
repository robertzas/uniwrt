import { Laptop, Moon, Palette, Sun } from "lucide";
import {
  type Skin,
  setCurrentSkin,
  setThemePreference,
  skins,
  type ThemePreference,
} from "../lib/appearance";
import { strings } from "../lib/strings";
import type { IconNode } from "../ui/Icon";
import { navItems } from "./nav";

/** An entry in the ⌘K palette. Features add theirs here as they land. */
export interface Command {
  id: string;
  group: string;
  label: string;
  icon: IconNode;
  /** Extra words that should find it, e.g. the page's description. */
  keywords?: string;
  run: () => void;
}

const themeIcons: Record<ThemePreference, IconNode> = { system: Laptop, light: Sun, dark: Moon };

/** The app's commands: every page, then the skins and themes. */
export function appCommands(navigate: (path: string) => void): Command[] {
  const p = strings.palette;
  return [
    ...navItems.map((item) => ({
      id: `go:${item.path}`,
      group: p.groups.navigate,
      label: item.label,
      icon: item.icon,
      keywords: item.description,
      run: () => navigate(item.path),
    })),
    ...skins.map((skin: Skin) => ({
      id: `skin:${skin}`,
      group: p.groups.appearance,
      label: p.skin(strings.skin[skin].label),
      icon: Palette,
      keywords: `${strings.skin.label} ${strings.skin[skin].description}`,
      run: () => setCurrentSkin(skin),
    })),
    ...(["system", "light", "dark"] as const).map((theme) => ({
      id: `theme:${theme}`,
      group: p.groups.appearance,
      label: p.theme(strings.theme[theme]),
      icon: themeIcons[theme],
      keywords: `${strings.theme.label} mode`,
      run: () => setThemePreference(theme),
    })),
  ];
}

/**
 * The commands that match every word of the query, in any of label, group or keywords. Labels that
 * start with the query come first; groups stay together, led by the group of the best match.
 */
export function filterCommands(commands: readonly Command[], query: string): Command[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...commands];
  const q = query.trim().toLowerCase();
  const ranked = commands
    .filter((c) => {
      const text = `${c.label} ${c.group} ${c.keywords ?? ""}`.toLowerCase();
      return words.every((word) => text.includes(word));
    })
    .map((c, i) => ({ c, i, rank: c.label.toLowerCase().startsWith(q) ? 0 : 1 }))
    .sort((a, b) => a.rank - b.rank || a.i - b.i)
    .map(({ c }) => c);
  const groups = new Map<string, Command[]>();
  for (const c of ranked) {
    groups.set(c.group, [...(groups.get(c.group) ?? []), c]);
  }
  return [...groups.values()].flat();
}
