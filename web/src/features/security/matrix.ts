import type { Network } from "../../api/gen/Network";
import type { Purpose } from "../../api/gen/Purpose";
import type { ZonePolicy } from "../../api/gen/ZonePolicy";

/** The WAN's zone: the matrix's last column, "Internet". */
export const WAN = "wan";

/**
 * Whether a network of `purpose` reaches another network (or with `toWan` the internet) before
 * the policies override the cell: the default and management networks reach everything, the
 * others the internet only (render's `firewall::seeded`).
 */
export function seeded(purpose: Purpose, toWan: boolean): boolean {
  return purpose === "default" || purpose === "mgmt" || toWan;
}

/** The cells' states, by `from` then `to`: allowed or not. */
export type Grid = Record<string, Record<string, boolean>>;

/** Whether traffic from `from` to `to` passes: the last policy on the cell, else the seed. */
function allowedBy(policies: readonly ZonePolicy[], from: Network, to: string): boolean {
  const policy = policies.findLast((p) => p.from === from.id && p.to === to);
  return policy ? policy.action === "allow" : seeded(from.purpose, to === WAN);
}

/** The columns of `from`'s row: every other network, then the internet. */
function targets(networks: readonly Network[], from: Network): string[] {
  return [...networks.filter((n) => n.id !== from.id).map((n) => n.id), WAN];
}

/** The matrix the policies make, one row per network. */
export function gridOf(networks: readonly Network[], policies: readonly ZonePolicy[]): Grid {
  return Object.fromEntries(
    networks.map((from) => [
      from.id,
      Object.fromEntries(targets(networks, from).map((to) => [to, allowedBy(policies, from, to)])),
    ]),
  );
}

/** The matrix every network's purpose seeds. */
export function seededGrid(networks: readonly Network[]): Grid {
  return gridOf(networks, []);
}

/** What a cell shows: allowed, or blocked with replies passing when the reverse is allowed. */
export interface Cell {
  allowed: boolean;
  /** Blocked, but the other side opens connections this way, so its replies pass. */
  repliesOnly: boolean;
  /** Differs from what the row's purpose seeds. */
  changed: boolean;
}

export function cellOf(grid: Grid, from: Network, to: string): Cell {
  const allowed = grid[from.id]?.[to] ?? false;
  return {
    allowed,
    repliesOnly: !allowed && to !== WAN && (grid[to]?.[from.id] ?? false),
    changed: allowed !== seeded(from.purpose, to === WAN),
  };
}

/** `grid` with the cell from `from` to `to` set. */
export function withCell(grid: Grid, from: string, to: string, allowed: boolean): Grid {
  return { ...grid, [from]: { ...grid[from], [to]: allowed } };
}

/**
 * The policies that make `grid`: the cells that keep their state keep their policies as they
 * were (an `established` one too), the others get one only where they differ from the seed.
 * Cells the grid lacks keep their policies.
 * Policies on cells outside the grid (from the internet) stay.
 */
export function policiesOf(
  grid: Grid,
  networks: readonly Network[],
  existing: readonly ZonePolicy[],
): ZonePolicy[] {
  const rows = new Set(networks.map((n) => n.id));
  const kept = existing.filter((p) => !rows.has(p.from));
  const cells: ZonePolicy[] = [];
  for (const from of networks) {
    for (const to of targets(networks, from)) {
      const allowed = grid[from.id]?.[to];
      const own = existing.filter((p) => p.from === from.id && p.to === to);
      // A cell the grid lacks (a network another save added) stays as it was.
      if (allowed === undefined || allowedBy(existing, from, to) === allowed) cells.push(...own);
      else if (allowed !== seeded(from.purpose, to === WAN)) {
        cells.push({ from: from.id, to, action: allowed ? "allow" : "block" });
      }
    }
  }
  return [...cells, ...kept];
}

/** Whether two grids differ in a cell. */
export function gridsDiffer(a: Grid, b: Grid): boolean {
  return Object.entries(a).some(([from, row]) =>
    Object.entries(row).some(([to, allowed]) => b[from]?.[to] !== allowed),
  );
}
