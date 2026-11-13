/** The floor list as the page keeps it, and where each device is (D-117). */
import type { Floor } from "../../api/gen/Floor";
import type { NodeSummary } from "../../api/gen/NodeSummary";

/** The list with `saved` in its place (or added), its devices taken off every other floor, by level. */
export function withFloor(floors: readonly Floor[], saved: Floor): Floor[] {
  const placed = new Set(saved.nodes.map((p) => p.node));
  const others = floors
    .filter((floor) => floor.id !== saved.id)
    .map((floor) =>
      floor.nodes.some((p) => placed.has(p.node))
        ? { ...floor, nodes: floor.nodes.filter((p) => !placed.has(p.node)) }
        : floor,
    );
  return [...others, saved].sort((a, b) => a.level - b.level);
}

/** The list without `node` on floor `id`. */
export function withoutNode(floors: readonly Floor[], id: number, node: number): Floor[] {
  return floors.map((floor) =>
    floor.id === id ? { ...floor, nodes: floor.nodes.filter((p) => p.node !== node) } : floor,
  );
}

export interface DeviceGroups {
  here: NodeSummary[];
  elsewhere: { node: NodeSummary; floor: Floor }[];
  unplaced: NodeSummary[];
}

/** The fleet's devices by where they are relative to `current`, each group in the fleet's order. */
export function groupDevices(
  nodes: readonly NodeSummary[],
  floors: readonly Floor[],
  current: Floor,
): DeviceGroups {
  const floorOf = new Map<number, Floor>();
  for (const floor of floors) for (const p of floor.nodes) floorOf.set(p.node, floor);
  const groups: DeviceGroups = { here: [], elsewhere: [], unplaced: [] };
  for (const node of nodes) {
    const floor = floorOf.get(node.id);
    if (!floor) groups.unplaced.push(node);
    else if (floor.id === current.id) groups.here.push(node);
    else groups.elsewhere.push({ node, floor });
  }
  return groups;
}

/** The level a new floor gets: one above the highest, or the ground floor. */
export function nextLevel(floors: readonly Floor[]): number {
  return floors.length === 0 ? 0 : Math.min(99, Math.max(...floors.map((f) => f.level)) + 1);
}
