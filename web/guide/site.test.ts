import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { PAGES_URL } from "./render";
import { buildSite } from "./site";

const shot = (name: string) => `${PAGES_URL}images/${name}`;
const size = { width: 2560, height: 1600 };

describe("the guide's site", () => {
  it("renders each chapter as a page with ids, .html links and the screenshots", () => {
    const pages = buildSite(
      {
        "README.md": "# Guide\n\n[Devices](devices.md)",
        "devices.md": `# Devices\n\n## Discovered\n\n![The list](${shot("devices.png")})\n\nSee [the index](README.md) and [above](#discovered).`,
      },
      { version: "1.2.3", images: new Map([["devices.png", size]]) },
    );
    expect(Object.keys(pages)).toEqual(["index.html", "devices.html"]);
    const html = pages["devices.html"] ?? "";
    expect(html).toContain('<h2 id="discovered">Discovered</h2>');
    expect(html).toContain('<a href="index.html">the index</a>');
    expect(html).toContain('<a href="devices.html#discovered">above</a>');
    expect(html).toContain(
      '<img src="images/devices.png" alt="The list" width="1280" height="800" loading="lazy" decoding="async">',
    );
    expect(html).toContain('<a href="devices.html" aria-current="page">Devices</a>');
    expect(html).toContain('<a class="prev" href="index.html">');
    expect(html).toContain("v1.2.3");
    expect(html).toContain('<li><a href="#discovered">Discovered</a></li>');
  });

  it("refuses an image that isn't a screenshot, a missing one and a stray one", () => {
    const build = (markdown: string, images: string[]) => () =>
      buildSite(
        { "README.md": markdown },
        { version: "1", images: new Map(images.map((name) => [name, size])) },
      );
    expect(build("![A](https://example.com/a.png)", [])).toThrow("isn't a screenshot");
    expect(build(`![A](${shot("a.png")})`, [])).toThrow("isn't a screenshot");
    expect(build(`![](${shot("a.png")})`, ["a.png"])).toThrow("without a description");
    expect(build("# Guide", ["a.png"])).toThrow("a.png isn't shown anywhere");
  });

  it("shows exactly the screenshots the shots script takes", () => {
    const dir = join(import.meta.dirname, "../../docs/guide");
    const shown = readdirSync(dir)
      .filter((name) => name.endsWith(".md"))
      .flatMap((name) => [
        ...readFileSync(join(dir, name), "utf8").matchAll(/!\[[^\]]*\]\(([^)]+)\)/g),
      ])
      .map((m) => m[1]?.replace(`${PAGES_URL}images/`, "").replace(/\.png$/, ""));
    const script = readFileSync(join(import.meta.dirname, "../shots/shots.spec.ts"), "utf8");
    const taken = [
      ...script.matchAll(/shoot\(\s*page,\s*"([\w-]+)"/g),
      ...script.matchAll(/\["([\w-]+)", "\//g),
    ].map((m) => m[1]);
    expect(new Set(shown)).toEqual(new Set(taken));
    expect(shown).toHaveLength(new Set(shown).size);
  });
});
