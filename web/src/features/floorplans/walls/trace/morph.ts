/**
 * The binary morphology the wall tracer needs (M6.2c2): chamfer distances, so growing or
 * shrinking a mask by any radius costs two sweeps over the image.
 */

/** A mask over a `width` × `height` image, 1 where set. */
export interface Bits {
  width: number;
  height: number;
  data: Uint8Array;
}

const FAR = 0xffff;

/**
 * The 3-4 chamfer distance from each pixel to the nearest pixel where `set` is `want`, in thirds
 * of a pixel (within about 8 % of the Euclidean distance); `FAR` where there is none.
 */
function distance(set: Bits, want: number): Uint16Array {
  const { width: w, height: h, data } = set;
  const d = new Uint16Array(w * h);
  for (let i = 0; i < d.length; i++) d[i] = data[i] === want ? 0 : FAR;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = y * w + x;
      let v = d[i] as number;
      if (v === 0) continue;
      if (x > 0) v = Math.min(v, (d[i - 1] as number) + 3);
      if (y > 0) {
        v = Math.min(v, (d[i - w] as number) + 3);
        if (x > 0) v = Math.min(v, (d[i - w - 1] as number) + 4);
        if (x < w - 1) v = Math.min(v, (d[i - w + 1] as number) + 4);
      }
      d[i] = v;
    }
  }
  for (let y = h - 1; y >= 0; y--) {
    for (let x = w - 1; x >= 0; x--) {
      const i = y * w + x;
      let v = d[i] as number;
      if (v === 0) continue;
      if (x < w - 1) v = Math.min(v, (d[i + 1] as number) + 3);
      if (y < h - 1) {
        v = Math.min(v, (d[i + w] as number) + 3);
        if (x < w - 1) v = Math.min(v, (d[i + w + 1] as number) + 4);
        if (x > 0) v = Math.min(v, (d[i + w - 1] as number) + 4);
      }
      d[i] = v;
    }
  }
  return d;
}

/** The pixels within `r` pixels of a set one. */
function grow(set: Bits, r: number): Bits {
  if (r <= 0) return set;
  const d = distance(set, 1);
  const limit = r * 3;
  const data = new Uint8Array(d.length);
  for (let i = 0; i < d.length; i++) data[i] = (d[i] as number) <= limit ? 1 : 0;
  return { width: set.width, height: set.height, data };
}

/** The set pixels farther than `r` pixels from an unset one (the image's edge doesn't count). */
function shrink(set: Bits, r: number): Bits {
  if (r <= 0) return set;
  const d = distance(set, 0);
  const limit = r * 3;
  const data = new Uint8Array(d.length);
  for (let i = 0; i < d.length; i++) data[i] = (d[i] as number) > limit ? 1 : 0;
  return { width: set.width, height: set.height, data };
}

/** Strokes thinner than about `2r` removed: text, dimension lines, hatching, furniture. */
export const open = (set: Bits, r: number): Bits => grow(shrink(set, r), r);
