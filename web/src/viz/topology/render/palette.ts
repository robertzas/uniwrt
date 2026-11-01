import type { Band } from "../../../api/gen/Band";
import type { NodeState } from "../../../api/gen/NodeState";
import { resolveColors, token } from "../../theme";

/** The design tokens the canvas paints with, resolved to colors for the current skin and theme. */
export interface Palette {
  canvas: string;
  surface: string;
  raised: string;
  edge: string;
  edgeStrong: string;
  fg: string;
  fgMuted: string;
  fgSubtle: string;
  accent: string;
  ok: string;
  warn: string;
  crit: string;
  info: string;
  band: Record<Band, string>;
  /** The link-quality ramp, poor to excellent. */
  link: readonly [string, string, string, string, string];
}

const tokens = {
  canvas: token("canvas"),
  surface: token("surface"),
  raised: token("raised"),
  edge: token("edge"),
  edgeStrong: token("edge-strong"),
  fg: token("fg"),
  fgMuted: token("fg-muted"),
  fgSubtle: token("fg-subtle"),
  accent: token("accent"),
  ok: token("ok"),
  warn: token("warn"),
  crit: token("crit"),
  info: token("info"),
  band2g: token("band-2g"),
  band5g: token("band-5g"),
  band6g: token("band-6g"),
  link1: token("link-1"),
  link2: token("link-2"),
  link3: token("link-3"),
  link4: token("link-4"),
  link5: token("link-5"),
};

/** The palette for the current skin and theme, resolved under `host`. */
export function readPalette(host: Element): Palette {
  const c = resolveColors(host, tokens);
  return {
    canvas: c.canvas,
    surface: c.surface,
    raised: c.raised,
    edge: c.edge,
    edgeStrong: c.edgeStrong,
    fg: c.fg,
    fgMuted: c.fgMuted,
    fgSubtle: c.fgSubtle,
    accent: c.accent,
    ok: c.ok,
    warn: c.warn,
    crit: c.crit,
    info: c.info,
    band: { "2g": c.band2g, "5g": c.band5g, "6g": c.band6g, "60g": c.band6g },
    link: [c.link1, c.link2, c.link3, c.link4, c.link5],
  };
}

/** The step of the quality ramp (0–4) for a quality of 0–100. */
export function qualityStep(quality: number): 0 | 1 | 2 | 3 | 4 {
  return Math.min(4, Math.max(0, Math.floor(quality / 20))) as 0 | 1 | 2 | 3 | 4;
}

export function stateColor(palette: Palette, state: NodeState): string {
  return state === "online" ? palette.ok : state === "degraded" ? palette.warn : palette.crit;
}

/** CPU load: fine below 60 %, busy below 85 %, else saturated. */
export function loadColor(palette: Palette, percent: number): string {
  return percent < 60 ? palette.ok : percent < 85 ? palette.warn : palette.crit;
}
