import type { Point } from "../layout/tree";
import type { Scene, SceneLink, SceneVertex, VertexKind } from "../scene";
import { type Camera, flowOf, linkWidth, particleAt, toScreen } from "./geometry";
import { loadColor, type Palette, qualityStep, stateColor } from "./palette";

/** Everything one frame shows. */
export interface Frame {
  width: number;
  height: number;
  camera: Camera;
  scene: Scene;
  positions: ReadonlyMap<string, Point>;
  palette: Palette;
  /** Each kind's icon, as a 24 × 24 path. */
  icons: ReadonlyMap<VertexKind, Path2D>;
  selected: string | null;
  hovered: string | null;
  pinned: ReadonlySet<string>;
  /** The animation clock, for flow particles. */
  seconds: number;
  /** Draw flow particles (not under reduced motion, nor in a replay, which has no traffic). */
  flows: boolean;
}

const FONT = '"Inter Variable", ui-sans-serif, system-ui, sans-serif';
const DIM = 0.22;
const MAX_LABEL = 26;

function truncate(text: string): string {
  return text.length > MAX_LABEL ? `${text.slice(0, MAX_LABEL - 1)}…` : text;
}

/** What stays bright while something is selected: it, its links and their other ends. */
function focusOf(scene: Scene, selected: string | null): Set<string> | undefined {
  if (!selected) return undefined;
  const focus = new Set([selected]);
  for (const link of scene.links) {
    if (link.id === selected || link.source === selected || link.target === selected) {
      focus.add(link.id).add(link.source).add(link.target);
    }
  }
  return focus;
}

function linkColor(palette: Palette, link: SceneLink): string {
  const edge = link.edge;
  if (!edge) return palette.edgeStrong;
  if (!edge.up) return palette.crit;
  if (edge.quality !== null) return palette.link[qualityStep(edge.quality)];
  return palette.edgeStrong;
}

/** On screen, a vertex grows and shrinks with the zoom, but never below a dot. */
function screenRadius(vertex: SceneVertex, camera: Camera): number {
  return Math.max(vertex.kind === "client" ? 2.5 : 5, vertex.radius * camera.k);
}

export function drawFrame(ctx: CanvasRenderingContext2D, frame: Frame) {
  const { scene, palette, camera } = frame;
  ctx.clearRect(0, 0, frame.width, frame.height);
  const focus = focusOf(scene, frame.selected);
  const screen = new Map<string, Point>();
  for (const vertex of scene.vertices) {
    const position = frame.positions.get(vertex.id);
    if (position) screen.set(vertex.id, toScreen(camera, position));
  }
  const alphaOf = (id: string) => (focus && !focus.has(id) ? DIM : 1);

  ctx.lineCap = "round";
  for (const link of scene.links) {
    const a = screen.get(link.source);
    const b = screen.get(link.target);
    if (!a || !b) continue;
    ctx.globalAlpha = alphaOf(link.id);
    const width = linkWidth(link.edge);
    if (frame.selected === link.id) {
      ctx.strokeStyle = palette.accent;
      ctx.lineWidth = width + 5;
      ctx.globalAlpha = 0.35;
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
      ctx.stroke();
      ctx.globalAlpha = 1;
    }
    ctx.strokeStyle = linkColor(palette, link);
    ctx.lineWidth = width;
    ctx.setLineDash(link.edge && !link.edge.up ? [5, 5] : link.edge ? [] : [2, 4]);
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.stroke();
  }
  ctx.setLineDash([]);

  if (frame.flows) drawFlows(ctx, frame, screen, alphaOf);
  if (scene.view === "rf" && camera.k >= 0.6) drawLinkLabels(ctx, frame, screen, alphaOf);

  for (const vertex of scene.vertices) {
    const p = screen.get(vertex.id);
    if (!p) continue;
    ctx.globalAlpha = alphaOf(vertex.id);
    drawVertex(ctx, frame, vertex, p);
  }

  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  ctx.lineJoin = "round";
  for (const vertex of scene.vertices) {
    const p = screen.get(vertex.id);
    if (!p) continue;
    const client = vertex.kind === "client";
    const marked = frame.selected === vertex.id || frame.hovered === vertex.id;
    // Clients are named from 1.6×, past what fitting zooms to: a fitted map stays legible.
    if (client && camera.k < 1.6 && !marked) continue;
    if (!client && camera.k < 0.35 && !marked) continue;
    ctx.globalAlpha = alphaOf(vertex.id);
    const r = screenRadius(vertex, camera);
    const rings = vertex.node?.state ? 9 : 0;
    ctx.font = `${client ? 400 : 500} ${client ? 11 : 12}px ${FONT}`;
    const label = truncate(vertex.label);
    const y = p.y + r + rings + 5;
    ctx.strokeStyle = palette.canvas;
    ctx.lineWidth = 3.5;
    ctx.strokeText(label, p.x, y);
    ctx.fillStyle = client ? palette.fgMuted : palette.fg;
    ctx.fillText(label, p.x, y);
  }
  ctx.globalAlpha = 1;
}

