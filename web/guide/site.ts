/**
 * The user guide's online copy (D-151, M9.4c): every chapter of `docs/guide/` as a static page for
 * GitHub Pages, with the screenshots CI takes from `serve --demo` (`web/shots/`). The chapters
 * render as the in-app help does (`render.ts`) but with a target of their own: headings carry
 * `id`s, links between chapters go to `<chapter>.html` and images show. An image must be one of
 * the screenshots (`PAGES_URL` + `images/<name>.png`, which the repo's copy shows too), and every
 * screenshot must be used: a missing or a stray one fails the build. `build-site.ts` writes it.
 */
import { type Chapter, escapeHtml, INDEX, PAGES_URL, renderGuide, type Target } from "./render";

/** A screenshot's size in pixels; CI takes them at twice the CSS size (`deviceScaleFactor: 2`). */
export interface ImageSize {
  width: number;
  height: number;
}

export interface SiteOptions {
  /** The release the guide describes, `0.42.0`. */
  version: string;
  /** The screenshots by file name (`devices.png`). */
  images: ReadonlyMap<string, ImageSize>;
}

/** The scale the screenshots are taken at. */
export const SCALE = 2;
const IMAGES = `${PAGES_URL}images/`;
const REPO = "https://github.com/robertzas/uniwrt";

const pageOf = (chapter: string) => `${chapter === INDEX ? "index" : chapter}.html`;
const sourceOf = (chapter: string) =>
  `${REPO}/blob/main/docs/guide/${chapter === INDEX ? "README" : chapter}.md`;

/** The guide's mark: the app's (`Wordmark.tsx`), a three-node mesh. */
const MARK = `<svg viewBox="0 0 24 24" class="mark" aria-hidden="true"><rect width="24" height="24" rx="6" class="mark-bg"/><path d="M12 6.75 6.75 16.25h10.5Z" class="mark-edge"/><circle cx="12" cy="6.75" r="2.25"/><circle cx="6.75" cy="16.25" r="2.25"/><circle cx="17.25" cy="16.25" r="2.25"/></svg>`;

function chapterList(chapters: readonly Chapter[], current: string): string {
  const items = chapters
    .map((c) => {
      const here = c.slug === current ? ' aria-current="page"' : "";
      return `<li><a href="${pageOf(c.slug)}"${here}>${escapeHtml(c.slug === INDEX ? "Contents" : c.title)}</a></li>`;
    })
    .join("");
  return `<ol class="chapters">${items}</ol>`;
}

function page(chapters: readonly Chapter[], at: number, version: string): string {
  const chapter = chapters[at] as Chapter;
  const prev = chapters[at - 1];
  const next = chapters[at + 1];
  const toc = chapter.sections.filter((s) => s.depth === 2);
  const pager = [
    prev
      ? `<a class="prev" href="${pageOf(prev.slug)}"><span>Previous</span>${escapeHtml(prev.title)}</a>`
      : "<span></span>",
    next
      ? `<a class="next" href="${pageOf(next.slug)}"><span>Next</span>${escapeHtml(next.title)}</a>`
      : "<span></span>",
  ].join("");
  const title =
    chapter.slug === INDEX ? "uniwrt user guide" : `${chapter.title} · uniwrt user guide`;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(title)}</title>
<meta name="description" content="The user guide of uniwrt ${escapeHtml(version)}, a controller for OpenWrt mesh networks.">
<link rel="stylesheet" href="style.css">
<link rel="icon" href="favicon.svg" type="image/svg+xml">
</head>
<body>
<a class="skip" href="#content">Skip to the content</a>
<header class="top">
<a class="brand" href="index.html">${MARK}<span>uniwrt</span><span class="brand-sub">User guide</span></a>
<span class="version">v${escapeHtml(version)}</span>
<a class="repo" href="${REPO}">GitHub</a>
</header>
<div class="layout">
<nav class="sidebar" aria-label="Chapters">${chapterList(chapters, chapter.slug)}</nav>
<main id="content">
<details class="mobile-nav"><summary>Chapters</summary>${chapterList(chapters, chapter.slug)}</details>
<article class="guide">
${chapter.html}</article>
<nav class="pager" aria-label="Chapters">${pager}</nav>
<footer class="foot">The guide for uniwrt v${escapeHtml(version)}, with screenshots of its demo mode. <a href="${sourceOf(chapter.slug)}">Edit this page on GitHub</a>.</footer>
</main>
${
  toc.length > 0
    ? `<nav class="toc" aria-label="On this page"><p>On this page</p><ol>${toc
        .map((s) => `<li><a href="#${s.anchor}">${escapeHtml(s.text)}</a></li>`)
        .join("")}</ol></nav>`
    : ""
}
</div>
</body>
</html>
`;
}

/** The site's pages by file name (`index.html`, `devices.html`, …) from the guide's Markdown files. */
export function buildSite(
  files: Readonly<Record<string, string>>,
  options: SiteOptions,
): Record<string, string> {
  const used = new Set<string>();
  const problems: string[] = [];
  const target: Target = {
    anchor: "id",
    link: (chapter, anchor) => `${pageOf(chapter)}${anchor ? `#${anchor}` : ""}`,
    image(url, alt, from) {
      const name = url.startsWith(IMAGES) ? url.slice(IMAGES.length) : undefined;
      const size = name === undefined ? undefined : options.images.get(name);
      if (name === undefined || !size) {
        problems.push(`${from}.md shows "${url}", which isn't a screenshot`);
        return "";
      }
      if (!alt) problems.push(`${from}.md shows ${name} without a description`);
      used.add(name);
      const width = Math.round(size.width / SCALE);
      const height = Math.round(size.height / SCALE);
      return `<a class="shot" href="images/${escapeHtml(name)}"><img src="images/${escapeHtml(name)}" alt="${escapeHtml(alt)}" width="${width}" height="${height}" loading="lazy" decoding="async"></a>`;
    },
  };
  const chapters = renderGuide(files, target);
  for (const name of options.images.keys()) {
    if (!used.has(name)) problems.push(`the screenshot ${name} isn't shown anywhere`);
  }
  if (problems.length > 0) throw new Error(`The guide's site: ${problems.join("; ")}`);
  return Object.fromEntries(
    chapters.map((c, i) => [pageOf(c.slug), page(chapters, i, options.version)]),
  );
}
