/**
 * A plan's footprint (D-118): the part of the drawing walls enclose, found in the browser from
 * the plan's pixels each time it loads. Pixels unlike the border's background are drawing; drawn
 * cells grow by `closeCells` so doorways close; the blank cells reached from the edge are
 * outside, the rest inside, which then shrinks back by `closeCells`. A drawing that encloses
 * almost nothing or almost everything (a photo, an open plan) counts whole (`detected` false).
 */
import type { Mask } from "./model";

/** Each cell is read from this many pixels on a side. */
export const SUB = 4;
/** How unlike the background a channel must be for a pixel to be drawing. */
const INK = 40;
const MIN_SHARE = 0.03;
const MAX_SHARE = 0.97;

/** RGBA pixels, as `ImageData` holds them. */
export interface Pixels {
  width: number;
  height: number;
  data: Uint8ClampedArray;
}

function whole(cols: number, rows: number): Mask {
  return { cols, rows, inside: new Uint8Array(cols * rows).fill(1), detected: false };
}

/** The median of each channel along the image's border. */
function background(p: Pixels): [number, number, number] {
  const channels: [number[], number[], number[]] = [[], [], []];
  const take = (x: number, y: number) => {
    const i = (y * p.width + x) * 4;
    for (let k = 0; k < 3; k++) channels[k]?.push(p.data[i + k] ?? 0);
  };
  for (let x = 0; x < p.width; x++) {
    take(x, 0);
    take(x, p.height - 1);
  }
  for (let y = 1; y < p.height - 1; y++) {
    take(0, y);
    take(p.width - 1, y);
  }
  const median = (values: number[]) => values.sort((a, b) => a - b)[values.length >> 1] ?? 255;
  return [median(channels[0]), median(channels[1]), median(channels[2])];
}

/** Cells within `r` (a disk) of any set cell. */
function grow(set: Uint8Array, cols: number, rows: number, r: number): Uint8Array {
  const out = new Uint8Array(set.length);
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) {
      if (!set[y * cols + x]) continue;
      for (let dy = -r; dy <= r; dy++) {
        const yy = y + dy;
        if (yy < 0 || yy >= rows) continue;
        for (let dx = -r; dx <= r; dx++) {
          const xx = x + dx;
          if (xx >= 0 && xx < cols && dx * dx + dy * dy <= r * r) out[yy * cols + xx] = 1;
        }
      }
    }
  }
  return out;
}

/** The footprint of `p`, read at `SUB` pixels per cell, on a `cols` × `rows` grid. */
export function footprint(p: Pixels, cols: number, rows: number, closeCells: number): Mask {
  if (p.width < cols || p.height < rows) return whole(cols, rows);
  const bg = background(p);
  const drawn = new Uint8Array(cols * rows);
  for (let y = 0; y < p.height; y++) {
    const row = Math.min(rows - 1, Math.floor((y * rows) / p.height));
    for (let x = 0; x < p.width; x++) {
      const i = (y * p.width + x) * 4;
      const ink =
        Math.abs((p.data[i] ?? 0) - bg[0]) > INK ||
        Math.abs((p.data[i + 1] ?? 0) - bg[1]) > INK ||
        Math.abs((p.data[i + 2] ?? 0) - bg[2]) > INK;
      if (ink) drawn[row * cols + Math.min(cols - 1, Math.floor((x * cols) / p.width))] = 1;
    }
  }
  const walls = grow(drawn, cols, rows, closeCells);
  // Flood the blank cells from the edge: they're outside.
  const outside = new Uint8Array(cols * rows);
  const stack: number[] = [];
  const visit = (i: number) => {
    if (!walls[i] && !outside[i]) {
      outside[i] = 1;
      stack.push(i);
    }
  };
  for (let x = 0; x < cols; x++) {
    visit(x);
    visit((rows - 1) * cols + x);
  }
  for (let y = 0; y < rows; y++) {
    visit(y * cols);
    visit(y * cols + cols - 1);
  }
  while (stack.length > 0) {
    const i = stack.pop() as number;
    const x = i % cols;
    if (x > 0) visit(i - 1);
    if (x < cols - 1) visit(i + 1);
    if (i >= cols) visit(i - cols);
    if (i < (rows - 1) * cols) visit(i + cols);
  }
  // Shrink back what growing the walls added outside them.
  const inside = grow(outside, cols, rows, closeCells).map((v) => 1 - v);
  let count = 0;
  for (const v of inside) count += v;
  const share = count / inside.length;
  if (share < MIN_SHARE || share > MAX_SHARE) return whole(cols, rows);
  return { cols, rows, inside, detected: true };
}

/** Reads the plan at `src` and finds its footprint; the whole plan where it can't be read. */
export async function loadFootprint(
  src: string,
  cols: number,
  rows: number,
  closeCells: number,
): Promise<Mask> {
  const image = new Image();
  image.decoding = "async";
  image.src = src;
  try {
    await image.decode();
  } catch {
    return whole(cols, rows);
  }
  const canvas = document.createElement("canvas");
  canvas.width = cols * SUB;
  canvas.height = rows * SUB;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) return whole(cols, rows);
  context.fillStyle = "#fff";
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.drawImage(image, 0, 0, canvas.width, canvas.height);
  return footprint(context.getImageData(0, 0, canvas.width, canvas.height), cols, rows, closeCells);
}
