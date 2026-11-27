/**
 * The user guide (`docs/guide/*.md`, D-151) rendered to HTML at build time, for the in-app help.
 * It runs in Node only (the Vite plugin in `plugin.ts`): the UI ships the HTML, never a Markdown
 * parser. Headings get GitHub's anchors as `data-anchor`, so a link written for the repo's copy
 * (`devices.md#discovered`) finds the same section here. Links between chapters become
 * `/help/<chapter>#<anchor>` and are checked: a missing chapter or section fails the build. Raw
 * HTML is escaped and nothing carries a `style` attribute (the CSP allows no inline styles).
 * Images stay text only: each becomes a link to the online copy (`PAGES_URL`). The same renderer
 * builds that online copy (`site.ts`, M9.4c) with a `Target` of its own: `id`s, `.html` links
 * and the screenshots shown.
 */
import { Marked, Renderer, type Tokens } from "marked";

/** Where the guide is published with its screenshots (M9.4c); images link there. */
export const PAGES_URL = "https://robertzas.github.io/uniwrt/guide/";

/** The guide's index page, `README.md`, which `/help` shows. */
export const INDEX = "index";

export interface Section {
  /** GitHub's anchor for the heading. */
  anchor: string;
  /** The heading as plain text. */
  text: string;
  /** 2 for `##`, 3 for `###`, … (the `#` title isn't a section). */
  depth: number;
}

export interface Chapter {
  /** The file's name without `.md`; `index` for README.md. */
  slug: string;
  title: string;
  sections: Section[];
  html: string;
}

/** How the HTML refers to things, which differs between the in-app help and the Pages copy. */
export interface Target {
  /** The attribute that carries a heading's anchor. */
  anchor: "data-anchor" | "id";
  /** Where a link to a chapter (and a section in it) goes. */
  link(chapter: string, anchor?: string): string;
  /** An image's HTML; `url` is absolute (a relative `src` is read against `PAGES_URL`). */
  image(url: string, alt: string, from: string): string;
}

/** The in-app help: `data-anchor`s, `/help/…` links and images as links to the online copy. */
export const APP: Target = {
  anchor: "data-anchor",
  link: helpPath,
  image: (url, alt) =>
    `<a href="${escapeHtml(url)}" target="_blank" rel="noopener noreferrer">${escapeHtml(alt || url)}</a>`,
};

/** A link from one chapter to another (or to a section of itself), for the check. */
export interface GuideLink {
  from: string;
  chapter: string;
  anchor?: string;
}

const entities: Record<string, string> = {
  "&amp;": "&",
  "&lt;": "<",
  "&gt;": ">",
  "&quot;": '"',
  "&#39;": "'",
};

export function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

