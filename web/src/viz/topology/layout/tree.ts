import type { Scene, SceneVertex, VertexKind } from "../scene";

export interface Point {
  x: number;
  y: number;
}

/** World units between the levels of the tree, and between neighbours on one level. */
const LEVEL = 120;
const GAP = 18;
/** The least room a leaf takes on its level, so that neighbours' labels don't collide. */
const LEAF_WIDTH: Record<VertexKind, number> = {
  internet: 120,
  gateway: 120,
  switch: 120,
  ap: 120,
  bridge: 120,
  peer: 130,
  cluster: 84,
  client: 24,
};

const order: Record<VertexKind, number> = {
  internet: 0,
  gateway: 1,
  switch: 2,
  ap: 3,
  bridge: 4,
  peer: 5,
  cluster: 6,
  client: 7,
};

function compare(a: SceneVertex, b: SceneVertex): number {
  return (
    order[a.kind] - order[b.kind] || a.label.localeCompare(b.label) || a.id.localeCompare(b.id)
  );
}

/**
 * A tidy tree over the scene's links (upstream to downstream): roots at the top, each parent
 * centred over its children, leaves side by side. Vertices that no link reaches are roots of
 * their own, to the right. Centred on x = 0, the roots at y = 0.
 */
export function treeLayout(scene: Scene): Map<string, Point> {
  const children = new Map<string, SceneVertex[]>();
  const hasParent = new Set<string>();
  for (const link of scene.links) {
    const child = scene.byId.get(link.target);
    if (!child || !scene.byId.has(link.source) || hasParent.has(child.id)) continue;
    hasParent.add(child.id);
    const list = children.get(link.source);
    if (list) list.push(child);
    else children.set(link.source, [child]);
  }
  for (const list of children.values()) list.sort(compare);
  const roots = scene.vertices.filter((vertex) => !hasParent.has(vertex.id)).sort(compare);

  const positions = new Map<string, Point>();
  let cursor = 0;
  const place = (vertex: SceneVertex, depth: number): number => {
    positions.set(vertex.id, { x: 0, y: depth * LEVEL });
    const kids = (children.get(vertex.id) ?? []).filter((child) => !positions.has(child.id));
    let x: number;
    if (kids.length === 0) {
      const width = Math.max(2 * vertex.radius + GAP, LEAF_WIDTH[vertex.kind]);
      x = cursor + width / 2;
      cursor += width;
    } else {
      const xs = kids.map((child) => place(child, depth + 1));
      x = ((xs[0] ?? 0) + (xs[xs.length - 1] ?? 0)) / 2;
    }
    positions.set(vertex.id, { x, y: depth * LEVEL });
    return x;
  };
  for (const root of roots) {
    if (!positions.has(root.id)) place(root, 0);
    cursor += GAP * 2;
  }
  // A cycle leaves vertices unplaced: they start trees of their own.
  for (const vertex of scene.vertices) {
    if (!positions.has(vertex.id)) place(vertex, 0);
  }
  const shift = (cursor - GAP * 2) / 2;
  for (const point of positions.values()) point.x -= shift;
  return positions;
}
