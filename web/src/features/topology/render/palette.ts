import type { Band } from "../../../api/gen/Band";
import type { NodeState } from "../../../api/gen/NodeState";

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
  canvas: "canvas",
  surface: "surface",
  raised: "raised",
  edge: "edge",
  edgeStrong: "edge-strong",
  fg: "fg",
  fgMuted: "fg-muted",
  fgSubtle: "fg-subtle",
  accent: "accent",
  ok: "ok",
  warn: "warn",
  crit: "crit",
  info: "info",
  band2g: "band-2g",
  band5g: "band-5g",
  band6g: "band-6g",
  link1: "link-1",
  link2: "link-2",
  link3: "link-3",
  link4: "link-4",
  link5: "link-5",
} as const;

/**
 * Reads the tokens through a probe element: a custom property holds `light-dark(…)` unresolved,
 * but a `color` set from it computes to the theme's color, which a canvas accepts.
 */
export function readPalette(host: Element): Palette {
  const probe = document.createElement("span");
  probe.hidden = true;
  host.append(probe);
  const read = (token: string) => {
    probe.style.color = `var(--color-${token})`;
    return getComputedStyle(probe).color || "currentColor";
  };
  const c = Object.fromEntries(
    Object.entries(tokens).map(([key, token]) => [key, read(token)]),
  ) as Record<keyof typeof tokens, string>;
  probe.remove();
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
