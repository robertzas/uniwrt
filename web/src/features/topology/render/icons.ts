import type { IconNode } from "lucide";

type Attrs = Record<string, string | number | undefined>;

const num = (attrs: Attrs, key: string) => Number(attrs[key] ?? 0);

/** An ellipse as two arcs. */
function ellipse(cx: number, cy: number, rx: number, ry: number): string {
  return `M${cx - rx} ${cy}a${rx} ${ry} 0 1 0 ${2 * rx} 0a${rx} ${ry} 0 1 0 ${-2 * rx} 0Z`;
}

function rect(attrs: Attrs): string {
  const [x, y, w, h] = [
    num(attrs, "x"),
    num(attrs, "y"),
    num(attrs, "width"),
    num(attrs, "height"),
  ];
  const r = Math.min(num(attrs, "rx") || num(attrs, "ry"), w / 2, h / 2);
  if (r <= 0) return `M${x} ${y}h${w}v${h}h${-w}Z`;
  return (
    `M${x + r} ${y}h${w - 2 * r}a${r} ${r} 0 0 1 ${r} ${r}v${h - 2 * r}` +
    `a${r} ${r} 0 0 1 ${-r} ${r}h${-(w - 2 * r)}a${r} ${r} 0 0 1 ${-r} ${-r}` +
    `v${-(h - 2 * r)}a${r} ${r} 0 0 1 ${r} ${-r}Z`
  );
}

function points(list: string | number | undefined, close: boolean): string {
  const values = String(list ?? "")
    .trim()
    .split(/[\s,]+/)
    .map(Number);
  let d = "";
  for (let i = 0; i + 1 < values.length; i += 2)
    d += `${i === 0 ? "M" : "L"}${values[i]} ${values[i + 1]}`;
  return close ? `${d}Z` : d;
}

/**
 * One SVG path for a Lucide icon's shapes, in its 24 × 24 box, for a canvas to stroke as a
 * Path2D: the canvas draws the same icons as the rest of the UI.
 */
export function iconPathData(icon: IconNode): string {
  return icon
    .map(([tag, raw]) => {
      const attrs = raw as Attrs;
      switch (tag) {
        case "path":
          return String(attrs.d ?? "");
        case "circle":
          return ellipse(num(attrs, "cx"), num(attrs, "cy"), num(attrs, "r"), num(attrs, "r"));
        case "ellipse":
          return ellipse(num(attrs, "cx"), num(attrs, "cy"), num(attrs, "rx"), num(attrs, "ry"));
        case "rect":
          return rect(attrs);
        case "line":
          return `M${num(attrs, "x1")} ${num(attrs, "y1")}L${num(attrs, "x2")} ${num(attrs, "y2")}`;
        case "polyline":
          return points(attrs.points, false);
        case "polygon":
          return points(attrs.points, true);
        default:
          return "";
      }
    })
    .filter((d) => d !== "")
    .join(" ");
}
