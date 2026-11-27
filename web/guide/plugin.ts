/**
 * The Vite plugin that embeds the user guide (D-151): `virtual:uniwrt-guide` is its index (each
 * chapter's title and sections) with `loadChapter(slug)`, which imports that chapter's HTML from
 * `virtual:uniwrt-guide/<slug>`, a chunk of its own. Both load only when help is opened, so the
 * initial bundle doesn't grow. A changed chapter reloads the dev server's page.
 */
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import type { Plugin } from "vite";
import { type Chapter, renderGuide } from "./render";

const INDEX_ID = "virtual:uniwrt-guide";
const CHAPTER_ID = `${INDEX_ID}/`;

export function guide(dir: string): Plugin {
  let cache: Chapter[] | undefined;
  const files = () => readdirSync(dir).filter((name) => name.endsWith(".md"));
  const chapters = () => {
    cache ??= renderGuide(
      Object.fromEntries(files().map((name) => [name, readFileSync(join(dir, name), "utf8")])),
    );
    return cache;
  };
  return {
    name: "uniwrt-guide",
    resolveId(id) {
      return id === INDEX_ID || id.startsWith(CHAPTER_ID) ? `\0${id}` : undefined;
    },
    load(id) {
      if (!id.startsWith(`\0${INDEX_ID}`)) return undefined;
      for (const name of files()) this.addWatchFile(join(dir, name));
      const all = chapters();
      if (id === `\0${INDEX_ID}`) {
        const index = all.map(({ html: _, ...chapter }) => chapter);
        const loaders = all
          .map(
            (c) =>
              `  ${JSON.stringify(c.slug)}: () => import(${JSON.stringify(CHAPTER_ID + c.slug)}),`,
          )
          .join("\n");
        return [
          `export const chapters = ${JSON.stringify(index)};`,
          `const loaders = {\n${loaders}\n};`,
          "export function loadChapter(slug) {",
          "  const load = Object.hasOwn(loaders, slug) ? loaders[slug] : undefined;",
          "  return load ? load().then((m) => m.html) : Promise.resolve(undefined);",
          "}",
        ].join("\n");
      }
      const chapter = all.find((c) => `\0${CHAPTER_ID}${c.slug}` === id);
      return chapter ? `export const html = ${JSON.stringify(chapter.html)};` : undefined;
    },
    watchChange(file) {
      if (file.startsWith(dir)) cache = undefined;
    },
    configureServer(server) {
      server.watcher.add(dir);
      server.watcher.on("change", (file) => {
        if (!file.startsWith(dir) || !file.endsWith(".md")) return;
        cache = undefined;
        for (const module of server.moduleGraph.idToModuleMap.values()) {
          if (module.id?.startsWith(`\0${INDEX_ID}`)) server.moduleGraph.invalidateModule(module);
        }
        server.ws.send({ type: "full-reload" });
      });
    },
  };
}
