/**
 * Writes the user guide's online copy (`site.ts`) for GitHub Pages:
 * `bun guide/build-site.ts --images <screenshots> --out <dir> --version <x.y.z>`.
 * The guide lands in `<dir>/guide/` (`PAGES_URL`), and `<dir>/index.html` sends visitors there.
 */
import { copyFileSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { stdout } from "node:process";
import { parseArgs } from "node:util";
import { buildSite, type ImageSize } from "./site";

const { values } = parseArgs({
  options: {
    images: { type: "string" },
    out: { type: "string" },
    version: { type: "string" },
  },
});
const { images, out, version } = values;
if (!images || !out || !version) {
  throw new Error("usage: build-site.ts --images <dir> --out <dir> --version <x.y.z>");
}

/** A PNG's size, from its IHDR chunk. */
function pngSize(file: string): ImageSize {
  const bytes = readFileSync(file);
  if (bytes.toString("latin1", 1, 4) !== "PNG") throw new Error(`${file} isn't a PNG`);
  return { width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
}

const here = import.meta.dirname;
const docs = join(here, "../../docs/guide");
const files = Object.fromEntries(
  readdirSync(docs)
    .filter((name) => name.endsWith(".md"))
    .map((name) => [name, readFileSync(join(docs, name), "utf8")]),
);
const shots = new Map(
  readdirSync(images)
    .filter((name) => name.endsWith(".png"))
    .map((name) => [name, pngSize(join(images, name))]),
);
const pages = buildSite(files, { version, images: shots });

const guide = join(out, "guide");
rmSync(out, { recursive: true, force: true });
mkdirSync(join(guide, "images"), { recursive: true });
mkdirSync(join(guide, "fonts"));
for (const [name, html] of Object.entries(pages)) writeFileSync(join(guide, name), html);
for (const name of shots.keys()) copyFileSync(join(images, name), join(guide, "images", name));
copyFileSync(join(here, "site.css"), join(guide, "style.css"));
copyFileSync(join(here, "favicon.svg"), join(guide, "favicon.svg"));
copyFileSync(
  join(here, "../node_modules/@fontsource-variable/inter/files/inter-latin-wght-normal.woff2"),
  join(guide, "fonts", "inter.woff2"),
);
writeFileSync(
  join(out, "index.html"),
  '<!doctype html>\n<meta charset="utf-8">\n<title>uniwrt user guide</title>\n<meta http-equiv="refresh" content="0; url=guide/">\n<link rel="canonical" href="guide/">\n<a href="guide/">The uniwrt user guide</a>\n',
);
stdout.write(`${Object.keys(pages).length} pages and ${shots.size} screenshots in ${guide}\n`);
