import type { AvailablePackage } from "../../../api/gen/AvailablePackage";
import type { InstalledPackage } from "../../../api/gen/InstalledPackage";
import type { Job } from "../../../api/gen/Job";
import type { PackageCatalog } from "../../../api/gen/PackageCatalog";
import type { PackageChanges } from "../../../api/gen/PackageChanges";
import { formatBytes } from "../../../lib/format";
import { strings } from "../../../lib/strings";

const t = strings.device.software;

/** Which installed packages the list shows. */
export type Shown = "explicit" | "dependency" | "all";

/** One of the three kinds of change a job makes. */
export type ChangeKind = keyof PackageChanges;

export const NO_CHANGES: PackageChanges = { install: [], remove: [], upgrade: [] };

export function changeCount(changes: PackageChanges): number {
  return changes.install.length + changes.remove.length + changes.upgrade.length;
}

/**
 * `changes` with `name` added to (`on`) or taken from `kind`'s list, each list sorted. A package
 * is either removed or upgraded, so choosing one drops the other.
 */
export function toggled(
  changes: PackageChanges,
  kind: ChangeKind,
  name: string,
  on: boolean,
): PackageChanges {
  const next = { ...changes, [kind]: changes[kind].filter((other) => other !== name) };
  if (on) {
    next[kind] = [...next[kind], name].sort();
    const other = kind === "remove" ? "upgrade" : kind === "upgrade" ? "remove" : undefined;
    if (other) next[other] = next[other].filter((entry) => entry !== name);
  }
  return next;
}

/**
 * `changes` against the packages read again: an install of what is now installed, and a removal or
 * an upgrade of what no longer is, are dropped.
 */
export function pruned(
  changes: PackageChanges,
  installed: readonly InstalledPackage[],
): PackageChanges {
  const names = new Set(installed.map((pkg) => pkg.name));
  return {
    install: changes.install.filter((name) => !names.has(name)),
    remove: changes.remove.filter((name) => names.has(name)),
    upgrade: changes.upgrade.filter((name) => names.has(name)),
  };
}

/** Whether `text` holds every word of `query`, ignoring case. */
function matches(text: string, words: readonly string[]): boolean {
  const lower = text.toLowerCase();
  return words.every((word) => lower.includes(word));
}

function words(query: string): string[] {
  return query.toLowerCase().split(/\s+/).filter(Boolean);
}

/** The installed packages `shown` picks whose name matches `query`, by name. */
export function shownPackages(
  packages: readonly InstalledPackage[],
  shown: Shown,
  query: string,
): InstalledPackage[] {
  const wanted = words(query);
  return packages.filter(
    (pkg) =>
      (shown === "all" || pkg.explicit === (shown === "explicit")) && matches(pkg.name, wanted),
  );
}

/** Why `pkg` can't be removed, if it can't: its lock, or the site's requirements unknown. */
export function removeBlock(
  pkg: InstalledPackage,
  requiredUnknown: string | null,
): string | undefined {
  if (pkg.lock) return t.lockWhy[pkg.lock];
  return requiredUnknown === null ? undefined : t.requiredUnknownShort;
}

/** What the feeds offer that isn't installed, by name. */
export function offered(
  catalog: PackageCatalog,
  installed: readonly InstalledPackage[],
): AvailablePackage[] {
  const names = new Set(installed.map((pkg) => pkg.name));
  return catalog.available.filter((pkg) => !names.has(pkg.name));
}

/**
 * The packages that match every word of `query` in their name or description: names that start
 * with the query first, then other name matches, then description matches. All when it is empty.
 */
export function searched(packages: readonly AvailablePackage[], query: string): AvailablePackage[] {
  const wanted = words(query);
  if (wanted.length === 0) return [...packages];
  const whole = wanted.join(" ");
  const starts: AvailablePackage[] = [];
  const named: AvailablePackage[] = [];
  const described: AvailablePackage[] = [];
  for (const pkg of packages) {
    if (pkg.name.startsWith(whole)) starts.push(pkg);
    else if (matches(pkg.name, wanted)) named.push(pkg);
    else if (matches(`${pkg.name} ${pkg.description}`, wanted)) described.push(pkg);
  }
  return [...starts, ...named, ...described];
}

/** Why an upgrade on offer can't be chosen, if it can't: its package comes with the firmware. */
export function upgradeBlock(
  name: string,
  installed: readonly InstalledPackage[],
): string | undefined {
  return installed.find((pkg) => pkg.name === name)?.with_firmware ? t.firmwareWhy : undefined;
}

/** What the chosen installs take on flash, from the sizes the index gives; `null` if it gives none. */
export function installBytes(changes: PackageChanges, catalog: PackageCatalog | undefined) {
  let total: number | null = null;
  for (const name of changes.install) {
    const size = catalog?.available.find((pkg) => pkg.name === name)?.installed_bytes;
    if (size != null) total = (total ?? 0) + size;
  }
  return total;
}

export interface ChangeRow {
  name: string;
  /** Its version, size or versions, as far as they are known. */
  detail: string;
}

/** The changes as the confirmation lists them: kind by kind (removals first, as the job runs). */
export function changeRows(
  changes: PackageChanges,
  catalog: PackageCatalog | undefined,
  installed: readonly InstalledPackage[],
): { kind: ChangeKind; rows: ChangeRow[] }[] {
  const version = (name: string) => installed.find((pkg) => pkg.name === name)?.version ?? "";
  const rows: Record<ChangeKind, (name: string) => string> = {
    remove: version,
    upgrade: (name) => {
      const offer = catalog?.upgrades.find((up) => up.name === name);
      return offer ? t.confirm.upgradeFrom(offer.from, offer.to) : version(name);
    },
    install: (name) => {
      const pkg = catalog?.available.find((entry) => entry.name === name);
      if (!pkg) return "";
      return pkg.installed_bytes === null
        ? pkg.version
        : `${pkg.version} · ${formatBytes(pkg.installed_bytes)}`;
    },
  };
  return (["remove", "upgrade", "install"] as const)
    .filter((kind) => changes[kind].length > 0)
    .map((kind) => ({
      kind,
      rows: changes[kind].map((name) => ({ name, detail: rows[kind](name) })),
    }));
}

/** A packages job on `node`. */
export function concerns(job: Job, node: number): boolean {
  return job.kind === "packages" && job.node === node;
}

/**
 * How many of the packages jobs on `node` ended after being seen running (`seen` remembers them):
 * the list is read again as it grows, but not for the ended jobs the topic's snapshot brings.
 */
export function endedCount(jobs: readonly Job[], node: number, seen: Set<number>): number {
  let ended = 0;
  for (const job of jobs) {
    if (!concerns(job, node)) continue;
    if (job.state === "running") seen.add(job.id);
    else if (seen.has(job.id)) ended += 1;
  }
  return ended;
}
