/// <reference types="node" />
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

/*
 * Checks the contrast promises in tokens.css for every skin and theme. Translucent layers (glass)
 * are composited the way browsers do, in gamma-encoded sRGB, over the canvas and over the peak of
 * each glow behind it.
 */

// Read from disk: Vitest blanks CSS imports, `?raw` included.
const css = readFileSync(`${import.meta.dirname}/tokens.css`, "utf8");

type Rgba = [number, number, number, number];
type Theme = "light" | "dark";

/** The body of the block that opens at `selector`, from the first match at or after `from`. */
function block(selector: string, from = 0): string {
  const start = css.indexOf(`${selector} {`, from);
  if (start < 0) throw new Error(`no block ${selector}`);
  let depth = 0;
  for (let i = css.indexOf("{", start); i < css.length; i++) {
    if (css[i] === "{") depth++;
    if (css[i] === "}" && --depth === 0) return css.slice(css.indexOf("{", start) + 1, i);
  }
  throw new Error(`unclosed block ${selector}`);
}

function declarations(body: string): Map<string, string> {
  const out = new Map<string, string>();
  for (const [, name, value] of body.matchAll(/--([\w-]+):\s*([^;]+);/g)) {
    if (name && value) out.set(name, value.replace(/\s+/g, " ").trim());
  }
  return out;
}

const base = declarations(block("@theme"));
const glass = declarations(block(':root[data-skin="glass"]'));
const media = css.indexOf("@media (prefers-reduced-transparency: reduce)");
const skins: Record<string, Map<string, string>> = {
  graphite: base,
  glass: new Map([...base, ...glass]),
  "glass (opaque)": new Map([
    ...base,
    ...glass,
    ...declarations(block(':root[data-skin="glass"]', media)),
  ]),
  unifi: new Map([...base, ...declarations(block(':root[data-skin="unifi"]'))]),
};

/** Splits the top-level, comma-separated arguments of a CSS function call. */
function args(call: string): string[] {
  const inner = call.slice(call.indexOf("(") + 1, call.lastIndexOf(")"));
  const out: string[] = [];
  let depth = 0;
  let current = "";
  for (const ch of inner) {
    if (ch === "(") depth++;
    if (ch === ")") depth--;
    if (ch === "," && depth === 0) {
      out.push(current.trim());
      current = "";
    } else {
      current += ch;
    }
  }
  return [...out, current.trim()];
}

function resolve(tokens: Map<string, string>, value: string, theme: Theme): string {
  const ref = value.match(/^var\(--([\w-]+)\)$/);
  if (ref?.[1]) return resolve(tokens, token(tokens, ref[1]), theme);
  if (value.startsWith("light-dark(")) {
    const [light = "", dark = ""] = args(value);
    return resolve(tokens, theme === "light" ? light : dark, theme);
  }
  return value;
}

function token(tokens: Map<string, string>, name: string): string {
  const value = tokens.get(name);
  if (value === undefined) throw new Error(`no token --${name}`);
  return value;
}

const toGamma = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);
const toLinear = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const clamp = (c: number) => Math.min(1, Math.max(0, c));

/** oklch() or `transparent` as gamma-encoded sRGB with alpha. */
function parse(color: string): Rgba {
  if (color === "transparent") return [0, 0, 0, 0];
  const m = color.match(/^oklch\(([\d.]+) ([\d.]+) ([\d.]+)(?: \/ ([\d.]+))?\)$/);
  if (!m) throw new Error(`unsupported color ${color}`);
  const [l, c, h, alpha] = [Number(m[1]), Number(m[2]), Number(m[3]), Number(m[4] ?? 1)];
  const a = c * Math.cos((h * Math.PI) / 180);
  const b = c * Math.sin((h * Math.PI) / 180);
  const lms = [
    (l + 0.3963377774 * a + 0.2158037573 * b) ** 3,
    (l - 0.1055613458 * a - 0.0638541728 * b) ** 3,
    (l - 0.0894841775 * a - 1.291485548 * b) ** 3,
  ] as const;
  const rgb = [
    4.0767416621 * lms[0] - 3.3077115913 * lms[1] + 0.2309699292 * lms[2],
    -1.2684380046 * lms[0] + 2.6097574011 * lms[1] - 0.3413193965 * lms[2],
    -0.0041960863 * lms[0] - 0.7034186147 * lms[1] + 1.707614701 * lms[2],
  ];
  return [
    toGamma(clamp(rgb[0] ?? 0)),
    toGamma(clamp(rgb[1] ?? 0)),
    toGamma(clamp(rgb[2] ?? 0)),
    alpha,
  ];
}