/** The visible text of inline tokens, as GitHub slugs it (entities decoded). */
function plain(tokens: readonly Tokens.Generic[] | undefined): string {
  return (tokens ?? [])
    .map((token) =>
      token.tokens ? plain(token.tokens) : typeof token.text === "string" ? token.text : "",
    )
    .join("")
    .replace(/&(amp|lt|gt|quot|#39);/g, (e) => entities[e] ?? e);
}

/**
 * GitHub's heading anchors (github-slugger): lower case, punctuation dropped, each space a hyphen;
 * a repeated anchor gets `-1`, `-2`, … One slugger per document.
 */
export function createSlugger(): (text: string) => string {
  const seen = new Map<string, number>();
  return (text) => {
    const base = text
      .trim()
      .toLowerCase()
      .replace(/[^\p{L}\p{M}\p{N}\p{Pc} -]/gu, "")
      .replace(/ /g, "-");
    let slug = base;
    let n = seen.get(base) ?? 0;
    while (seen.has(slug)) slug = `${base}-${++n}`;
    seen.set(base, n);
    seen.set(slug, 0);
    return slug;
  };
}

/** The in-app path of a chapter and section. */
export function helpPath(chapter: string, anchor?: string): string {
  return `${chapter === INDEX ? "/help" : `/help/${chapter}`}${anchor ? `#${anchor}` : ""}`;
}

/**
 * One chapter. `href`s are rewritten here; `renderGuide` checks the links it returns.
 */
export function renderChapter(
  slug: string,
  markdown: string,
  target: Target = APP,
): Chapter & { links: GuideLink[] } {
  const slugger = createSlugger();
  const sections: Section[] = [];
  const links: GuideLink[] = [];
  let title = "";
  const base = new Renderer();
  const marked = new Marked({
    gfm: true,
    renderer: {
      heading({ tokens, depth }) {
        const text = plain(tokens);
        const anchor = slugger(text);
        if (depth === 1 && !title) title = text;
        else sections.push({ anchor, text, depth });
        return `<h${depth} ${target.anchor}="${anchor}">${this.parser.parseInline(tokens)}</h${depth}>\n`;
      },
      link({ href, tokens }) {
        const inner = this.parser.parseInline(tokens);
        if (/^(https?|mailto):/i.test(href)) {
          return `<a href="${escapeHtml(href)}" target="_blank" rel="noopener noreferrer">${inner}</a>`;
        }
        const local = /^(?:([\w-]+)\.md)?(?:#([\w-]+))?$/.exec(href);
        if (!local || (!local[1] && !local[2])) {
          throw new Error(`${slug}.md: the link "${href}" is neither a chapter nor a web address`);
        }
        const chapter = local[1] ? (local[1] === "README" ? INDEX : local[1]) : slug;
        links.push({ from: slug, chapter, ...(local[2] && { anchor: local[2] }) });
        return `<a href="${target.link(chapter, local[2])}">${inner}</a>`;
      },
      image({ href, text }) {
        const url = /^https?:/i.test(href) ? href : new URL(href, PAGES_URL).href;
        return target.image(url, text, slug);
      },
      // Wide tables scroll inside a wrapper on a phone, keeping their table semantics.
      table(token) {
        return `<div class="guide-table">${base.table.call(this, token)}</div>\n`;
      },
      html({ text }) {
        return escapeHtml(text);
      },
    },
  });
  const html = marked.parse(markdown, { async: false });
  if (/<[^>]*\sstyle=/i.test(html))
    throw new Error(`${slug}.md: the rendered HTML carries a style`);
  return { slug, title: title || slug, sections, html, links };
}

/**
 * Every chapter, in the order README.md first links them (the rest after, by name), with every
 * link between them checked. `files` maps file names (`devices.md`) to their Markdown.
 */
export function renderGuide(
  files: Readonly<Record<string, string>>,
  target: Target = APP,
): Chapter[] {
  const rendered = Object.entries(files)
    .filter(([name]) => name.endsWith(".md"))
    .map(([name, markdown]) => {
      const base = name.slice(0, -3);
      return renderChapter(base === "README" ? INDEX : base, markdown, target);
    });
  const bySlug = new Map(rendered.map((c) => [c.slug, c]));
  const broken = rendered.flatMap((c) =>
    c.links
      .filter((l) => {
        const target = bySlug.get(l.chapter);
        return !target || (l.anchor && !target.sections.some((s) => s.anchor === l.anchor));
      })
      .map(
        (l) =>
          `${l.from === INDEX ? "README" : l.from}.md → ${l.chapter}${l.anchor ? `#${l.anchor}` : ""}`,
      ),
  );
  if (broken.length > 0) throw new Error(`The guide has broken links: ${broken.join(", ")}`);
  const order = [
    INDEX,
    ...new Set(
      bySlug
        .get(INDEX)
        ?.links.map((l) => l.chapter)
        .filter((s) => s !== INDEX),
    ),
  ];
  const rank = (slug: string) => (order.includes(slug) ? order.indexOf(slug) : order.length);
  return rendered
    .sort((a, b) => rank(a.slug) - rank(b.slug) || a.slug.localeCompare(b.slug))
    .map(({ links: _, ...chapter }) => chapter);
}
