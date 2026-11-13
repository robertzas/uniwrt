/**
 * Plans are normalized in the browser (D-117): any image it can open (PNG, JPEG, WebP, GIF,
 * SVG) is drawn at up to 4096 px on its longest side and encoded as WebP (PNG where the browser
 * can't encode WebP), which drops metadata (a photo's location) and scripts. Vector plans are
 * drawn at the full 4096 px so they stay sharp when zoomed. The image is read as a `data:` URL,
 * which the CSP's `img-src` allows (`blob:` isn't).
 */
import { fitWithin } from "./units";

export const MAX_SIDE = 4096;
const MIN_SIDE = 16;
/** Larger files than any plan needs are refused before decoding. */
const MAX_FILE_BYTES = 64 * 1024 * 1024;

export class PlanError extends Error {}

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new PlanError("unreadable"));
    reader.readAsDataURL(file);
  });
}

function load(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new PlanError("unreadable"));
    image.src = src;
  });
}

function encode(canvas: HTMLCanvasElement, type: string): Promise<Blob | null> {
  return new Promise((resolve) => canvas.toBlob(resolve, type, 0.9));
}

/** The plan to upload; a [`PlanError`] names why a file can't be one. */
export async function normalizePlan(file: File): Promise<Blob> {
  if (file.size > MAX_FILE_BYTES) throw new PlanError("too_large");
  const image = await load(await readAsDataUrl(file));
  const { naturalWidth: width, naturalHeight: height } = image;
  if (width === 0 || height === 0) throw new PlanError("no_size");
  const vector = file.type === "image/svg+xml";
  const factor = vector ? MAX_SIDE / Math.max(width, height) : fitWithin(width, height, MAX_SIDE);
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(width * factor));
  canvas.height = Math.max(1, Math.round(height * factor));
  if (canvas.width < MIN_SIDE || canvas.height < MIN_SIDE) throw new PlanError("too_small");
  const context = canvas.getContext("2d");
  if (!context) throw new PlanError("unreadable");
  // Transparent plans (many SVGs and PNGs) read best on white, like paper.
  context.fillStyle = "#fff";
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.drawImage(image, 0, 0, canvas.width, canvas.height);
  const webp = await encode(canvas, "image/webp");
  if (webp?.type === "image/webp") return webp;
  const png = await encode(canvas, "image/png");
  if (!png) throw new PlanError("unreadable");
  return png;
}
