import type { FirmwareProfile } from "../../../api/gen/FirmwareProfile";

/** The models a search shows at most. */
export const SHOWN = 8;

/**
 * The models matching `query`: every word must appear in a title or the profile's ID. Models
 * with a title starting with the query come first, then those with a word starting with its
 * first word, each in the catalog's order. `more` counts the matches not shown.
 */
export function matchProfiles(
  profiles: readonly FirmwareProfile[],
  query: string,
  limit = SHOWN,
): { shown: FirmwareProfile[]; more: number } {
  const q = query.trim().toLowerCase();
  const words = q.split(/\s+/).filter(Boolean);
  if (words.length === 0) return { shown: [], more: 0 };
  const first = words[0] ?? "";
  const ranked: [number, FirmwareProfile][] = [];
  for (const profile of profiles) {
    const titles = profile.titles.map((title) => title.toLowerCase());
    const haystack = `${titles.join(" ")} ${profile.id.toLowerCase()}`;
    if (!words.every((word) => haystack.includes(word))) continue;
    const rank = titles.some((title) => title.startsWith(q))
      ? 0
      : haystack.split(/[\s_-]+/).some((word) => word.startsWith(first))
        ? 1
        : 2;
    ranked.push([rank, profile]);
  }
  ranked.sort((a, b) => a[0] - b[0]);
  return {
    shown: ranked.slice(0, limit).map(([, profile]) => profile),
    more: Math.max(0, ranked.length - limit),
  };
}

/** The packages an image adds, and those it replaces (ASU's `-name`). */
export function packageChanges(packages: readonly string[]): {
  added: string[];
  removed: string[];
} {
  return {
    added: packages.filter((name) => !name.startsWith("-")),
    removed: packages.filter((name) => name.startsWith("-")).map((name) => name.slice(1)),
  };
}