function drawVertex(ctx: CanvasRenderingContext2D, frame: Frame, vertex: SceneVertex, p: Point) {
  const { palette, camera } = frame;
  const r = screenRadius(vertex, camera);
  const circle = (radius: number) => {
    ctx.beginPath();
    ctx.arc(p.x, p.y, radius, 0, 2 * Math.PI);
  };

  if (vertex.kind === "client") {
    circle(r);
    ctx.fillStyle = vertex.band ? palette.band[vertex.band] : palette.fgSubtle;
    ctx.fill();
    ctx.strokeStyle = palette.canvas;
    ctx.lineWidth = 1.5;
    ctx.stroke();
  } else {
    circle(r);
    ctx.fillStyle = vertex.kind === "cluster" ? palette.raised : palette.surface;
    ctx.fill();
    ctx.strokeStyle = palette.edgeStrong;
    ctx.lineWidth = 1.5;
    ctx.stroke();
    if (vertex.kind === "cluster") {
      ctx.fillStyle = palette.fg;
      ctx.font = `600 ${Math.max(9, Math.min(13, r * 0.8))}px ${FONT}`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(String(vertex.members.length), p.x, p.y + 0.5);
    } else {
      const icon = frame.icons.get(vertex.kind);
      if (icon && r >= 7) {
        const size = r * 1.1;
        ctx.save();
        ctx.translate(p.x - size / 2, p.y - size / 2);
        ctx.scale(size / 24, size / 24);
        ctx.strokeStyle =
          vertex.node?.node != null || vertex.kind === "internet" ? palette.fg : palette.fgMuted;
        ctx.lineWidth = 2;
        ctx.lineCap = "round";
        ctx.lineJoin = "round";
        ctx.stroke(icon);
        ctx.restore();
      }
    }
  }

  const node = vertex.node;
  if (node?.state) {
    circle(r + 3.5);
    ctx.strokeStyle = stateColor(palette, node.state);
    ctx.lineWidth = 2.5;
    ctx.stroke();
    if (node.cpu_percent !== null) {
      circle(r + 7.5);
      ctx.strokeStyle = palette.edge;
      ctx.lineWidth = 2;
      ctx.stroke();
      const share = Math.min(1, Math.max(0, node.cpu_percent / 100));
      if (share > 0) {
        ctx.beginPath();
        ctx.arc(p.x, p.y, r + 7.5, -Math.PI / 2, -Math.PI / 2 + share * 2 * Math.PI);
        ctx.strokeStyle = loadColor(palette, node.cpu_percent);
        ctx.stroke();
      }
    }
  }

  if (frame.pinned.has(vertex.id)) {
    const angle = -Math.PI / 4;
    ctx.beginPath();
    ctx.arc(p.x + Math.cos(angle) * (r + 1), p.y + Math.sin(angle) * (r + 1), 3, 0, 2 * Math.PI);
    ctx.fillStyle = palette.accent;
    ctx.fill();
    ctx.strokeStyle = palette.canvas;
    ctx.lineWidth = 1.5;
    ctx.stroke();
  }

  const marked =
    frame.selected === vertex.id
      ? palette.accent
      : frame.hovered === vertex.id
        ? palette.edgeStrong
        : undefined;
  if (marked) {
    circle(r + (node?.state ? 12 : 5));
    ctx.strokeStyle = marked;
    ctx.lineWidth = 2;
    ctx.stroke();
  }
}

function drawFlows(
  ctx: CanvasRenderingContext2D,
  frame: Frame,
  screen: ReadonlyMap<string, Point>,
  alphaOf: (id: string) => number,
) {
  const { palette } = frame;
  for (const link of frame.scene.links) {
    const edge = link.edge;
    const traffic = edge?.traffic;
    if (!edge?.up || !traffic) continue;
    const a = screen.get(link.source);
    const b = screen.get(link.target);
    if (!a || !b) continue;
    const length = Math.hypot(b.x - a.x, b.y - a.y);
    if (length < 12) continue;
    ctx.globalAlpha = alphaOf(link.id);
    const radius = linkWidth(edge) > 4 ? 2.5 : 2;
    for (const [bps, from, to, color] of [
      [traffic.down_bps, a, b, palette.accent],
      [traffic.up_bps, b, a, palette.info],
    ] as const) {
      const flow = flowOf(bps);
      if (!flow) continue;
      ctx.fillStyle = color;
      for (let i = 0; i < flow.count; i++) {
        const t = particleAt(frame.seconds, flow.speed / length, i, flow.count);
        ctx.beginPath();
        ctx.arc(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t, radius, 0, 2 * Math.PI);
        ctx.fill();
      }
    }
  }
}

/** Mesh links in the RF view carry their signal, or batman-adv's TQ. */
function drawLinkLabels(
  ctx: CanvasRenderingContext2D,
  frame: Frame,
  screen: ReadonlyMap<string, Point>,
  alphaOf: (id: string) => number,
) {
  const { palette } = frame;
  ctx.font = `500 10px ${FONT}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  for (const link of frame.scene.links) {
    const edge = link.edge;
    if (!edge || (edge.medium !== "mesh" && edge.medium !== "wifi")) continue;
    const text =
      edge.signal_dbm !== null
        ? `${edge.signal_dbm} dBm`
        : edge.tq !== null
          ? `TQ ${Math.round((edge.tq / 255) * 100)} %`
          : undefined;
    const a = screen.get(link.source);
    const b = screen.get(link.target);
    if (!text || !a || !b || Math.hypot(b.x - a.x, b.y - a.y) < 70) continue;
    const x = (a.x + b.x) / 2;
    const y = (a.y + b.y) / 2;
    const width = ctx.measureText(text).width + 10;
    ctx.globalAlpha = alphaOf(link.id);
    ctx.fillStyle = palette.surface;
    ctx.strokeStyle = palette.edge;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.roundRect(x - width / 2, y - 8, width, 16, 8);
    ctx.fill();
    ctx.stroke();
    ctx.fillStyle = palette.fgMuted;
    ctx.fillText(text, x, y + 0.5);
  }
}
