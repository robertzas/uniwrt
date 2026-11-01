/** Design tokens for canvases: resolved to colors for the current skin and theme, and their changes. */

/**
 * Resolves CSS colors (token references such as `var(--color-accent)`, or any color expression)
 * through a probe element: a custom property holds `light-dark(…)` unresolved, but a `color` set
 * from it computes to the theme's color, which a canvas accepts.
 */
export function resolveColors<K extends string>(
  host: Element,
  colors: Record<K, string>,
): Record<K, string> {
  const probe = document.createElement("span");
  probe.hidden = true;
  host.append(probe);
  const out = {} as Record<K, string>;
  for (const key of Object.keys(colors) as K[]) {
    probe.style.color = colors[key];
    out[key] = getComputedStyle(probe).color || "currentColor";
  }
  probe.remove();
  return out;
}

/** A token as a color expression: `token("accent")`, or `token("accent", 0.15)` for a tint. */
export function token(name: string, alpha?: number): string {
  const color = `var(--color-${name})`;
  return alpha === undefined
    ? color
    : `color-mix(in srgb, ${color} ${Math.round(alpha * 100)}%, transparent)`;
}

/**
 * Calls `onChange` when the skin or theme changes: the `data-theme`/`data-skin` attributes, or the
 * system's color scheme. Returns a function that stops watching.
 */
export function watchTheme(onChange: () => void): () => void {
  const attributes = new MutationObserver(onChange);
  attributes.observe(document.documentElement, { attributeFilter: ["data-theme", "data-skin"] });
  const dark = matchMedia("(prefers-color-scheme: dark)");
  dark.addEventListener("change", onChange);
  return () => {
    attributes.disconnect();
    dark.removeEventListener("change", onChange);
  };
}
