import type { FeedSet } from "../../api/gen/FeedSet";
import type { SetStatus } from "../../api/gen/SetStatus";

/** How often the card reads the mirror again while a sync runs. */
export const MIRROR_POLL_MS = 2000;

/**
 * A feed's short name from its URL: the feed under `packages/<arch>/` (`base`, `luci`, …),
 * `core` for the target's own packages, `kmods` for its kernel modules, else the last segment.
 */
export function feedName(url: string): string {
  const path = url
    .replace(/^[a-z]+:\/\/[^/]+\//, "")
    .replace(/\/packages\.adb$/, "")
    .replace(/\/+$/, "");
  const parts = path.split("/");
  const at = parts.lastIndexOf("targets");
  if (at >= 0 && parts[at + 3] === "packages") return "core";
  if (at >= 0 && parts[at + 3] === "kmods") return "kmods";
  return parts.at(-1) || url;
}

/** The URL without its scheme, as a feed's subtitle. */
export function feedPath(url: string): string {
  return url.replace(/^[a-z]+:\/\//, "");
}

/** `25.12.6 · mediatek/filogic · aarch64_cortex-a53 · apk`. */
export function setLabel(set: FeedSet): string {
  return [set.release, set.target, set.arch, set.manager].join(" · ");
}

/** A set's key among the view's sets. */
export function setKey(set: FeedSet): string {
  return `${set.release}|${set.target}|${set.arch}|${set.manager}`;
}

/** How many of the set's feeds failed their last refresh. */
export function failingFeeds(status: SetStatus): number {
  return status.feeds.filter((feed) => feed.error !== null).length;
}

/** The sets in use first, then those on offer, each by release (newest first), then label. */
export function orderedSets(sets: readonly SetStatus[]): SetStatus[] {
  return [...sets].sort(
    (a, b) =>
      Number(a.offered) - Number(b.offered) ||
      b.set.release.localeCompare(a.set.release, undefined, { numeric: true }) ||
      setLabel(a.set).localeCompare(setLabel(b.set)),
  );
}