/** `top` painted over an opaque `bottom`. */
function over(top: Rgba, bottom: Rgba): Rgba {
  const [r, g, b, a] = top;
  return [r * a + bottom[0] * (1 - a), g * a + bottom[1] * (1 - a), b * a + bottom[2] * (1 - a), 1];
}

function luminance([r, g, b]: Rgba): number {
  return 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
}

function contrast(fg: Rgba, bg: Rgba): number {
  const [hi, lo] = [luminance(fg), luminance(bg)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

/** The opaque grounds text can sit on: canvas, surface and raised, over every backdrop. */
function grounds(tokens: Map<string, string>, theme: Theme): Map<string, Rgba> {
  const color = (name: string) => parse(resolve(tokens, token(tokens, `color-${name}`), theme));
  const canvas = color("canvas");
  const backdrops: [string, Rgba][] = [["canvas", canvas]];
  const image = token(tokens, "canvas-image");
  if (image !== "none") {
    for (const [i, gradient] of args(`(${image})`).entries()) {
      const peak = args(gradient)[1] ?? "";
      backdrops.push([`glow ${i + 1}`, over(parse(resolve(tokens, peak, theme)), canvas)]);
    }
  }
  const out = new Map<string, Rgba>();
  for (const [name, backdrop] of backdrops) {
    const surface = over(color("surface"), backdrop);
    out.set(name, backdrop);
    out.set(`surface on ${name}`, surface);
    out.set(`raised on ${name}`, over(color("raised"), backdrop));
    out.set(`raised in a surface on ${name}`, over(color("raised"), surface));
  }
  return out;
}

const text = ["fg", "fg-muted", "fg-subtle", "accent"];
const indicators = [
  "edge-strong",
  ...["ok", "warn", "crit", "info"],
  ...["band-2g", "band-5g", "band-6g"],
  ...["link-1", "link-2", "link-3", "link-4", "link-5"],
];

describe.each(Object.entries(skins))("%s skin", (_, tokens) => {
  describe.each(["light", "dark"] as const)("%s theme", (theme) => {
    const color = (name: string) => parse(resolve(tokens, token(tokens, `color-${name}`), theme));
    const misses = (names: string[], min: number) => {
      const out: string[] = [];
      for (const [ground, bg] of grounds(tokens, theme)) {
        for (const name of names) {
          const ratio = contrast(color(name), bg);
          if (ratio < min) out.push(`${name} on ${ground}: ${ratio.toFixed(2)}`);
        }
      }
      return out;
    };

    it("text tiers and accent reach 4.5:1 on every ground", () => {
      expect(misses(text, 4.5)).toEqual([]);
    });

    it("indicators and control outlines reach 3:1 on every ground", () => {
      expect(misses(indicators, 3)).toEqual([]);
    });

    it("on-accent text reaches 4.5:1 on the accent fill", () => {
      expect(contrast(color("on-accent"), color("accent-fill"))).toBeGreaterThanOrEqual(4.5);
    });

    it("the current page stays readable in the navigation", () => {
      const out: string[] = [];
      for (const [ground, bg] of grounds(tokens, theme)) {
        if (!ground.startsWith("surface")) continue;
        const ratio = contrast(color("nav-active-fg"), over(color("nav-active"), bg));
        if (ratio < 4.5) out.push(`${ground}: ${ratio.toFixed(2)}`);
      }
      expect(out).toEqual([]);
    });
  });
});
